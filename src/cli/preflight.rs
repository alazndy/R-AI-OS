use std::path::{Path, PathBuf};
use std::process::Command;

struct Check {
    label: &'static str,
    pass: bool,
    detail: String,
    blocking: bool,
}

pub fn cmd_preflight(project: Option<String>, dev_ops_path: &Path) -> bool {
    let target_path = resolve_path(project.as_deref(), dev_ops_path);
    let target_path = match target_path {
        Some(p) => p,
        None => {
            eprintln!("Project not found. Pass a name or run from project directory.");
            return false;
        }
    };

    let project_name = project
        .as_deref()
        .unwrap_or_else(|| target_path.file_name().unwrap_or_default().to_str().unwrap_or("?"));

    println!();
    println!("━━━ PRE-FLIGHT CHECK: {} ━━━━━━━━━━━━━━━━━━━━━━━━━━", project_name);
    println!();

    let checks = run_checks(&target_path);
    let blockers: Vec<_> = checks.iter().filter(|c| !c.pass && c.blocking).collect();
    let warnings: Vec<_> = checks.iter().filter(|c| !c.pass && !c.blocking).collect();

    for c in &checks {
        let icon = if c.pass { "✓" } else if c.blocking { "✗" } else { "⚠" };
        let detail = if c.detail.is_empty() {
            String::new()
        } else {
            format!("  {}", c.detail)
        };
        println!("  {}  {:<28}{}", icon, c.label, detail);
    }

    println!();
    if blockers.is_empty() && warnings.is_empty() {
        println!("  RESULT: ✓ READY TO COMMIT");
    } else if blockers.is_empty() {
        println!(
            "  RESULT: ⚠ READY (with {} warning(s))",
            warnings.len()
        );
    } else {
        println!(
            "  RESULT: ✗ BLOCKED ({} blocker(s), {} warning(s))",
            blockers.len(),
            warnings.len()
        );
        println!();
        println!("  Fix before commit:");
        for (i, c) in blockers.iter().enumerate() {
            println!("  {}. {} — {}", i + 1, c.label, c.detail);
        }
    }
    println!();

    blockers.is_empty()
}

fn run_checks(path: &Path) -> Vec<Check> {
    vec![
        check_git_staged(path),
        check_git_unstaged(path),
        check_readme(path),
        check_memory_md(path),
        check_sigmap(path),
        check_dep_audit(path),
        check_secrets_in_diff(path),
        check_security_scan(path),
    ]
}

fn check_git_staged(path: &Path) -> Check {
    let out = Command::new("git")
        .args(["-C", &path.to_string_lossy(), "diff", "--cached", "--stat"])
        .output();
    match out {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            let lines: Vec<_> = text.lines().filter(|l| !l.is_empty()).collect();
            let has_staged = !lines.is_empty();
            Check {
                label: "Staged changes",
                pass: has_staged,
                detail: if has_staged {
                    format!("{} file(s) staged", lines.len().saturating_sub(1).max(1))
                } else {
                    "nothing staged for commit".into()
                },
                blocking: !has_staged,
            }
        }
        Err(_) => Check {
            label: "Staged changes",
            pass: false,
            detail: "git not available".into(),
            blocking: true,
        },
    }
}

fn check_git_unstaged(path: &Path) -> Check {
    let out = Command::new("git")
        .args(["-C", &path.to_string_lossy(), "diff", "--stat"])
        .output();
    match out {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            let count = text.lines().filter(|l| !l.is_empty()).count();
            let unstaged = count > 0;
            Check {
                label: "Unstaged changes",
                pass: !unstaged,
                detail: if unstaged {
                    format!("{} file(s) not staged", count.saturating_sub(1).max(1))
                } else {
                    String::new()
                },
                blocking: false,
            }
        }
        Err(_) => Check {
            label: "Unstaged changes",
            pass: true,
            detail: String::new(),
            blocking: false,
        },
    }
}

fn check_readme(path: &Path) -> Check {
    let exists = path.join("README.md").exists();
    Check {
        label: "README.md",
        pass: exists,
        detail: if exists { String::new() } else { "missing — create it".into() },
        blocking: false,
    }
}

fn check_memory_md(path: &Path) -> Check {
    let memory = path.join("memory.md");
    if !memory.exists() {
        return Check {
            label: "memory.md",
            pass: false,
            detail: "missing — use standard template".into(),
            blocking: false,
        };
    }
    let age_days = std::fs::metadata(&memory)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
        .map(|d| d.as_secs() / 86400)
        .unwrap_or(0);

    Check {
        label: "memory.md",
        pass: age_days <= 7,
        detail: if age_days > 7 {
            format!("stale ({}d) — update Change Log section", age_days)
        } else {
            format!("up to date ({}d)", age_days)
        },
        blocking: false,
    }
}

fn check_sigmap(path: &Path) -> Check {
    let exists = path.join("SIGMAP.md").exists();
    Check {
        label: "SIGMAP.md",
        pass: exists,
        detail: if exists { String::new() } else { "missing — run: sigmap".into() },
        blocking: false,
    }
}

fn check_dep_audit(path: &Path) -> Check {
    // Detect package manager and run audit
    if path.join("Cargo.toml").exists() {
        return run_cargo_audit(path);
    }
    if path.join("package.json").exists() {
        return run_npm_audit(path);
    }
    Check {
        label: "Dependency audit",
        pass: true,
        detail: "n/a (no Cargo.toml or package.json)".into(),
        blocking: false,
    }
}

fn run_cargo_audit(path: &Path) -> Check {
    let out = Command::new("cargo")
        .args(["audit", "--quiet"])
        .current_dir(path)
        .output();
    match out {
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let stdout = String::from_utf8_lossy(&o.stdout);
            let combined = format!("{}{}", stdout, stderr);
            let has_vuln = combined.to_lowercase().contains("error")
                || combined.contains("vulnerability")
                || !o.status.success();
            let _vuln_count = combined
                .lines()
                .filter(|l| l.contains("Vulnerable crates:") || l.contains("vulnerability"))
                .count();
            Check {
                label: "Dependency audit",
                pass: !has_vuln,
                detail: if has_vuln {
                    "cargo audit found issues — run: cargo audit".to_string()
                } else {
                    "no known CVEs".into()
                },
                blocking: has_vuln,
            }
        }
        Err(_) => Check {
            label: "Dependency audit",
            pass: true,
            detail: "cargo-audit not installed (skipped)".into(),
            blocking: false,
        },
    }
}

fn run_npm_audit(path: &Path) -> Check {
    let pm = if path.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else {
        "npm"
    };
    let out = Command::new(pm)
        .args(["audit", "--audit-level=high"])
        .current_dir(path)
        .output();
    match out {
        Ok(o) => {
            let pass = o.status.success();
            Check {
                label: "Dependency audit",
                pass,
                detail: if pass {
                    "no high/critical CVEs".into()
                } else {
                    format!("run: {} audit --audit-level=high", pm)
                },
                blocking: !pass,
            }
        }
        Err(_) => Check {
            label: "Dependency audit",
            pass: true,
            detail: format!("{} not available (skipped)", pm),
            blocking: false,
        },
    }
}

fn check_secrets_in_diff(path: &Path) -> Check {
    let out = Command::new("git")
        .args(["-C", &path.to_string_lossy(), "diff", "--cached"])
        .output();
    let diff = match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(_) => return Check { label: "Secrets in diff", pass: true, detail: String::new(), blocking: false },
    };

    // Simple pattern matching — no regex dep needed
    let patterns: &[(&str, &str)] = &[
        ("API key", "api_key="),
        ("AWS key", "AKIA"),
        ("Private key", "BEGIN PRIVATE KEY"),
        ("Token", "token="),
        ("Password", "password="),
    ];

    let diff_lower = diff.to_lowercase();
    let mut found = Vec::new();
    for (name, pat) in patterns {
        if diff_lower.contains(&pat.to_lowercase()) {
            found.push(*name);
        }
    }

    if found.is_empty() {
        Check {
            label: "Secrets in diff",
            pass: true,
            detail: String::new(),
            blocking: false,
        }
    } else {
        Check {
            label: "Secrets in diff",
            pass: false,
            detail: format!("possible secret detected: {} — review diff", found.join(", ")),
            blocking: true,
        }
    }
}

fn check_security_scan(path: &Path) -> Check {
    let report = crate::security::scanner::scan_project_fast(path);
    let high_count = report
        .issues
        .iter()
        .filter(|i| matches!(i.severity, crate::security::Severity::High | crate::security::Severity::Critical))
        .count();
    let total = report.issues.len();

    Check {
        label: "Security scan (OWASP)",
        pass: high_count == 0,
        detail: if total == 0 {
            "clean".into()
        } else if high_count > 0 {
            format!("{} HIGH/CRITICAL finding(s) — run: raios security", high_count)
        } else {
            format!("{} low/medium finding(s)", total)
        },
        blocking: high_count > 0,
    }
}

fn resolve_path(project: Option<&str>, dev_ops_path: &Path) -> Option<PathBuf> {
    // 1. Exact name match in portfolio
    if let Some(name) = project {
        let projects = crate::entities::discover_entities(dev_ops_path);
        if let Some(p) = projects.iter().find(|p| {
            p.name.to_lowercase() == name.to_lowercase()
                || p.name.to_lowercase().contains(&name.to_lowercase())
        }) {
            return Some(p.local_path.clone());
        }
        // 2. Try as direct path
        let path = PathBuf::from(name);
        if path.is_dir() {
            return Some(path);
        }
    }
    // 3. Fallback: current directory
    std::env::current_dir().ok()
}
