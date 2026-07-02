//! Heuristic scan for obvious secrets (API keys, tokens, private keys) in
//! free-text agent messages. Not exhaustive — a deterrent against accidental
//! paste, not a DLP system. Shared by `raios handoff` (`cli/handoff.rs`) and
//! the A2A `message/send` endpoint (`server/http/a2a.rs`) so both delivery
//! paths for a handoff message get the same protection.

use std::sync::LazyLock;

/// Compiled once on first use (not once per `looks_like_secret` call — the
/// previous implementation re-compiled all 7 patterns on every invocation,
/// including on the A2A `message/send` hot path).
static SECRET_PATTERNS: LazyLock<Vec<(regex::Regex, &'static str)>> = LazyLock::new(|| {
    let patterns: &[(&str, &str)] = &[
        (r"AKIA[0-9A-Z]{16}", "AWS access key"),
        (r"sk-ant-[A-Za-z0-9_-]{20,}", "Anthropic API key"),
        (r"sk-[A-Za-z0-9]{20,}", "OpenAI-style API key"),
        (r"gh[pousr]_[A-Za-z0-9]{36,}", "GitHub token"),
        (r"github_pat_[A-Za-z0-9_]{20,}", "GitHub fine-grained PAT"),
        (
            r"-----BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY-----",
            "private key block",
        ),
        (
            r"(?i)(api[_-]?key|secret|password|token)\s*[=:]\s*['\x22]?[A-Za-z0-9_\-/+]{12,}",
            "key/secret/password/token assignment",
        ),
    ];
    patterns
        .iter()
        .map(|(pattern, label)| {
            (
                regex::Regex::new(pattern).expect("secret_scan pattern must be valid regex"),
                *label,
            )
        })
        .collect()
});

/// Returns a human-readable label for the first secret-like pattern found in
/// `text`, or `None` if nothing matched.
pub fn looks_like_secret(text: &str) -> Option<&'static str> {
    SECRET_PATTERNS
        .iter()
        .find(|(re, _)| re.is_match(text))
        .map(|(_, label)| *label)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_anthropic_key() {
        assert_eq!(
            looks_like_secret("here's my key sk-ant-abcdefghijklmnopqrstuvwxyz"),
            Some("Anthropic API key")
        );
    }

    #[test]
    fn detects_aws_key() {
        assert_eq!(looks_like_secret("AKIAABCDEFGHIJKLMNOP"), Some("AWS access key"));
    }

    #[test]
    fn detects_private_key_block() {
        assert_eq!(
            looks_like_secret("-----BEGIN RSA PRIVATE KEY-----\nMIIEow...\n-----END..."),
            Some("private key block")
        );
    }

    #[test]
    fn detects_generic_assignment() {
        assert_eq!(
            looks_like_secret("password = 'sup3rSecretValue123'"),
            Some("key/secret/password/token assignment")
        );
    }

    #[test]
    fn clean_text_passes() {
        assert_eq!(looks_like_secret("skeleton ready, implement auth handlers"), None);
    }
}
