//! Best-effort, cached check for a newer raios release. Never blocks or
//! errors the command it's attached to — any failure (offline, GitHub
//! down, unparseable response) is silent. Skipped entirely for
//! non-interactive/piped output so it never pollutes scripted or
//! machine-consumed (`--json`) usage.

use serde::{Deserialize, Serialize};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
const CURL_TIMEOUT_SECS: &str = "3";
const RELEASES_LATEST_URL: &str = "https://api.github.com/repos/alazndy/R-AI-OS/releases/latest";
const INSTALL_CMD: &str =
    "curl -fsSL https://raw.githubusercontent.com/alazndy/R-AI-OS/master/scripts/get-raios.sh | sh";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateCache {
    checked_at: u64,
    /// `None` means the last check failed (offline, rate-limited, etc.) —
    /// still cached so a command doesn't pay a curl round-trip on every
    /// single invocation until the interval elapses again.
    latest_tag: Option<String>,
    #[serde(default)]
    changelog_summary: String,
}

fn cache_path() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("raios")
            .join("update-check-cache.json"),
    )
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Prints a short notice on stderr if a newer raios release exists.
pub fn maybe_notify_update() {
    if !std::io::stdout().is_terminal() {
        return;
    }
    let Some(path) = cache_path() else { return };
    let Some(cache) = load_or_refresh_cache(&path) else {
        return;
    };
    let Some(latest_tag) = &cache.latest_tag else {
        return;
    };
    let current = env!("CARGO_PKG_VERSION");
    if !is_newer(latest_tag, current) {
        return;
    }

    eprintln!();
    eprintln!("  \u{2726} raios {latest_tag} is available (you have v{current})");
    if !cache.changelog_summary.is_empty() {
        eprintln!("    {}", cache.changelog_summary);
    }
    eprintln!("    Update: {INSTALL_CMD}");
}

fn load_or_refresh_cache(path: &Path) -> Option<UpdateCache> {
    if let Ok(content) = std::fs::read_to_string(path) {
        if let Ok(cache) = serde_json::from_str::<UpdateCache>(&content) {
            if now_secs().saturating_sub(cache.checked_at) < CHECK_INTERVAL_SECS {
                return Some(cache);
            }
        }
    }
    Some(refresh_cache(path))
}

fn refresh_cache(path: &Path) -> UpdateCache {
    let output = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            CURL_TIMEOUT_SECS,
            RELEASES_LATEST_URL,
        ])
        .output()
        .ok();

    let cache = match output {
        Some(out) if out.status.success() => parse_release_response(&out.stdout),
        _ => UpdateCache {
            checked_at: now_secs(),
            latest_tag: None,
            changelog_summary: String::new(),
        },
    };

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        path,
        serde_json::to_string_pretty(&cache).unwrap_or_default(),
    );

    cache
}

fn parse_release_response(body: &[u8]) -> UpdateCache {
    let parsed: serde_json::Value = serde_json::from_slice(body).unwrap_or(serde_json::Value::Null);
    let tag = parsed
        .get("tag_name")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    // GitHub's auto-generated release body (the "## What's Changed" PR-list
    // format this repo's release workflow produces) starts with a markdown
    // header line before the actual bullet list — skip header lines (and, on
    // this repo, the "**Full Changelog**: ..." compare-link footer line) to
    // land on the first real change description.
    let summary = parsed
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("**Full Changelog**"))
        .unwrap_or("")
        .trim_start_matches(['-', '*', ' '])
        .trim_start_matches("**")
        .chars()
        .take(120)
        .collect::<String>();
    UpdateCache {
        checked_at: now_secs(),
        latest_tag: tag,
        changelog_summary: summary,
    }
}

/// Compares two `vX.Y.Z` (or `X.Y.Z`) strings. Returns `false` on any parse
/// failure, matching this module's fail-silent contract.
fn is_newer(candidate: &str, current: &str) -> bool {
    let (Some(c), Some(cur)) = (parse_triplet(candidate), parse_triplet(current)) else {
        return false;
    };
    c > cur
}

fn parse_triplet(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches('v');
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_detects_a_higher_version() {
        assert!(is_newer("v3.9.0", "3.8.0"));
        assert!(is_newer("v4.0.0", "3.8.0"));
        assert!(is_newer("v3.8.1", "3.8.0"));
    }

    #[test]
    fn is_newer_is_false_for_equal_or_older() {
        assert!(!is_newer("v3.8.0", "3.8.0"));
        assert!(!is_newer("v3.7.9", "3.8.0"));
    }

    #[test]
    fn is_newer_is_false_on_unparseable_input() {
        assert!(!is_newer("v3.8", "3.8.0"));
        assert!(!is_newer("not-a-version", "3.8.0"));
    }

    #[test]
    fn parse_triplet_strips_leading_v_and_parses_each_part() {
        assert_eq!(parse_triplet("v3.8.0"), Some((3, 8, 0)));
        assert_eq!(parse_triplet("10.2.33"), Some((10, 2, 33)));
    }

    #[test]
    fn parse_triplet_returns_none_for_malformed_input() {
        assert_eq!(parse_triplet("3.8"), None);
        assert_eq!(parse_triplet("abc"), None);
        assert_eq!(parse_triplet(""), None);
    }

    #[test]
    fn parse_release_response_extracts_tag_and_first_changelog_line() {
        let body =
            br#"{"tag_name": "v3.9.0", "body": "\n- **Feature:** does a thing\n- more stuff"}"#;
        let cache = parse_release_response(body);
        assert_eq!(cache.latest_tag.as_deref(), Some("v3.9.0"));
        assert_eq!(cache.changelog_summary, "Feature:** does a thing");
    }

    #[test]
    fn parse_release_response_skips_the_whats_changed_header_from_real_github_output() {
        // Real shape of this repo's auto-generated release body (confirmed
        // live against api.github.com/repos/alazndy/R-AI-OS/releases/latest
        // before writing this fix) — the first line is a markdown header,
        // not a change description, and the last is a compare-link footer.
        let body = br###"{"tag_name": "v3.9.0", "body": "## What's Changed\n* feat: add thing by @alazndy in https://github.com/alazndy/R-AI-OS/pull/13\n\n**Full Changelog**: https://github.com/alazndy/R-AI-OS/compare/v3.8.0...v3.9.0"}"###;
        let cache = parse_release_response(body);
        assert_eq!(cache.latest_tag.as_deref(), Some("v3.9.0"));
        assert_eq!(
            cache.changelog_summary,
            "feat: add thing by @alazndy in https://github.com/alazndy/R-AI-OS/pull/13"
        );
    }

    #[test]
    fn parse_release_response_handles_missing_fields_without_panicking() {
        let cache = parse_release_response(b"{}");
        assert_eq!(cache.latest_tag, None);
        assert_eq!(cache.changelog_summary, "");
    }

    #[test]
    fn parse_release_response_handles_garbage_input_without_panicking() {
        let cache = parse_release_response(b"not json at all");
        assert_eq!(cache.latest_tag, None);
    }

    #[test]
    fn load_or_refresh_cache_reuses_a_fresh_cache_without_touching_the_network() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("update-check-cache.json");
        let fresh = UpdateCache {
            checked_at: now_secs(),
            latest_tag: Some("v9.9.9".to_string()),
            changelog_summary: "test entry".to_string(),
        };
        std::fs::write(&path, serde_json::to_string(&fresh).unwrap()).unwrap();

        let loaded = load_or_refresh_cache(&path).unwrap();
        assert_eq!(loaded.latest_tag.as_deref(), Some("v9.9.9"));
        assert_eq!(loaded.changelog_summary, "test entry");
    }

    // A stale or missing cache file falls through to refresh_cache(), which
    // shells out to `curl` against the real GitHub API — deliberately not
    // mocked here (this codebase shells out to curl for one-off HTTP calls
    // throughout rather than carrying an injectable HTTP client trait; see
    // bootstrap.rs's sync_rules for the established precedent). That path is
    // verified live instead: `curl -fsSL --max-time 3
    // https://api.github.com/repos/alazndy/R-AI-OS/releases/latest | cargo
    // run --bin <throwaway> -- parse` against parse_release_response, run
    // manually before shipping this module.
}
