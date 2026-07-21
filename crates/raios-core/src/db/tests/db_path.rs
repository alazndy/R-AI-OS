use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn db_path_respects_raios_db_path_env_override() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original = std::env::var("RAIOS_DB_PATH").ok();

    std::env::set_var("RAIOS_DB_PATH", "/tmp/raios-test-override/workspace.db");
    let resolved = super::db_path();

    match original {
        Some(v) => std::env::set_var("RAIOS_DB_PATH", v),
        None => std::env::remove_var("RAIOS_DB_PATH"),
    }

    assert_eq!(
        resolved,
        std::path::PathBuf::from("/tmp/raios-test-override/workspace.db")
    );
}
