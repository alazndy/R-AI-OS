use super::AnkaAction;

/// ANKA's CLI boundary. Every action delegates to `raios_runtime::anka` and
/// serializes the shared contract types (hits, and `AnkaIndexStatusDto` for
/// status/index) — the surface renders, it never computes state.
pub(super) fn cmd_anka(action: AnkaAction, json: bool) {
    let result = match action {
        // Status and Index serialize the shared `AnkaIndexStatusDto` — the same
        // contract type the MCP surface speaks — instead of hand-building a
        // mirror of it. `state` now comes from the runtime's coverage
        // computation; it used to be guessed here from `last_indexed_at`.
        AnkaAction::Status => raios_runtime::anka::status().and_then(|status| {
            serde_json::to_value(raios_runtime::anka::status_dto(status)).map_err(Into::into)
        }),
        AnkaAction::Index { harness } => harness
            .as_deref()
            .map(raios_runtime::anka::parse_harness)
            .transpose()
            .and_then(raios_runtime::anka::index)
            .and_then(|status| {
                serde_json::to_value(raios_runtime::anka::status_dto(status)).map_err(Into::into)
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
        // Consent and diagnostics: `policy-init` returns the view of what it
        // wrote (it never reads the cache), `policy-show` the resolved rules,
        // tombstone count, and the kept/excluded breakdown of the current one.
        AnkaAction::PolicyInit { home } => raios_runtime::anka::policy_init(home.into())
            .and_then(|view| serde_json::to_value(view).map_err(Into::into)),
        AnkaAction::PolicyShow => raios_runtime::anka::policy_show()
            .and_then(|view| serde_json::to_value(view).map_err(Into::into)),
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
