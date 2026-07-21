//! Capability-declaration sandbox for built-in MCP tools ("no ambient
//! authority"): a tool only gets the filesystem/network access it declares,
//! whether that declaration comes from `raios-policy.toml`
//! (`ToolRule.capabilities`) or this module's built-in default map.
//!
//! Scope, honestly stated: enforcement here covers the two chokepoints that
//! actually exist in the codebase today — `SandboxGuard` for filesystem
//! boundaries and `EgressFilter` for outbound domains. `exec` is declarative
//! only (surfaced via `raios policy caps`) because there is no chokepoint
//! that intercepts arbitrary `std::process::Command` spawns without a much
//! larger refactor of every build/deps/git module. Filesystem enforcement
//! only fires for the 12 tools that resolve a real path via
//! `McpServer::resolve_git_path` (see `PATH_RESOLVING_TOOLS`) — tools that
//! take a `project` string purely as a name *filter* (`get_health`,
//! `list_projects`, `portfolio_status`) never touch that path directly and
//! are intentionally left alone.

use super::egress::EgressFilter;
use super::policy::ToolCapabilities;
use super::sandbox::SandboxGuard;

/// Tool names whose implementation resolves `args["project"]` to a real
/// filesystem path via `McpServer::resolve_git_path` before touching disk.
/// Kept in sync with the `resolve_git_path(args)` call sites in
/// `src/mcp/tools_dev.rs`, `tools_git.rs`, and `tools_workspace.rs`.
pub const PATH_RESOLVING_TOOLS: &[&str] = &[
    "git_status",
    "git_log",
    "git_diff",
    "git_commit",
    "disk_usage",
    "version_info",
    "version_bump",
    "env_status",
    "deps_status",
    "run_build",
    "run_tests",
    "project_info",
];

/// Every built-in MCP tool name, for `raios policy caps` to enumerate.
///
/// Intentionally a standalone list rather than importing from
/// `McpServer::static_tools_manifest` (`src/mcp/mod.rs`): that manifest's
/// exact serialized bytes are SHA-256 pinned for tamper detection, so it must
/// stay self-contained rather than depend on code outside the `mcp` module.
/// Keep this in sync by hand if a tool is added or removed there.
pub const ALL_TOOLS: &[&str] = &[
    "update_state",
    "handover",
    "add_task",
    "get_health",
    "get_inbox",
    "list_projects",
    "get_stats",
    "semantic_search",
    "anka_recall",
    "project_info",
    "portfolio_status",
    "disk_usage",
    "list_ports",
    "usage_status",
    "version_info",
    "version_bump",
    "env_status",
    "deps_status",
    "run_build",
    "run_tests",
    "git_status",
    "git_log",
    "git_diff",
    "git_commit",
    "ask_architect",
    "get_validation_errors",
    "session_note",
    "create_swarm_task",
    "list_swarm_tasks",
    "approve_swarm_task",
    "route_capability",
    "list_evolution_candidates",
    "promote_evolution_candidate",
    "get_agent_stats",
];

/// Built-in capability defaults for R-AI-OS's own MCP tools. A TOML
/// `[[tools.rules]]` `capabilities` block always takes precedence — see
/// `resolve`.
pub fn default_for(tool: &str) -> ToolCapabilities {
    match tool {
        // Read-only inspection of a caller-supplied project path.
        "git_status" | "git_log" | "git_diff" | "project_info" | "disk_usage" | "version_info"
        | "env_status" | "deps_status" => ToolCapabilities {
            fs_read: vec!["*".into()],
            ..Default::default()
        },
        // Writes within the resolved project path (commit, changelog, build
        // artifacts) and shells out to git/cargo/npm/etc.
        "git_commit" | "version_bump" => ToolCapabilities {
            fs_read: vec!["*".into()],
            fs_write: vec!["*".into()],
            exec: true,
            ..Default::default()
        },
        "run_build" | "run_tests" => ToolCapabilities {
            fs_read: vec!["*".into()],
            fs_write: vec!["*".into()],
            exec: true,
            ..Default::default()
        },
        // Talk to the local aiosd daemon over TCP (127.0.0.1:42069).
        "semantic_search"
        | "get_inbox"
        | "route_capability"
        | "list_evolution_candidates"
        | "promote_evolution_candidate" => ToolCapabilities {
            network: vec!["127.0.0.1".into(), "localhost".into()],
            ..Default::default()
        },
        // ANKA reads its fixed owner-only local cache; it accepts no caller-supplied filesystem path.
        "anka_recall" => ToolCapabilities {
            fs_read: vec!["<anka-cache>".into()],
            ..Default::default()
        },
        // Everything else (get_stats, list_ports, usage_status, list_projects,
        // get_health, portfolio_status, session_note, update_state, handover,
        // add_task, ask_architect, get_validation_errors, swarm tools, …)
        // needs neither declared filesystem nor network access to do its job.
        _ => ToolCapabilities::default(),
    }
}

/// Resolves the effective capability for a tool call: an explicit
/// `raios-policy.toml` override wins, otherwise the built-in default.
pub fn resolve(tool: &str, override_caps: Option<ToolCapabilities>) -> ToolCapabilities {
    override_caps.unwrap_or_else(|| default_for(tool))
}

/// Enforces the filesystem half of a tool's capability against the path it
/// actually resolved to touch. A tool with no declared `fs_read`/`fs_write`
/// capability is denied outright — it has no ambient authority to touch any
/// path, regardless of what the caller passed in.
///
/// `workspace_root` is the real jail boundary (e.g. `Config::dev_ops_path`)
/// — every path-resolving tool call must stay inside it. It must be a
/// distinct, caller-supplied root, not `resolved_target` itself: passing the
/// target as its own workspace makes the boundary check trivially true for
/// any path, which previously left the filesystem jail as a no-op for this
/// call path (only `blocked_paths` was ever actually enforced).
pub fn check_fs_capability(
    caps: &ToolCapabilities,
    workspace_root: &std::path::Path,
    resolved_target: &std::path::Path,
    blocked_paths: &[String],
) -> Result<(), String> {
    if caps.fs_read.is_empty() && caps.fs_write.is_empty() {
        return Err("tool has no declared filesystem capability".to_string());
    }
    SandboxGuard::new(workspace_root.to_path_buf())
        .with_blocked_paths(blocked_paths.to_vec())
        .check(resolved_target)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Enforces the network half of a tool's capability. A tool that declares no
/// network capability is never checked here (nothing to enforce — it has no
/// chokepoint-visible network access to begin with). A tool that *does*
/// declare network domains must have at least one of them permitted by the
/// current egress policy — defense in depth: capability states intent,
/// egress (once the operator enables it) is the actual gate.
pub fn check_network_capability(
    caps: &ToolCapabilities,
    egress: &EgressFilter,
) -> Result<(), String> {
    if caps.network.is_empty() {
        return Ok(());
    }
    if caps
        .network
        .iter()
        .any(|domain| egress.check(domain).is_ok())
    {
        Ok(())
    } else {
        Err(format!(
            "none of the declared network capabilities {:?} are permitted by the egress policy",
            caps.network
        ))
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_for_path_resolving_read_only_tool_declares_fs_read() {
        let caps = default_for("git_status");
        assert_eq!(caps.fs_read, vec!["*"]);
        assert!(caps.fs_write.is_empty());
        assert!(!caps.exec);
    }

    #[test]
    fn default_for_build_tool_declares_write_and_exec() {
        let caps = default_for("run_build");
        assert_eq!(caps.fs_write, vec!["*"]);
        assert!(caps.exec);
    }

    #[test]
    fn default_for_daemon_tool_declares_localhost_network() {
        let caps = default_for("semantic_search");
        assert!(caps.network.contains(&"127.0.0.1".to_string()));
        assert!(caps.fs_read.is_empty());
    }

    #[test]
    fn default_for_unlisted_tool_is_empty() {
        let caps = default_for("get_stats");
        assert_eq!(caps, ToolCapabilities::default());
    }

    #[test]
    fn resolve_prefers_toml_override_over_default() {
        let override_caps = ToolCapabilities {
            exec: true,
            ..Default::default()
        };
        let resolved = resolve("get_stats", Some(override_caps.clone()));
        assert_eq!(resolved, override_caps);
    }

    #[test]
    fn resolve_falls_back_to_default_when_no_override() {
        assert_eq!(resolve("run_build", None), default_for("run_build"));
    }

    #[test]
    fn fs_capability_denies_tool_with_no_declared_access() {
        let tmp = TempDir::new().unwrap();
        let caps = ToolCapabilities::default();
        let result = check_fs_capability(&caps, tmp.path(), tmp.path(), &[]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("no declared filesystem capability"));
    }

    #[test]
    fn fs_capability_allows_declared_tool_outside_blocked_paths() {
        let tmp = TempDir::new().unwrap();
        let caps = ToolCapabilities {
            fs_read: vec!["*".into()],
            ..Default::default()
        };
        assert!(check_fs_capability(&caps, tmp.path(), tmp.path(), &[]).is_ok());
    }

    #[test]
    fn fs_capability_still_blocks_explicit_blocked_path() {
        let caps = ToolCapabilities {
            fs_write: vec!["*".into()],
            ..Default::default()
        };
        let sensitive = std::path::Path::new("/home/user/.ssh");
        let blocked = vec!["/home/user/.ssh".to_string()];
        let result = check_fs_capability(&caps, sensitive, sensitive, &blocked);
        assert!(result.is_err());
    }

    /// Regression test for the bug where `workspace_root == resolved_target`
    /// made the jail boundary trivially true for any path. With a real,
    /// distinct workspace root, a target outside it must be denied.
    #[test]
    fn fs_capability_confines_target_to_distinct_workspace_root() {
        let workspace = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let caps = ToolCapabilities {
            fs_read: vec!["*".into()],
            ..Default::default()
        };
        let result = check_fs_capability(&caps, workspace.path(), outside.path(), &[]);
        assert!(result.is_err());
    }

    #[test]
    fn fs_capability_allows_target_actually_inside_workspace_root() {
        let workspace = TempDir::new().unwrap();
        let inside = workspace.path().join("project-a");
        std::fs::create_dir_all(&inside).unwrap();
        let caps = ToolCapabilities {
            fs_read: vec!["*".into()],
            ..Default::default()
        };
        assert!(check_fs_capability(&caps, workspace.path(), &inside, &[]).is_ok());
    }

    /// Regression test for the blocklist bypass via `..`: the old
    /// implementation string-matched the blocklist against the raw,
    /// unresolved target, so a `..` segment that lexically/canonically
    /// resolves into a blocked directory slipped through.
    #[test]
    fn fs_capability_blocklist_not_bypassable_via_dotdot() {
        let workspace = TempDir::new().unwrap();
        let sub = workspace.path().join("sub");
        let secret = workspace.path().join(".secrets");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(&secret).unwrap();

        let traversal_target = sub.join("..").join(".secrets").join("key");
        let blocked = vec![secret.to_string_lossy().to_string()];
        let caps = ToolCapabilities {
            fs_read: vec!["*".into()],
            ..Default::default()
        };

        let result = check_fs_capability(&caps, workspace.path(), &traversal_target, &blocked);
        assert!(result.is_err());
    }

    #[test]
    fn network_capability_skips_check_when_not_declared() {
        let caps = ToolCapabilities::default();
        let egress = EgressFilter::disabled();
        assert!(check_network_capability(&caps, &egress).is_ok());
    }

    #[test]
    fn network_capability_allows_when_egress_disabled() {
        // Matches EgressFilter's own documented decision order: not enabled -> allow all.
        let caps = ToolCapabilities {
            network: vec!["127.0.0.1".into()],
            ..Default::default()
        };
        let egress = EgressFilter::disabled();
        assert!(check_network_capability(&caps, &egress).is_ok());
    }

    #[test]
    fn network_capability_denied_when_egress_enabled_without_matching_allow() {
        let caps = ToolCapabilities {
            network: vec!["127.0.0.1".into()],
            ..Default::default()
        };
        let egress = EgressFilter {
            enabled: true,
            deny_all: false,
            allowed_domains: vec!["api.github.com".into()],
            blocked_domains: vec![],
        };
        let result = check_network_capability(&caps, &egress);
        assert!(result.is_err());
    }

    #[test]
    fn network_capability_allowed_when_egress_enabled_with_matching_allow() {
        let caps = ToolCapabilities {
            network: vec!["127.0.0.1".into()],
            ..Default::default()
        };
        let egress = EgressFilter {
            enabled: true,
            deny_all: false,
            allowed_domains: vec!["127.0.0.1".into()],
            blocked_domains: vec![],
        };
        assert!(check_network_capability(&caps, &egress).is_ok());
    }
}
