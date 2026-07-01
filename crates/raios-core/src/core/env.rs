use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvFile {
    pub name: String,
    pub exists: bool,
    pub key_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvReport {
    pub files: Vec<EnvFile>,
    pub has_env: bool,
    pub has_example: bool,
    /// Keys in .env.example but missing from .env
    pub missing_keys: Vec<String>,
    /// Keys in .env with empty value (KEY= or KEY="")
    pub empty_keys: Vec<String>,
    /// Keys in .env but absent from .env.example
    pub undocumented_keys: Vec<String>,
    pub total_env_keys: usize,
    pub total_example_keys: usize,
    pub ok: bool,
}

// ─── Public API ──────────────────────────────────────────────────────────────

pub fn check(dir: &Path) -> EnvReport {
    let tracked = [
        ".env",
        ".env.local",
        ".env.development",
        ".env.production",
        ".env.example",
        ".env.sample",
        ".env.template",
    ];

    let files: Vec<EnvFile> = tracked
        .iter()
        .map(|name| {
            let path = dir.join(name);
            EnvFile {
                name: (*name).to_string(),
                exists: path.exists(),
                key_count: if path.exists() {
                    parse_keys(&path).len()
                } else {
                    0
                },
            }
        })
        .collect();

    let env_path = dir.join(".env");
    let example_path = find_example(dir);
    let has_env = env_path.exists();
    let has_example = example_path.is_some();

    let env_keys: HashSet<String> = if has_env {
        parse_keys(&env_path).into_iter().collect()
    } else {
        HashSet::new()
    };

    let example_keys: HashSet<String> = example_path
        .as_ref()
        .map(|p| parse_keys(p).into_iter().collect())
        .unwrap_or_default();

    let empty_keys = if has_env {
        parse_empty_keys(&env_path)
    } else {
        vec![]
    };
    let missing_keys = sorted_diff(&example_keys, &env_keys);
    let undocumented_keys = if has_example {
        sorted_diff(&env_keys, &example_keys)
    } else {
        vec![]
    };

    let ok = has_env && missing_keys.is_empty() && empty_keys.is_empty();

    EnvReport {
        files,
        has_env,
        has_example,
        missing_keys,
        empty_keys,
        undocumented_keys,
        total_env_keys: env_keys.len(),
        total_example_keys: example_keys.len(),
        ok,
    }
}

// ─── Parsers ─────────────────────────────────────────────────────────────────

fn parse_keys(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(extract_key)
        .collect()
}

fn parse_empty_keys(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                return None;
            }
            let eq = line.find('=')?;
            let key = line[..eq].trim().to_string();
            let value = line[eq + 1..].trim().trim_matches('"').trim_matches('\'');
            if value.is_empty() {
                Some(key)
            } else {
                None
            }
        })
        .collect()
}

fn extract_key(line: &str) -> Option<String> {
    let line = line.trim();
    if line.starts_with('#') || line.is_empty() {
        return None;
    }
    let key = line.split('=').next()?.trim().to_string();
    if key.is_empty() || key.contains(' ') {
        None
    } else {
        Some(key)
    }
}

fn find_example(dir: &Path) -> Option<std::path::PathBuf> {
    [".env.example", ".env.sample", ".env.template"]
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.exists())
}

fn sorted_diff(a: &HashSet<String>, b: &HashSet<String>) -> Vec<String> {
    let mut v: Vec<String> = a.difference(b).cloned().collect();
    v.sort();
    v
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(id: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("raios_env_{}", id));
        let _ = std::fs::create_dir_all(&p);
        p
    }

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn no_files_reports_not_ok() {
        let dir = tmp("none");
        let r = check(&dir);
        assert!(!r.has_env);
        assert!(!r.has_example);
        assert!(!r.ok);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn all_keys_present_is_ok() {
        let dir = tmp("full");
        write(&dir, ".env.example", "DB_URL=\nAPI_KEY=\n");
        write(
            &dir,
            ".env",
            "DB_URL=postgres://localhost\nAPI_KEY=secret\n",
        );
        let r = check(&dir);
        assert!(r.ok);
        assert!(r.missing_keys.is_empty());
        assert!(r.empty_keys.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_key_detected() {
        let dir = tmp("miss");
        write(&dir, ".env.example", "DB_URL=\nAPI_KEY=\nSECRET=\n");
        write(&dir, ".env", "DB_URL=postgres\nAPI_KEY=k\n");
        let r = check(&dir);
        assert_eq!(r.missing_keys, vec!["SECRET"]);
        assert!(!r.ok);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_value_detected() {
        let dir = tmp("empty");
        write(&dir, ".env.example", "DB_URL=\n");
        write(&dir, ".env", "DB_URL=\n");
        let r = check(&dir);
        assert!(r.empty_keys.contains(&"DB_URL".to_string()));
        assert!(!r.ok);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undocumented_key_detected() {
        let dir = tmp("undoc");
        write(&dir, ".env.example", "DB_URL=\n");
        write(&dir, ".env", "DB_URL=postgres\nSECRET_TOKEN=xyz\n");
        let r = check(&dir);
        assert!(r.undocumented_keys.contains(&"SECRET_TOKEN".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn comments_and_blanks_ignored() {
        let dir = tmp("cmts");
        write(
            &dir,
            ".env",
            "# comment\n\nDB_URL=val\n  # another\nAPI=v\n",
        );
        let r = check(&dir);
        assert_eq!(r.total_env_keys, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
