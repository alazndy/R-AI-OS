# CI/CD Status Tracking — Design Spec
**Date:** 2026-05-15
**Project:** R-AI-OS
**Status:** Approved

---

## Goal

`raios ci [proje]` komutu — son GitHub Actions run durumunu ve her job'ın ayrı status'unu gösterir.

## Architecture

```
raios ci [project] [--json]
    ├── find project → EntityProject (remote_url)
    ├── parse_owner_repo(remote_url) → "owner/repo"
    ├── gh api /repos/{owner}/{repo}/actions/runs?per_page=1
    ├── gh api /repos/{owner}/{repo}/actions/runs/{run_id}/jobs
    └── print_ci_report(report, json)
```

Auth: `gh auth token` (GitHub CLI).

## Types — `src/core/ci.rs`

```rust
pub struct CiRun {
    pub id: u64,
    pub workflow_name: String,
    pub status: String,             // "completed" | "in_progress" | "queued"
    pub conclusion: Option<String>, // "success" | "failure" | "cancelled"
    pub branch: String,
    pub created_at: String,
    pub html_url: String,
}

pub struct CiJob {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub duration_secs: Option<u64>,
}

pub struct CiReport { pub run: CiRun, pub jobs: Vec<CiJob> }

pub fn get_ci_status(project_path: &Path) -> anyhow::Result<CiReport>
pub fn parse_owner_repo(remote_url: &str) -> Option<String>
```

## Output

**Terminal:**
```
CI: R-AI-OS @ master — ✓ success  (5m ago)
  ✓ build    42s
  ✓ test     1m 23s
  ✗ clippy   18s  ← FAILED
```

**JSON:** `{"run":{...},"jobs":[...]}`

## Error Handling

| Durum | Çıktı |
|-------|-------|
| `gh` kurulu değil | `"gh CLI not found — install GitHub CLI"` |
| Auth yok | `"Not authenticated — run: gh auth login"` |
| Remote URL yok | `"No GitHub remote found"` |
| No CI runs | `"No CI runs found"` |

## Files Changed

| Dosya | Değişiklik |
|-------|-----------|
| `src/core/ci.rs` | Yeni dosya — tüm CI logic |
| `src/core/mod.rs` | `pub mod ci;` |
| `src/cli.rs` | `Commands::Ci` + `cmd_ci()` |

## Tests

1. `parse_owner_repo("https://github.com/alazndy/R-AI-OS.git")` → `"alazndy/R-AI-OS"`
2. `parse_owner_repo("git@github.com:alazndy/R-AI-OS.git")` → `"alazndy/R-AI-OS"` (SSH)
3. `parse_owner_repo("invalid")` → `None`
