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
    ///  1. Cloud metadata / link-local target → always block, even if the
    ///     filter itself is disabled or the target is on an allowlist. This
    ///     is a fixed safety floor (SSRF metadata-endpoint protection), not
    ///     a policy-toggle — see `is_blocked_metadata_target`. Deliberately
    ///     does *not* cover `127.0.0.1`/`localhost`: those are this
    ///     codebase's own legitimate target (the local `aiosd` daemon,
    ///     `capabilities::default_for`'s `network` capability), not an SSRF
    ///     target.
    ///  2. Not enabled → allow everything else
    ///  3. deny_all → block everything
    ///  4. blocked_domains match → block
    ///  5. allowed_domains match → allow
    ///  6. default → block (fail-closed)
    pub fn check(&self, url: &str) -> Result<()> {
        let domain = extract_domain(url);

        if is_blocked_metadata_target(&domain) {
            return Err(anyhow!(
                "Egress Denied: '{}' is a cloud metadata / link-local target — always blocked",
                domain
            ));
        }

        if !self.enabled {
            return Ok(());
        }

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

/// Extracts the host from a URL using an RFC 3986-compliant parser (the
/// `url` crate) instead of ad-hoc string splitting. Correctly handles
/// userinfo (`user@host`), IPv6 literals (`[::1]`), ports, and mixed case —
/// all of which the previous naive `split('/')`/`split(':')` implementation
/// mishandled (e.g. it truncated any IPv6 literal at its first `:`, and
/// never stripped a `user@` prefix that could otherwise visually mislead
/// domain-matching output).
///
/// Also accepts a bare `domain.com` (no scheme) by retrying with a `http://`
/// prefix, since capability declarations in this codebase pass bare hosts
/// (e.g. `"127.0.0.1"`, `"localhost"`) rather than full URLs.
fn extract_domain(url: &str) -> String {
    let parsed = url::Url::parse(url)
        .or_else(|_| url::Url::parse(&format!("http://{url}")))
        .ok();

    match parsed.and_then(|u| u.host_str().map(str::to_string)) {
        // `host_str()` keeps the `[...]` bracket wrapper the URL spec
        // requires for IPv6 literals in the authority component; strip it
        // so callers/config entries can write the bare address ("::1") the
        // same way they'd write "127.0.0.1" or "api.github.com".
        Some(host) => host
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .unwrap_or(&host)
            .to_lowercase(),
        // Parsing failed entirely (e.g. empty string) — fall back to the
        // raw input so callers still get a deterministic, if unmatched,
        // value rather than a panic.
        None => url.to_lowercase(),
    }
}

/// Cloud instance-metadata and link-local targets that are never a
/// legitimate destination for any tool in this codebase — a fixed safety
/// floor independent of `raios-policy.toml`. Covers the well-known
/// AWS/GCP/Azure/DigitalOcean/Oracle metadata IP (`169.254.169.254`), its
/// AWS IPv6 equivalent, and the wider IPv4 link-local block (`169.254.0.0/16`)
/// that metadata services live in across cloud providers. Deliberately
/// excludes `127.0.0.1`/`localhost`/`::1` — this codebase's own local daemon
/// legitimately listens there (see `check`'s doc comment).
fn is_blocked_metadata_target(host: &str) -> bool {
    const BLOCKED_HOSTS: &[&str] = &[
        "169.254.169.254",
        "fd00:ec2::254",
        "metadata.google.internal",
        "metadata.internal",
        "metadata.azure.com",
    ];
    if BLOCKED_HOSTS.contains(&host) {
        return true;
    }
    if let Ok(std::net::IpAddr::V4(v4)) = host.parse() {
        let [a, b, ..] = v4.octets();
        if a == 169 && b == 254 {
            return true;
        }
    }
    false
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

    // ─── Metadata / SSRF hardening regression tests ──────────────────────

    #[test]
    fn metadata_ip_blocked_even_when_filter_disabled() {
        // The whole point of a fixed safety floor: it must not depend on the
        // operator ever configuring/enabling [egress] in raios-policy.toml.
        let f = EgressFilter::disabled();
        assert!(!f.is_allowed("http://169.254.169.254/latest/meta-data/"));
    }

    #[test]
    fn metadata_ip_blocked_even_when_explicitly_allowlisted() {
        // A fixed floor, not a policy toggle: an operator mistake (or a
        // compromised policy file) allowlisting the metadata IP must not
        // override this.
        let f = filter_with(&["169.254.169.254"], &[]);
        assert!(!f.is_allowed("http://169.254.169.254/"));
    }

    #[test]
    fn link_local_range_blocked_beyond_exact_metadata_ip() {
        // Metadata services on some cloud platforms use other addresses in
        // the same 169.254.0.0/16 block, not just .169.254.
        let f = EgressFilter::disabled();
        assert!(!f.is_allowed("http://169.254.1.1/"));
    }

    #[test]
    fn loopback_and_localhost_remain_unaffected_by_metadata_block() {
        // 127.0.0.1/localhost is this codebase's own legitimate daemon
        // target (capabilities::default_for's network capability) — the
        // metadata floor must never catch it.
        let f = EgressFilter::disabled();
        assert!(f.is_allowed("http://127.0.0.1:42069/"));
        assert!(f.is_allowed("http://localhost:42069/"));
    }

    #[test]
    fn ipv6_literal_is_correctly_parsed_not_truncated_at_first_colon() {
        // Regression: the old `domain.split(':').next()` truncated any IPv6
        // literal at its first colon, producing a garbage/unmatchable host.
        let f = filter_with(&["::1"], &[]);
        assert!(f.is_allowed("http://[::1]:8080/status"));
    }

    #[test]
    fn userinfo_prefix_does_not_leak_into_extracted_host() {
        // Regression: the old naive parser never stripped a `user@` prefix,
        // so the "domain" fed into matching could include unrelated text.
        let f = filter_with(&["example.com"], &[]);
        assert!(f.is_allowed("http://attacker@example.com/"));
    }
}
