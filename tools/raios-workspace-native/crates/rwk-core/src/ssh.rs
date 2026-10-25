//! Quick SSH connections: the hosts the user already has (`~/.ssh/config`, `known_hosts`) and
//! the one validated shape a typed target must take before it becomes an argument of `ssh`.
//!
//! A target is never command text. It is checked here, then handed to `ssh` as separate
//! arguments after `--`, so a host that looks like an option (`-oProxyCommand=...`) cannot be
//! smuggled in.

use std::path::Path;

use crate::error::{AppError, ErrorCode, Result};

/// Longest target accepted, in bytes.
const MAX_TARGET_BYTES: usize = 255;
/// Most hosts listed.
pub const MAX_HOSTS: usize = 300;
/// Largest file read, in bytes. Anything past it is ignored.
const MAX_FILE_BYTES: u64 = 1 << 20;

/// A validated place to connect to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    /// `host` or `user@host`; an alias from the user's ssh config is a host here.
    destination: String,
    port: Option<u16>,
}

impl SshTarget {
    /// Parses `host`, `user@host`, `host:port`, `user@host:port` or `[ipv6]:port`.
    pub fn parse(raw: &str) -> Result<Self> {
        let invalid = |why: &str| {
            AppError::new(
                ErrorCode::InvalidInput,
                format!("Geçersiz SSH hedefi: {why}"),
            )
        };
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(invalid("boş"));
        }
        if raw.len() > MAX_TARGET_BYTES {
            return Err(invalid("çok uzun"));
        }
        if raw.starts_with('-') {
            return Err(invalid("'-' ile başlayamaz"));
        }
        if raw.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(invalid("boşluk ve kontrol karakteri içeremez"));
        }
        let (user, rest) = match raw.split_once('@') {
            Some((user, rest)) => (Some(user), rest),
            None => (None, raw),
        };
        if let Some(user) = user {
            if user.is_empty() || !user.chars().all(is_name_char) || user.starts_with('-') {
                return Err(invalid("kullanıcı adı"));
            }
        }
        let (host, port) = split_host_port(rest).ok_or_else(|| invalid("sunucu veya port"))?;
        if host.is_empty() || host.starts_with('-') {
            return Err(invalid("sunucu adı"));
        }
        let destination = match user {
            Some(user) => format!("{user}@{host}"),
            None => host,
        };
        Ok(Self { destination, port })
    }

    /// The text shown for the target: what the user would type.
    pub fn display(&self) -> String {
        match self.port {
            Some(port) => format!("{}:{port}", self.destination),
            None => self.destination.clone(),
        }
    }

    /// The arguments for `ssh`: an optional port, then `--`, then the destination.
    pub fn args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(port) = self.port {
            args.push("-p".to_owned());
            args.push(port.to_string());
        }
        args.push("--".to_owned());
        args.push(self.destination.clone());
        args
    }
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')
}

/// Splits `host[:port]`; IPv6 must be in brackets when a port follows. Returns the bare host
/// (IPv6 without brackets, which is how `ssh` takes it).
fn split_host_port(text: &str) -> Option<(String, Option<u16>)> {
    if let Some(inner) = text.strip_prefix('[') {
        let (address, after) = inner.split_once(']')?;
        let valid = !address.is_empty()
            && address.contains(':')
            && address.chars().all(|c| c.is_ascii_hexdigit() || c == ':');
        if !valid {
            return None;
        }
        let port = match after {
            "" => None,
            after => Some(parse_port(after.strip_prefix(':')?)?),
        };
        return Some((address.to_owned(), port));
    }
    match text.matches(':').count() {
        0 => text
            .chars()
            .all(is_name_char)
            .then(|| (text.to_owned(), None)),
        1 => {
            let (host, port) = text.split_once(':')?;
            if !host.chars().all(is_name_char) {
                return None;
            }
            Some((host.to_owned(), Some(parse_port(port)?)))
        }
        // A bare IPv6 address, no port.
        _ => (text.chars().all(|c| c.is_ascii_hexdigit() || c == ':'))
            .then(|| (text.to_owned(), None)),
    }
}

fn parse_port(text: &str) -> Option<u16> {
    let port: u16 = text.parse().ok()?;
    (port != 0).then_some(port)
}

/// One host the user already knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshHost {
    /// What to connect to: an alias of the ssh config or a host name.
    pub alias: String,
    /// Where it leads, when the config says (`user@host:port`).
    pub detail: Option<String>,
}

impl SshHost {
    /// The target for this host, `None` when its name is not a valid target.
    pub fn target(&self) -> Option<SshTarget> {
        SshTarget::parse(&self.alias).ok()
    }
}

/// Reads the `Host` blocks of an ssh config. Wildcard and negated patterns are skipped, every
/// other pattern of a `Host` line is a host, and `HostName`, `User` and `Port` of the block
/// become its detail. `Match` blocks and `Include` lines are not followed.
pub fn parse_ssh_config(text: &str) -> Vec<SshHost> {
    struct Block {
        aliases: Vec<String>,
        hostname: Option<String>,
        user: Option<String>,
        port: Option<String>,
    }
    let mut blocks: Vec<Block> = Vec::new();
    let mut current: Option<Block> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (keyword, value) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((keyword, value)) => (keyword, value.trim_start_matches(['=', ' ', '\t']).trim()),
            None => continue,
        };
        match keyword.to_ascii_lowercase().as_str() {
            "host" => {
                blocks.extend(current.take());
                let aliases = value
                    .split_whitespace()
                    .filter(|pattern| !pattern.contains(['*', '?', '!']))
                    .map(str::to_owned)
                    .collect();
                current = Some(Block {
                    aliases,
                    hostname: None,
                    user: None,
                    port: None,
                });
            }
            "match" => blocks.extend(current.take()),
            "hostname" => {
                if let Some(block) = current.as_mut() {
                    block.hostname.get_or_insert_with(|| value.to_owned());
                }
            }
            "user" => {
                if let Some(block) = current.as_mut() {
                    block.user.get_or_insert_with(|| value.to_owned());
                }
            }
            "port" => {
                if let Some(block) = current.as_mut() {
                    block.port.get_or_insert_with(|| value.to_owned());
                }
            }
            _ => {}
        }
    }
    blocks.extend(current);
    blocks
        .into_iter()
        .flat_map(|block| {
            let detail = block.hostname.map(|host| {
                let mut text = String::new();
                if let Some(user) = &block.user {
                    text.push_str(user);
                    text.push('@');
                }
                text.push_str(&host);
                if let Some(port) = &block.port {
                    text.push(':');
                    text.push_str(port);
                }
                text
            });
            block.aliases.into_iter().map(move |alias| SshHost {
                alias,
                detail: detail.clone(),
            })
        })
        .collect()
}

/// Reads the plain host names of a `known_hosts` file. Hashed entries cannot be listed and
/// markers (`@cert-authority`, `@revoked`) and patterns are skipped; `[host]:port` becomes
/// `host:port`.
pub fn parse_known_hosts(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('@') {
                return None;
            }
            line.split_whitespace().next()
        })
        .flat_map(|field| field.split(','))
        .filter(|host| !host.starts_with('|') && !host.contains(['*', '?', '!']))
        .map(
            |host| match host.strip_prefix('[').and_then(|h| h.split_once("]:")) {
                Some((name, port)) => format!("{name}:{port}"),
                None => host.to_owned(),
            },
        )
        .collect()
}

fn read_limited(path: &Path) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut text = String::new();
    file.take(MAX_FILE_BYTES).read_to_string(&mut text).ok()?;
    Some(text)
}

/// The hosts of `~/.ssh` under `home`: config aliases first, then hosts only `known_hosts`
/// knows. Duplicates are dropped, entries that are not valid targets are left out, and at
/// most [`MAX_HOSTS`] are returned.
pub fn discover(home: &Path) -> Vec<SshHost> {
    let ssh = home.join(".ssh");
    let mut hosts = read_limited(&ssh.join("config"))
        .map(|text| parse_ssh_config(&text))
        .unwrap_or_default();
    let known = read_limited(&ssh.join("known_hosts"))
        .map(|text| parse_known_hosts(&text))
        .unwrap_or_default();
    hosts.extend(known.into_iter().map(|alias| SshHost {
        alias,
        detail: None,
    }));
    let mut seen = std::collections::HashSet::new();
    hosts.retain(|host| host.target().is_some() && seen.insert(host.alias.clone()));
    hosts.truncate(MAX_HOSTS);
    hosts
}

/// The hosts matching `query`: every word of the query must appear (case-insensitively) in the
/// alias or the detail. An empty query matches all. An exact alias comes first.
pub fn filter_hosts<'a>(hosts: &'a [SshHost], query: &str) -> Vec<&'a SshHost> {
    let words: Vec<String> = query
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    let mut matched: Vec<&SshHost> = hosts
        .iter()
        .filter(|host| {
            let haystack = format!(
                "{} {}",
                host.alias.to_ascii_lowercase(),
                host.detail.as_deref().unwrap_or("").to_ascii_lowercase()
            );
            words.iter().all(|word| haystack.contains(word.as_str()))
        })
        .collect();
    let exact = query.trim();
    matched.sort_by_key(|host| host.alias != exact);
    matched
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(raw: &str) -> Result<SshTarget> {
        SshTarget::parse(raw)
    }

    #[test]
    fn plain_forms_parse_and_become_separate_arguments_after_double_dash() {
        let parsed = target("deploy@build.example.com:2222").unwrap();
        assert_eq!(parsed.display(), "deploy@build.example.com:2222");
        assert_eq!(
            parsed.args(),
            ["-p", "2222", "--", "deploy@build.example.com"]
        );
        assert_eq!(target("prod").unwrap().args(), ["--", "prod"]);
        assert_eq!(target("  10.0.0.7  ").unwrap().display(), "10.0.0.7");
        assert_eq!(
            target("[fe80::1]:22").unwrap().args(),
            ["-p", "22", "--", "fe80::1"]
        );
        assert_eq!(target("fe80::1").unwrap().args(), ["--", "fe80::1"]);
    }

    #[test]
    fn option_lookalikes_whitespace_controls_and_bad_ports_are_refused() {
        for bad in [
            "",
            "   ",
            "-oProxyCommand=evil",
            "-p",
            "host name",
            "host\tname",
            "host\nname",
            "a@@b",
            "@host",
            "-x@host",
            "user@-host",
            "host:0",
            "host:65536",
            "host:abc",
            "host:22:33x",
            "[::1",
            "[host]:22",
            "host;rm -rf",
            "$(id)",
            "host`id`",
            "ho\u{0}st",
        ] {
            assert!(target(bad).is_err(), "{bad:?} must be refused");
        }
        assert!(target(&"a".repeat(256)).is_err());
    }

    #[test]
    fn config_aliases_carry_their_details_and_skip_patterns() {
        let config = "\
# comment
Host *
    ServerAliveInterval 30

Host prod prod-db !bad *.internal
    HostName 10.1.2.3
    User deploy
    Port 2200

Host=lab
    hostname lab.example.com

Match host foo
    User ignored

Host after
";
        let hosts = parse_ssh_config(config);
        let names: Vec<&str> = hosts.iter().map(|h| h.alias.as_str()).collect();
        assert_eq!(names, ["prod", "prod-db", "lab", "after"]);
        assert_eq!(hosts[0].detail.as_deref(), Some("deploy@10.1.2.3:2200"));
        assert_eq!(hosts[2].detail.as_deref(), Some("lab.example.com"));
        assert_eq!(hosts[3].detail, None);
    }

    #[test]
    fn known_hosts_lists_plain_names_only() {
        let known = "\
|1|hashed= ssh-ed25519 AAAA
@cert-authority *.corp ssh-rsa AAAA
github.com,140.82.1.1 ssh-ed25519 AAAA
[example.org]:2222 ssh-rsa AAAA
*.wild ssh-rsa AAAA
# comment
";
        assert_eq!(
            parse_known_hosts(known),
            ["github.com", "140.82.1.1", "example.org:2222"]
        );
    }

    #[test]
    fn discovery_merges_dedupes_and_drops_invalid_entries() {
        let home = tempfile::tempdir().unwrap();
        let ssh = home.path().join(".ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::write(
            ssh.join("config"),
            "Host prod\n  HostName 10.0.0.1\nHost -evil\n",
        )
        .unwrap();
        std::fs::write(
            ssh.join("known_hosts"),
            "prod ssh-rsa A\nother.example ssh-rsa A\n",
        )
        .unwrap();

        let hosts = discover(home.path());
        let names: Vec<&str> = hosts.iter().map(|h| h.alias.as_str()).collect();
        assert_eq!(names, ["prod", "other.example"]);
        assert_eq!(hosts[0].detail.as_deref(), Some("10.0.0.1"));
        // No ~/.ssh at all is simply no hosts.
        assert!(discover(&home.path().join("nowhere")).is_empty());
    }

    #[test]
    fn filtering_needs_every_word_and_puts_an_exact_alias_first() {
        let hosts = vec![
            SshHost {
                alias: "prod-db".into(),
                detail: Some("deploy@10.1.2.3".into()),
            },
            SshHost {
                alias: "prod".into(),
                detail: None,
            },
            SshHost {
                alias: "lab".into(),
                detail: Some("lab.example.com".into()),
            },
        ];
        let names = |query: &str| -> Vec<String> {
            filter_hosts(&hosts, query)
                .iter()
                .map(|h| h.alias.clone())
                .collect()
        };
        assert_eq!(names(""), ["prod-db", "prod", "lab"]);
        assert_eq!(names("prod"), ["prod", "prod-db"]);
        assert_eq!(names("DEPLOY db"), ["prod-db"]);
        assert_eq!(names("example"), ["lab"]);
        assert!(names("nothing").is_empty());
    }
}
