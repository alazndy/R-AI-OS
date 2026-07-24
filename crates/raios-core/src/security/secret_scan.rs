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
        (r"(?i)bearer\s+[A-Za-z0-9._-]{20,}", "bearer token"),
        (
            r"eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}",
            "JSON web token",
        ),
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

/// Replaces every recognized credential-shaped value before it can be written
/// to an ANKA transcript cache. This is intentionally conservative: it is a
/// safety net, not a claim of complete secret detection.
pub fn redact_secrets(text: &str) -> String {
    SECRET_PATTERNS
        .iter()
        .fold(text.to_string(), |redacted, (re, label)| {
            re.replace_all(&redacted, format!("[REDACTED:{label}]").as_str())
                .into_owned()
        })
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
        assert_eq!(
            looks_like_secret("AKIAABCDEFGHIJKLMNOP"),
            Some("AWS access key")
        );
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
        assert_eq!(
            looks_like_secret("skeleton ready, implement auth handlers"),
            None
        );
    }

    #[test]
    fn redacts_recognized_secrets_without_dropping_context() {
        let text = "deploy with token=abcdefghijklmno and keep the release notes";
        let redacted = redact_secrets(text);
        assert!(!redacted.contains("abcdefghijklmno"));
        assert!(redacted.contains("[REDACTED:key/secret/password/token assignment]"));
        assert!(redacted.contains("keep the release notes"));
    }

    #[test]
    fn redacts_bearer_and_json_web_tokens() {
        let text = "Bearer abcdefghijklmnopqrstuvwxyz.abcdefghijklmno.abcdefghijklmnop";
        assert!(redact_secrets(text).contains("REDACTED"));
    }
}
