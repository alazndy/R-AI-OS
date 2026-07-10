use std::path::Path;

pub(super) fn cmd_search(query: &str, top_k: usize, reindex: bool, scope: &Path, json: bool) {
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

    let vector_hits = cortex.search_scoped(query, top_k, scope).unwrap_or_default();
    let bm25_hits = match raios_runtime::indexer::ProjectIndex::build(scope) {
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
