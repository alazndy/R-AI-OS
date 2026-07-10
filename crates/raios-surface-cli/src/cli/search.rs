use std::path::Path;

pub(super) fn cmd_search(query: &str, top_k: usize, reindex: bool, scope: &Path, json: bool) {
    let mut vector_hits = None;

    if reindex {
        if let Some(indexed) = daemon_cortex_reindex(scope) {
            if !json {
                println!("Cortex: Re-indexed {} via daemon ({} chunks).", scope.display(), indexed);
            }
            vector_hits = daemon_vector_search(query, top_k, scope);
        }
    } else {
        vector_hits = daemon_vector_search(query, top_k, scope);
    }

    let vector_hits = match vector_hits {
        Some(hits) => {
            if !json {
                println!("Cortex: Query served by resident daemon.");
            }
            hits
        }
        None => {
            if !json {
                println!("Cortex: Daemon unreachable. Falling back to local in-process search...");
            }
            let mut cortex = match raios_runtime::cortex::Cortex::init() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Cortex init failed: {e:?}");
                    return;
                }
            };
            let needs_index = reindex || cortex.chunk_count() == 0;

            if needs_index {
                if !json {
                    if reindex {
                        println!("Cortex: Re-indexing {} (forced)...", scope.display());
                    } else {
                        println!("Cortex: First run — indexing {}...", scope.display());
                    }
                }
                let indexed = cortex.index_project(scope).unwrap_or(0);
                if !json {
                    println!("Indexed {} chunks. Searching...\n", indexed);
                }
            } else if !json {
                println!(
                    "Cortex: {} chunks loaded from cache. Searching...\n",
                    cortex.chunk_count()
                );
            }

            cortex.search_scoped(query, top_k, scope).unwrap_or_default()
        }
    };
    let bm25_hits = match raios_runtime::indexer::ProjectIndex::load_or_build(
        scope,
        &raios_runtime::cortex::store::default_db_path(),
        reindex,
    ) {
        Ok(idx) => idx.search(query),
        Err(e) => {
            eprintln!("Index build failed: {e}");
            vec![]
        }
    };
    let fused = raios_runtime::hybrid_search::fuse(bm25_hits, vector_hits, top_k);

    if json {
        let results: Vec<serde_json::Value> = fused
            .iter()
            .map(|r| {
                serde_json::json!({
                    "path": r.path.to_string_lossy(), "project": r.project, "snippet": r.snippet,
                    "line": r.start_line, "score": r.rrf_score, "source": r.source.label()
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&results).unwrap_or_default()
        );
        return;
    }

    println!("\nSearch Results for: '{}'", query);
    println!("{}", "─".repeat(72));
    if fused.is_empty() {
        println!("No results found.");
        return;
    }

    for r in fused {
        let source_tag = match r.source {
            raios_runtime::hybrid_search::ResultSource::VectorOnly => "Semantic",
            raios_runtime::hybrid_search::ResultSource::BM25Only => "Keyword ",
            raios_runtime::hybrid_search::ResultSource::Hybrid => "Hybrid  ",
        };
        println!(
            "[{}] {:<30} (score: {:.4})",
            source_tag, r.project, r.rrf_score
        );
        println!("  Path: {}", r.path.to_string_lossy());
        println!("  Line: {}", r.start_line);
        println!(
            "  Snippet: \"{}...\"",
            r.snippet
                .chars()
                .take(120)
                .collect::<String>()
                .replace('\n', " ")
        );
        println!();
    }
}

pub(super) fn cmd_cortex_index(force: bool, dev_ops: &Path, json: bool) {
    let mut cortex = match raios_runtime::cortex::Cortex::init() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Cortex init failed: {e:?}");
            std::process::exit(1);
        }
    };

    if !force && cortex.chunk_count() > 0 {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "status":"already_indexed",
                    "mode": embedding_mode(),
                    "files": cortex.file_count(),
                    "chunks": cortex.chunk_count()
                })
            );
        } else {
            println!(
                "Cortex index — embedding mode: {}\nFiles indexed: {} | Chunks: {}",
                embedding_mode(),
                cortex.file_count(),
                cortex.chunk_count()
            );
        }
        return;
    }

    if !json {
        println!("Indexing workspace… (this may take a minute on first run)");
    }
    match cortex.index_workspace(dev_ops) {
        Ok(n) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "status":"ok",
                        "mode": embedding_mode(),
                        "files": cortex.file_count(),
                        "chunks": cortex.chunk_count(),
                        "indexed": n
                    })
                );
            } else {
                println!(
                    "Cortex index — embedding mode: {}\nFiles indexed: {} | Chunks: {}",
                    embedding_mode(),
                    cortex.file_count(),
                    cortex.chunk_count()
                );
            }
        }
        Err(e) => eprintln!("Indexing failed: {e}"),
    }
}

fn embedding_mode() -> &'static str {
    #[cfg(feature = "cortex")]
    {
        "real (all-MiniLM-L6-v2)"
    }
    #[cfg(not(feature = "cortex"))]
    {
        "fallback"
    }
}

fn daemon_vector_search(query: &str, top_k: usize, scope: &Path)
    -> Option<Vec<raios_runtime::cortex::store::VectorResult>> {
    daemon_vector_search_opt(query, top_k, scope, None)
}

fn daemon_vector_search_opt(query: &str, top_k: usize, scope: &Path, port: Option<u16>)
    -> Option<Vec<raios_runtime::cortex::store::VectorResult>> {
    use std::io::{BufRead, BufReader, Write};
    let port = port.unwrap_or(42069);
    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().ok()?;
    let stream = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300)).ok()?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(15))).ok()?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(2))).ok()?;
    let mut writer = stream.try_clone().ok()?;
    let token_path = raios_core::config::Config::config_file()
        .parent().map(|p| p.to_path_buf()).unwrap_or_default().join(".session_token");
    let token = std::fs::read_to_string(token_path).ok()?;
    writer.write_all(format!("AUTH {}\n", token.trim()).as_bytes()).ok()?;
    let req = serde_json::json!({
        "command": "VectorSearch", "query": query, "top_k": top_k,
        "scope": scope.to_string_lossy(),
    });
    writer.write_all(format!("{req}\n").as_bytes()).ok()?;
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = line.ok()?;
        let v: serde_json::Value = serde_json::from_str(&line).ok()?;
        if v["event"] == "VectorResults" {
            return serde_json::from_value(v["vector_hits"].clone()).ok();
        }
    }
    None
}

fn daemon_cortex_reindex(scope: &Path) -> Option<usize> {
    daemon_cortex_reindex_opt(scope, None)
}

fn daemon_cortex_reindex_opt(scope: &Path, port: Option<u16>) -> Option<usize> {
    use std::io::{BufRead, BufReader, Write};
    let port = port.unwrap_or(42069);
    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().ok()?;
    let stream = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300)).ok()?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(65))).ok()?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(2))).ok()?;
    let mut writer = stream.try_clone().ok()?;
    let token_path = raios_core::config::Config::config_file()
        .parent().map(|p| p.to_path_buf()).unwrap_or_default().join(".session_token");
    let token = std::fs::read_to_string(token_path).ok()?;
    writer.write_all(format!("AUTH {}\n", token.trim()).as_bytes()).ok()?;
    let req = serde_json::json!({
        "command": "CortexReindex",
        "scope": scope.to_string_lossy(),
    });
    writer.write_all(format!("{req}\n").as_bytes()).ok()?;
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = line.ok()?;
        let v: serde_json::Value = serde_json::from_str(&line).ok()?;
        if v["event"] == "CortexReindexed" {
            return v["indexed"].as_u64().map(|n| n as usize);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_vector_search_returns_none_when_unreachable() {
        let start = std::time::Instant::now();
        let hits = daemon_vector_search_opt("test", 5, Path::new("."), Some(1));
        assert!(hits.is_none());
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    }
}
