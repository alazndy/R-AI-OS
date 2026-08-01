use std::path::{Path, PathBuf};

pub(super) fn cmd_obsidian_sync(vault: Option<String>, dry_run: bool, dev_ops: &Path, json: bool) {
    let vault_path: PathBuf = vault
        .map(PathBuf::from)
        .unwrap_or_else(raios_runtime::obsidian::default_vault_path);

    let report = raios_runtime::obsidian::sync_vault(dev_ops, &vault_path, dry_run);

    if json {
        let out = serde_json::json!({
            "vault": vault_path.display().to_string(),
            "dry_run": dry_run,
            "written": report.written,
            "errors": report.errors,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        if !report.errors.is_empty() {
            std::process::exit(1);
        }
        return;
    }

    let verb = if dry_run { "would write" } else { "wrote" };
    println!(
        "Obsidian vault sync — {} note(s) {} to {}",
        report.written,
        verb,
        vault_path.display()
    );
    if !report.errors.is_empty() {
        println!("\nErrors:");
        for e in &report.errors {
            println!("  ✗ {e}");
        }
        std::process::exit(1);
    }
}
