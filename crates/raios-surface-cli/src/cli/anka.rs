use super::AnkaAction;

/// Structural CLI boundary for ANKA. Indexing and recall are intentionally not
/// implemented until the parser, redaction, cache schema, and promotion policy
/// receive architectural approval.
pub(super) fn cmd_anka(action: AnkaAction, json: bool) {
    let result = match action {
        AnkaAction::Status => raios_runtime::anka::status().map(|status| {
            serde_json::json!({
                "state": if status.last_indexed_at.is_some() { "ready" } else { "empty" },
                "cache_path": status.cache_path,
                "indexed_sources": status.indexed_sources,
                "indexed_records": status.indexed_records,
                "last_indexed_at": status.last_indexed_at,
            })
        }),
        AnkaAction::Index { harness } => harness
            .as_deref()
            .map(raios_runtime::anka::parse_harness)
            .transpose()
            .and_then(raios_runtime::anka::index)
            .map(|status| {
                serde_json::json!({
                    "state": "ready",
                    "cache_path": status.cache_path,
                    "indexed_sources": status.indexed_sources,
                    "indexed_records": status.indexed_records,
                    "last_indexed_at": status.last_indexed_at,
                })
            }),
        AnkaAction::Search {
            query,
            project,
            harness,
            limit,
        } => harness
            .as_deref()
            .map(raios_runtime::anka::parse_harness)
            .transpose()
            .and_then(|harness| {
                raios_runtime::anka::search(raios_core::anka::AnkaSearchQuery {
                    text: query,
                    project,
                    harness,
                    limit,
                })
            })
            .map(|hits| serde_json::json!({"hits": hits})),
        AnkaAction::Blame { path, limit } => {
            raios_runtime::anka::blame(&path, limit).map(|hits| serde_json::json!({"hits": hits}))
        }
        AnkaAction::Forget { id } => raios_runtime::anka::forget(&id)
            .map(|forgotten| serde_json::json!({"forgotten": forgotten, "id": id})),
    };

    match result {
        Ok(payload) if json => println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        ),
        Ok(payload) => print_human(&payload),
        Err(error) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"ok": false, "error": error.to_string()})
                );
            } else {
                eprintln!("ANKA failed: {error}");
            }
            std::process::exit(1);
        }
    }
}

fn print_human(payload: &serde_json::Value) {
    if let Some(hits) = payload.get("hits").and_then(serde_json::Value::as_array) {
        if hits.is_empty() {
            println!("No matching ANKA evidence.");
            return;
        }
        for hit in hits {
            let source = &hit["source"];
            println!(
                "{}  [{}] {} · {}",
                hit["id"].as_str().unwrap_or("unknown"),
                source["harness"].as_str().unwrap_or("unknown"),
                source["project"].as_str().unwrap_or("unknown"),
                source["occurred_at"].as_str().unwrap_or("unknown"),
            );
            println!("  {}", hit["snippet"].as_str().unwrap_or(""));
        }
        return;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(payload).unwrap_or_default()
    );
}
