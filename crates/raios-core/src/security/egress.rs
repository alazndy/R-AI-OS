use anyhow::{anyhow, Result};

// ─── Config ────────────────────────────────────────────────────────────────────

/// Egress filter — loaded from `raios-policy.toml` [egress] section.
#[derive(Debug, Clone)]
pub struct EgressFilter {
    pub enabled: bool,
    /// If true, ALL outbound calls are blocked regardless of allowed_domains.
    pub deny_all: bool,
    /// Allowlist of exact or suffix-matched domains (e.g. "api.github.com", ".crates.io").
    pub allowed_domains: Vec<String>,
    /// Explicit blocklist checked before the allowlist (takes priority).
    pub blocked_domains: Vec<String>,
}

impl EgressFilter {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            deny_all: false,
            allowed_domains: vec![],
            blocked_domains: vec![],
        }
    }

    /// Build from the [egress] section of `raios-policy.toml`.
    pub fn from_policy(policy: &raios_core::security::PolicyConfig) -> Self {
        if let Some(egress) = &policy.egress {
            Self {
                enabled: egress.enabled,
                deny_all: egress.deny_all.unwrap_or(false),
                allowed_domains: egress.allowed_domains.clone(),
                blocked_domains: egress.blocked_domains.clone(),
            }
        } else {
            Self::disabled()
        }
    }

    /// Returns `Ok(())` if the domain is allowed by the egress policy.
    ///
    /// Decision order:
    ///  1. Not enabled → allow everything
    ///  2. deny_all → block everything
    ///  3. blocked_domains match → block
    ///  4. allowed_domains match → allow
    ///  5. default → block (fail-closed)
    pub fn check(&self, url: &str) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let domain = extract_domain(url);

        if self.deny_all {
            return Err(anyhow!(
                "Egress Denied: all outbound network calls are blocked by policy"
            ));
        }

        // Explicit blocklist (highest priority)
        for blocked in &self.blocked_domains {
            if domain_matches(&domain, blocked) {
                return Err(anyhow!(
                    "Egress Denied: domain '{}' is explicitly blocked",
                    domain
                ));
            }
        }

        // Allowlist
        for allowed in &self.allowed_domains {
            if domain_matches(&domain, allowed) {
                return Ok(());
            }
        }

        // Fail-closed: not in allowlist → deny
        Err(anyhow!(
            "Egress Denied: domain '{}' is not in the allowed_domains list",
            domain
        ))
    }

    pub fn is_allowed(&self, url: &str) -> bool {
        self.check(url).is_ok()
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Very lightweight domain extractor (no external HTTP deps).
/// Handles `https://api.github.com/foo`, `http://...`, or bare `domain.com`.
fn extract_domain(url: &str) -> String {
    let stripped = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let domain = stripped.split('/').next().unwrap_or(stripped);
    // Drop port if present
    domain.split(':').next().unwrap_or(domain).to_lowercase()
}

/// Suffix/exact matching: allowed entry ".github.com" matches "api.github.com".
fn domain_matches(domain: &str, pattern: &str) -> bool {
    let p = pattern.trim().to_lowercase();
    if p.starts_with('.') {
        domain == &p[1..] || domain.ends_with(&p)
    } else {
        domain == p.as_str()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn filter_with(allowed: &[&str], blocked: &[&str]) -> EgressFilter {
        EgressFilter {
            enabled: true,
            deny_all: false,
            allowed_domains: allowed.iter().map(|s| s.to_string()).collect(),
            blocked_domains: blocked.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn disabled_filter_allows_everything() {
        let f = EgressFilter::disabled();
        assert!(f.is_allowed("https://evil.com/exfiltrate"));
    }

    #[test]
    fn deny_all_blocks_everything() {
        let f = EgressFilter {
            enabled: true,
            deny_all: true,
            ..EgressFilter::disabled()
        };
        assert!(!f.is_allowed("https://api.github.com"));
    }

    #[test]
    fn exact_domain_allowed() {
        let f = filter_with(&["api.github.com"], &[]);
        assert!(f.is_allowed("https://api.github.com/repos/foo/bar"));
    }

    #[test]
    fn suffix_wildcard_match() {
        let f = filter_with(&[".crates.io"], &[]);
        assert!(f.is_allowed("https://static.crates.io/crates/serde.crate"));
        assert!(f.is_allowed("https://crates.io/"));
    }

    #[test]
    fn unknown_domain_is_blocked() {
        let f = filter_with(&["api.github.com"], &[]);
        assert!(!f.is_allowed("https://evil.com/data"));
    }

    #[test]
    fn blocked_domain_takes_priority_over_allowed() {
        let f = filter_with(&[".github.com"], &["evil.github.com"]);
        assert!(!f.is_allowed("https://evil.github.com/leak"));
        assert!(f.is_allowed("https://api.github.com/ok"));
    }

    #[test]
    fn port_is_stripped_for_matching() {
        let f = filter_with(&["api.github.com"], &[]);
        assert!(f.is_allowed("https://api.github.com:443/foo"));
    }
}
