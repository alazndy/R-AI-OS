use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiRun {
    pub id: u64,
    pub workflow_name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub branch: String,
    pub created_at: String,
    pub html_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiJob {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub duration_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiReport {
    pub run: CiRun,
    pub jobs: Vec<CiJob>,
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Parse "owner/repo" from a GitHub remote URL (HTTPS or SSH).
pub fn parse_owner_repo(remote_url: &str) -> Option<String> {
    // HTTPS: https://github.com/owner/repo.git
    if let Some(rest) = remote_url.strip_prefix("https://github.com/") {
        let repo = rest.trim_end_matches(".git");
        let parts: Vec<&str> = repo.splitn(2, '/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Some(format!("{}/{}", parts[0], parts[1]));
        }
    }
    // SSH: git@github.com:owner/repo.git
    if let Some(rest) = remote_url.strip_prefix("git@github.com:") {
        let repo = rest.trim_end_matches(".git");
        let parts: Vec<&str> = repo.splitn(2, '/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Some(format!("{}/{}", parts[0], parts[1]));
        }
    }
    None
}

/// Fetch the latest GitHub Actions run and its job breakdown for the given project.
pub fn get_ci_status(project_path: &Path) -> Result<CiReport> {
    let remote_url = get_remote_url(project_path)
        .ok_or_else(|| anyhow::anyhow!("No GitHub remote found for this project"))?;

    let slug = parse_owner_repo(&remote_url)
        .ok_or_else(|| anyhow::anyhow!("Could not parse GitHub owner/repo from: {}", remote_url))?;

    let runs_json = gh_api(&format!("/repos/{}/actions/runs?per_page=1", slug))?;
    let run_val = runs_json["workflow_runs"]
        .as_array()
        .and_then(|a| a.first())
        .ok_or_else(|| anyhow::anyhow!("No CI runs found for {}", slug))?;

    let run = CiRun {
        id: run_val["id"].as_u64().unwrap_or(0),
        workflow_name: run_val["name"].as_str().unwrap_or("CI").to_string(),
        status: run_val["status"].as_str().unwrap_or("unknown").to_string(),
        conclusion: run_val["conclusion"].as_str().map(str::to_string),
        branch: run_val["head_branch"].as_str().unwrap_or("?").to_string(),
        created_at: run_val["created_at"].as_str().unwrap_or("").to_string(),
        html_url: run_val["html_url"].as_str().unwrap_or("").to_string(),
    };

    let jobs_json = gh_api(&format!("/repos/{}/actions/runs/{}/jobs", slug, run.id))?;
    let jobs: Vec<CiJob> = jobs_json["jobs"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|j| {
            let started = j["started_at"].as_str().unwrap_or("");
            let completed = j["completed_at"].as_str().unwrap_or("");
            CiJob {
                name: j["name"].as_str().unwrap_or("?").to_string(),
                status: j["status"].as_str().unwrap_or("unknown").to_string(),
                conclusion: j["conclusion"].as_str().map(str::to_string),
                duration_secs: parse_duration_secs(started, completed),
            }
        })
        .collect();

    Ok(CiReport { run, jobs })
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn get_remote_url(project_path: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(project_path)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn gh_api(endpoint: &str) -> Result<serde_json::Value> {
    let out = std::process::Command::new("gh")
        .args(["api", endpoint])
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!("gh CLI not found — install GitHub CLI: https://cli.github.com")
            } else {
                anyhow::anyhow!("gh command failed: {e}")
            }
        })?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("401") || stderr.contains("authentication") {
            anyhow::bail!("Not authenticated — run: gh auth login");
        }
        anyhow::bail!("GitHub API error: {}", stderr.trim());
    }

    let json: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| anyhow::anyhow!("Failed to parse GitHub API response: {e}"))?;
    Ok(json)
}

fn parse_duration_secs(started_at: &str, completed_at: &str) -> Option<u64> {
    if started_at.is_empty() || completed_at.is_empty() {
        return None;
    }
    let start = rfc3339_to_secs(started_at)?;
    let end = rfc3339_to_secs(completed_at)?;
    end.checked_sub(start)
}

fn rfc3339_to_secs(ts: &str) -> Option<u64> {
    let ts = ts.trim_end_matches('Z');
    let parts: Vec<&str> = ts.split('T').collect();
    if parts.len() != 2 {
        return None;
    }
    let date_parts: Vec<u64> = parts[0].split('-').filter_map(|s| s.parse().ok()).collect();
    let time_parts: Vec<u64> = parts[1].split(':').filter_map(|s| s.parse().ok()).collect();
    if date_parts.len() < 3 || time_parts.len() < 3 {
        return None;
    }
    let y = date_parts[0].saturating_sub(1970);
    let d = y * 365 + (date_parts[1].saturating_sub(1)) * 30 + date_parts[2];
    Some(d * 86400 + time_parts[0] * 3600 + time_parts[1] * 60 + time_parts[2])
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_https_url() {
        let result = parse_owner_repo("https://github.com/alazndy/R-AI-OS.git");
        assert_eq!(result, Some("alazndy/R-AI-OS".to_string()));
    }

    #[test]
    fn parse_https_url_no_git_suffix() {
        let result = parse_owner_repo("https://github.com/alazndy/R-AI-OS");
        assert_eq!(result, Some("alazndy/R-AI-OS".to_string()));
    }

    #[test]
    fn parse_ssh_url() {
        let result = parse_owner_repo("git@github.com:alazndy/R-AI-OS.git");
        assert_eq!(result, Some("alazndy/R-AI-OS".to_string()));
    }

    #[test]
    fn parse_invalid_url_returns_none() {
        assert_eq!(parse_owner_repo("invalid"), None);
        assert_eq!(parse_owner_repo(""), None);
        assert_eq!(parse_owner_repo("https://gitlab.com/foo/bar"), None);
    }
}
