use std::path::Path;

pub(super) fn cmd_locate(pattern: &str, scope: &Path, ignore_case: bool, reindex: bool, json: bool) {
    let matches = match raios_runtime::search::trigram::locate(
        scope,
        &raios_runtime::cortex::store::default_db_path(),
        pattern,
        ignore_case,
        reindex,
    ) {
        Ok(matches) => matches,
        Err(e) => {
            eprintln!("locate failed: {e}");
            std::process::exit(1);
        }
    };

    if json {
        let results: Vec<serde_json::Value> = matches
            .iter()
            .map(|m| {
                serde_json::json!({
                    "path": m.path.to_string_lossy(),
                    "line": m.line_no,
                    "text": m.line,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&results).unwrap_or_default()
        );
        return;
    }

    if matches.is_empty() {
        eprintln!("no matches");
        return;
    }

    for m in matches {
        println!("{}:{}:{}", m.path.display(), m.line_no, m.line);
    }
}
