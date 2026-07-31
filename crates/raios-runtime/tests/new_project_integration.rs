//! End-to-end coverage for `raios_runtime::new_project::create`, exercising
//! the real filesystem + git scaffolding path (no network / gh CLI calls).
//!
//! `create()` always writes to `entities.json` via
//! `raios_core::db::open_db()` (step 8, `add_to_entities`/`save_entities`) —
//! that call ignores the `dev_ops` argument for DB routing and unconditionally
//! resolves to `~/.config/raios/workspace.db` unless `RAIOS_DB_PATH` is set.
//! Every test in this file MUST point `RAIOS_DB_PATH` at a tempdir before
//! calling `create()`, or it will read and permanently write rows into the
//! developer's real global database. Env vars are process-global (not
//! thread-scoped) and Rust runs tests in the same binary on parallel
//! threads, so `ENV_LOCK` serializes the set/restore around each test body —
//! mirrors the pattern in `crates/raios-core/src/db/tests/db_path.rs`.

use raios_runtime::new_project::{create, NewProjectConfig};
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn create_scaffolds_a_project_without_fabricating_a_github_url() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original_db_path = std::env::var("RAIOS_DB_PATH").ok();

    let dev_ops = tempfile::tempdir().expect("tempdir");
    std::env::set_var("RAIOS_DB_PATH", dev_ops.path().join("test-workspace.db"));

    let cfg = NewProjectConfig {
        name: "sample-project",
        category: "tools",
        dev_ops: dev_ops.path(),
        github: false,
        no_vault: true,
        vault_path: None,
    };

    let result = create(&cfg);

    match original_db_path {
        Some(v) => std::env::set_var("RAIOS_DB_PATH", v),
        None => std::env::remove_var("RAIOS_DB_PATH"),
    }

    let project_dir = dev_ops.path().join("tools").join("sample-project");
    assert_eq!(result.path, project_dir);
    assert!(project_dir.join("code").is_dir());
    assert!(project_dir.join("reference").is_dir());
    assert!(project_dir.join("public").is_dir());
    assert!(project_dir.join(".git").is_dir());

    let memory = std::fs::read_to_string(project_dir.join("memory.md")).expect("memory.md");
    assert!(memory.contains("sample-project"));

    // With github integration off, no GitHub repo was created, so no URL
    // should ever be fabricated from a hardcoded owner — the placeholder
    // written in step 4 must survive untouched.
    let gitrepo = std::fs::read_to_string(project_dir.join("gitrepo.md")).expect("gitrepo.md");
    assert!(gitrepo.contains("**GitHub:** TBD"));
    assert!(result.github_url.is_none());
    assert!(!gitrepo.contains("github.com"));

    let all_steps_have_a_description = result.steps.iter().all(|(desc, _)| !desc.is_empty());
    assert!(all_steps_have_a_description);
}

#[test]
fn create_writes_a_vault_note_when_vault_is_enabled() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original_db_path = std::env::var("RAIOS_DB_PATH").ok();

    let dev_ops = tempfile::tempdir().expect("dev_ops tempdir");
    let vault = tempfile::tempdir().expect("vault tempdir");
    std::env::set_var("RAIOS_DB_PATH", dev_ops.path().join("test-workspace.db"));

    let cfg = NewProjectConfig {
        name: "vaulted-project",
        category: "tools",
        dev_ops: dev_ops.path(),
        github: false,
        no_vault: false,
        vault_path: Some(vault.path()),
    };

    let result = create(&cfg);

    match original_db_path {
        Some(v) => std::env::set_var("RAIOS_DB_PATH", v),
        None => std::env::remove_var("RAIOS_DB_PATH"),
    }

    let vault_ok = result
        .steps
        .iter()
        .find(|(desc, _)| desc == "Update Obsidian Vault")
        .map(|(_, ok)| *ok)
        .unwrap_or(false);
    assert!(vault_ok, "vault step should succeed: {:?}", result.steps);

    let note_path = vault
        .path()
        .join("Projeler")
        .join("tools")
        .join("vaulted-project.md");
    assert!(note_path.exists(), "expected vault note at {note_path:?}");
}
