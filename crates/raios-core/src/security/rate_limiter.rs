//! Phase 13 — Tool call rate limiter (fixed-window counter, zero extra deps).
//!
//! Each tool gets a counter that resets every `window_secs`. When `count >= max_calls`
//! the call is rejected with a structured error that tells the agent when the window resets.

use std::collections::HashMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};

// ─── Config (lives in raios-policy.toml [rate_limits]) ──────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Master on/off switch.
    pub enabled: bool,
    /// Window size in seconds (default: 60).
    #[serde(default = "default_window")]
    pub window_secs: u64,
    /// Default max calls per window for any tool not covered by a rule.
    #[serde(default = "default_max")]
    pub default_max: u32,
    /// Per-tool overrides.  Use `"*"` as tool name for a global cap.
    #[serde(default)]
    pub rules: Vec<ToolRateRule>,
}

fn default_window() -> u64 {
    60
}
fn default_max() -> u32 {
    100
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            window_secs: default_window(),
            default_max: default_max(),
            rules: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRateRule {
    /// Tool name or `"*"` for the global catch-all.
    pub tool: String,
    pub max_calls: u32,
}

// ─── Runtime state ───────────────────────────────────────────────────────────

struct WindowBucket {
    count: u32,
    window_start: Instant,
}

pub struct RateLimiter {
    config: RateLimitConfig,
    buckets: HashMap<String, WindowBucket>,
}

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct RateLimitError {
    pub tool: String,
    pub calls_this_window: u32,
    pub max_calls: u32,
    pub window_secs: u64,
    pub resets_in_secs: u64,
}

impl std::fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rate_limit: tool '{}' called {}/{} times in {}s window — resets in {}s",
            self.tool,
            self.calls_this_window,
            self.max_calls,
            self.window_secs,
            self.resets_in_secs,
        )
    }
}

// ─── Status (for raios rate-status CLI) ─────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ToolRateStatus {
    pub tool: String,
    pub calls_this_window: u32,
    pub max_calls: u32,
    pub window_secs: u64,
    pub resets_in_secs: u64,
}

// ─── Implementation ──────────────────────────────────────────────────────────

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            buckets: HashMap::new(),
        }
    }

    /// Creates a no-op limiter (enabled = false).
    pub fn disabled() -> Self {
        Self::new(RateLimitConfig {
            enabled: false,
            ..Default::default()
        })
    }

    /// Build from an optional policy section; falls back to a permissive default.
    pub fn from_policy(config: Option<RateLimitConfig>) -> Self {
        match config {
            Some(c) => Self::new(c),
            None => Self::disabled(),
        }
    }

    fn max_for(&self, tool: &str) -> u32 {
        // Exact match first, then wildcard, then default.
        self.config
            .rules
            .iter()
            .find(|r| r.tool == tool)
            .or_else(|| self.config.rules.iter().find(|r| r.tool == "*"))
            .map(|r| r.max_calls)
            .unwrap_or(self.config.default_max)
    }

    /// Attempt to consume one call slot for `tool`.
    ///
    /// Returns `Ok(())` if allowed, `Err(RateLimitError)` if the window is exhausted.
    pub fn check(&mut self, tool: &str) -> Result<(), RateLimitError> {
        if !self.config.enabled {
            return Ok(());
        }

        let window = std::time::Duration::from_secs(self.config.window_secs);
        let now = Instant::now();
        let max = self.max_for(tool);

        let bucket = self
            .buckets
            .entry(tool.to_string())
            .or_insert(WindowBucket {
                count: 0,
                window_start: now,
            });

        if now.duration_since(bucket.window_start) >= window {
            bucket.count = 0;
            bucket.window_start = now;
        }

        if bucket.count >= max {
            let elapsed = now.duration_since(bucket.window_start);
            let resets_in = window.saturating_sub(elapsed).as_secs();
            return Err(RateLimitError {
                tool: tool.to_string(),
                calls_this_window: bucket.count,
                max_calls: max,
                window_secs: self.config.window_secs,
                resets_in_secs: resets_in,
            });
        }

        bucket.count += 1;
        Ok(())
    }

    /// Snapshot of all active buckets (for `raios rate-status`).
    pub fn status(&self) -> Vec<ToolRateStatus> {
        let window = std::time::Duration::from_secs(self.config.window_secs);
        let now = Instant::now();
        let mut out: Vec<ToolRateStatus> = self
            .buckets
            .iter()
            .map(|(tool, b)| {
                let elapsed = now.duration_since(b.window_start);
                ToolRateStatus {
                    tool: tool.clone(),
                    calls_this_window: b.count,
                    max_calls: self.max_for(tool),
                    window_secs: self.config.window_secs,
                    resets_in_secs: window.saturating_sub(elapsed).as_secs(),
                }
            })
            .collect();
        out.sort_by_key(|b| std::cmp::Reverse(b.calls_this_window));
        out
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn limiter(max: u32, window: u64) -> RateLimiter {
        RateLimiter::new(RateLimitConfig {
            enabled: true,
            window_secs: window,
            default_max: max,
            rules: vec![],
        })
    }

    #[test]
    fn allows_calls_within_limit() {
        let mut rl = limiter(3, 60);
        assert!(rl.check("run_build").is_ok());
        assert!(rl.check("run_build").is_ok());
        assert!(rl.check("run_build").is_ok());
    }

    #[test]
    fn blocks_on_limit_exceeded() {
        let mut rl = limiter(2, 60);
        assert!(rl.check("run_build").is_ok());
        assert!(rl.check("run_build").is_ok());
        let err = rl.check("run_build").unwrap_err();
        assert_eq!(err.tool, "run_build");
        assert_eq!(err.calls_this_window, 2);
        assert_eq!(err.max_calls, 2);
        assert!(err.to_string().contains("rate_limit:"));
    }

    #[test]
    fn independent_buckets_per_tool() {
        let mut rl = limiter(1, 60);
        assert!(rl.check("tool_a").is_ok());
        // tool_b has its own independent bucket
        assert!(rl.check("tool_b").is_ok());
        assert!(rl.check("tool_a").is_err());
        assert!(rl.check("tool_b").is_err());
    }

    #[test]
    fn disabled_limiter_always_allows() {
        let mut rl = RateLimiter::disabled();
        for _ in 0..1000 {
            assert!(rl.check("any_tool").is_ok());
        }
    }

    #[test]
    fn per_tool_rule_overrides_default() {
        let mut rl = RateLimiter::new(RateLimitConfig {
            enabled: true,
            window_secs: 60,
            default_max: 100,
            rules: vec![ToolRateRule {
                tool: "git_commit".to_string(),
                max_calls: 2,
            }],
        });
        assert!(rl.check("git_commit").is_ok());
        assert!(rl.check("git_commit").is_ok());
        assert!(rl.check("git_commit").is_err()); // limited to 2
                                                  // other tools still get default 100
        assert!(rl.check("list_projects").is_ok());
    }

    #[test]
    fn wildcard_rule_applies_to_all_tools() {
        let mut rl = RateLimiter::new(RateLimitConfig {
            enabled: true,
            window_secs: 60,
            default_max: 100,
            rules: vec![ToolRateRule {
                tool: "*".to_string(),
                max_calls: 1,
            }],
        });
        assert!(rl.check("any_tool").is_ok());
        assert!(rl.check("any_tool").is_err());
        assert!(rl.check("other_tool").is_ok());
        assert!(rl.check("other_tool").is_err());
    }

    #[test]
    fn status_returns_active_buckets() {
        let mut rl = limiter(10, 60);
        rl.check("run_build").unwrap();
        rl.check("run_build").unwrap();
        rl.check("git_status").unwrap();
        let status = rl.status();
        assert_eq!(status.len(), 2);
        let build = status.iter().find(|s| s.tool == "run_build").unwrap();
        assert_eq!(build.calls_this_window, 2);
    }
}
