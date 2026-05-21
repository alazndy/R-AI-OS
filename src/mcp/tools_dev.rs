use serde_json::{json, Value};

use super::McpServer;

impl McpServer {
    pub(super) fn tool_disk_usage(&self, args: &Value) -> Result<Value, String> {
        let path = self.resolve_git_path(args)?;
        let r = crate::core::disk::analyze(&path);
        let mut lines = vec![format!("Total: {}  Source: {}  Cache: {}  Files: {}",
            crate::core::disk::human_size(r.total_bytes), crate::core::disk::human_size(r.source_bytes),
            crate::core::disk::human_size(r.cache_bytes), r.file_count)];
        for c in &r.cache_dirs {
            lines.push(format!("  cache  {:.<30} {}  ({})", c.path.file_name().unwrap_or_default().to_string_lossy(), crate::core::disk::human_size(c.bytes), c.kind));
        }
        for (f, s) in r.largest_files.iter().take(5) {
            lines.push(format!("  large  {}  {}", f.file_name().unwrap_or_default().to_string_lossy(), crate::core::disk::human_size(*s)));
        }
        Ok(json!({ "content": [{ "type": "text", "text": lines.join("\n") }], "total_mb": r.total_mb(), "source_mb": r.source_mb(), "cache_mb": r.cache_mb(), "file_count": r.file_count, "cache_dirs": r.cache_dirs.len() }))
    }

    pub(super) fn tool_list_ports(&self) -> Result<Value, String> {
        let ports = crate::core::process::list_ports();
        let text = if ports.is_empty() {
            "No listening ports found".into()
        } else {
            let mut lines = vec![format!("{:<8} {:<10} {}", "PORT", "PID", "PROCESS")];
            for p in &ports {
                let pid_s = p.pid.map(|n| n.to_string()).unwrap_or_else(|| "—".into());
                lines.push(format!("{:<8} {:<10} {}", p.port, pid_s, p.process_name.as_deref().unwrap_or("—")));
            }
            lines.join("\n")
        };
        Ok(json!({ "content": [{ "type": "text", "text": text }], "ports": ports }))
    }

    pub(super) fn tool_version_info(&self, args: &Value) -> Result<Value, String> {
        let path = self.resolve_git_path(args)?;
        let v = crate::core::version::info(&path).ok_or_else(|| "No version file found".to_string())?;
        let text = format!("Version: {} ({})\nFile: {}\nLast tag: {}\nCommits since tag: {}", v.current, v.project_type, v.version_file, v.last_tag.as_deref().unwrap_or("none"), v.commits_since_tag);
        Ok(json!({ "content": [{ "type": "text", "text": text }], "current": v.current, "project_type": v.project_type, "last_tag": v.last_tag, "commits_since_tag": v.commits_since_tag }))
    }

    pub(super) fn tool_version_bump(&self, args: &Value) -> Result<Value, String> {
        let path = self.resolve_git_path(args)?;
        let level = args["level"].as_str().ok_or("missing level")?;
        let bump_type = crate::core::version::BumpType::parse(level)
            .ok_or_else(|| format!("Invalid level '{}' — use patch | minor | major", level))?;
        let changelog = args["changelog"].as_bool().unwrap_or(false);
        let tag = args["tag"].as_bool().unwrap_or(false);
        let r = crate::core::version::bump(&path, &bump_type, changelog, tag);
        let text = if r.ok { format!("✓ {} → {}  ({})\n{}", r.old_version, r.new_version, r.version_file, r.changelog_entry) } else { format!("✗ {}", r.message) };
        Ok(json!({ "content": [{ "type": "text", "text": text }], "ok": r.ok, "old_version": r.old_version, "new_version": r.new_version, "changelog_entry": r.changelog_entry }))
    }

    pub(super) fn tool_env_status(&self, args: &Value) -> Result<Value, String> {
        let path = self.resolve_git_path(args)?;
        let r = crate::core::env::check(&path);
        let mut lines = vec![format!("has_env: {}  has_example: {}  total_keys: {}", r.has_env, r.has_example, r.total_env_keys)];
        if !r.missing_keys.is_empty() { lines.push(format!("MISSING ({}):{}", r.missing_keys.len(), r.missing_keys.iter().map(|k| format!(" {}", k)).collect::<String>())); }
        if !r.empty_keys.is_empty() { lines.push(format!("EMPTY ({}):{}", r.empty_keys.len(), r.empty_keys.iter().map(|k| format!(" {}=", k)).collect::<String>())); }
        if !r.undocumented_keys.is_empty() { lines.push(format!("UNDOCUMENTED ({}):{}", r.undocumented_keys.len(), r.undocumented_keys.iter().map(|k| format!(" {}", k)).collect::<String>())); }
        if r.ok { lines.push("✓ All env keys present and set".into()); }
        Ok(json!({ "content": [{ "type": "text", "text": lines.join("\n") }], "ok": r.ok, "has_env": r.has_env, "has_example": r.has_example, "missing_keys": r.missing_keys, "empty_keys": r.empty_keys, "undocumented_keys": r.undocumented_keys, "total_env_keys": r.total_env_keys, "total_example_keys": r.total_example_keys }))
    }

    pub(super) fn tool_deps_status(&self, args: &Value) -> Result<Value, String> {
        let path = self.resolve_git_path(args)?;
        let r = crate::core::deps::check(&path);
        let cve_summary = if r.cve_critical > 0 { format!("🔴 {} CVEs ({} critical)", r.cve_count, r.cve_critical) } else if r.cve_count > 0 { format!("⚠ {} CVEs", r.cve_count) } else { "✓ No known CVEs".into() };
        let outdated_summary = if r.outdated_count > 0 { format!("⚠ {} outdated packages", r.outdated_count) } else { "✓ All deps up to date".into() };
        let details = r.cve_issues.iter().map(|v| format!("[{}] {} {} — {}", v.severity.to_uppercase(), v.package, v.version, v.description))
            .chain(r.outdated.iter().take(10).map(|d| format!("{} {} → {}", d.name, d.current, d.latest)))
            .collect::<Vec<_>>().join("\n");
        Ok(json!({ "content": [{ "type": "text", "text": format!("{}\n{}\n{}", cve_summary, outdated_summary, details) }], "cve_count": r.cve_count, "cve_critical": r.cve_critical, "outdated_count": r.outdated_count, "has_lockfile": r.has_lockfile, "project_type": r.project_type, "tool_missing": r.tool_missing }))
    }

    pub(super) fn tool_run_build(&self, args: &Value) -> Result<Value, String> {
        let path = self.resolve_git_path(args)?;
        let r = crate::core::build::build(&path);
        let status = if r.ok { "✓ OK" } else { "✗ FAILED" };
        let diag = r.diagnostics.iter().map(|d| format!("  [{}] {}:{} — {}", d.level, d.file, d.line.map(|l| l.to_string()).unwrap_or_default(), d.message)).collect::<Vec<_>>().join("\n");
        let text = format!("{} {} — {} in {}ms  ({} warnings, {} errors)\n{}", status, r.project_type, r.command, r.duration_ms, r.warnings, r.errors, diag);
        Ok(json!({ "content": [{ "type": "text", "text": text }], "ok": r.ok, "warnings": r.warnings, "errors": r.errors, "duration_ms": r.duration_ms, "project_type": r.project_type }))
    }

    pub(super) fn tool_run_tests(&self, args: &Value) -> Result<Value, String> {
        let path = self.resolve_git_path(args)?;
        let r = crate::core::build::test(&path);
        let status = if r.ok { "✓" } else { "✗" };
        let fails = r.failures.iter().map(|f| format!("  ↳ {}", f)).collect::<Vec<_>>().join("\n");
        let text = format!("{} {} — {} passed, {} failed, {} ignored  ({}ms)\n{}", status, r.command, r.passed, r.failed, r.ignored, r.duration_ms, fails);
        Ok(json!({ "content": [{ "type": "text", "text": text }], "ok": r.ok, "passed": r.passed, "failed": r.failed, "ignored": r.ignored, "duration_ms": r.duration_ms, "project_type": r.project_type }))
    }
}
