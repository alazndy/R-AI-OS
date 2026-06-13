/// Static capability table: (command_name, space-separated keywords).
static CAPABILITIES: &[(&str, &str)] = &[
    (
        "health",
        "health check compliance grade memory sigmap git dirty status project",
    ),
    (
        "stats",
        "portfolio statistics total projects grade distribution overview",
    ),
    (
        "security",
        "security owasp scan vulnerability injection xss sql secret key password hardcoded",
    ),
    (
        "build",
        "build compile rust node python go cargo npm errors warnings",
    ),
    ("test", "test run tests cargo pytest jest pass fail"),
    (
        "deps",
        "deps dependencies outdated packages cve vulnerability audit cargo npm pip",
    ),
    ("env", "env environment variables missing empty secret keys"),
    (
        "git status",
        "git status branch dirty clean files staged unstaged",
    ),
    ("git log", "git log commits history messages"),
    ("git diff", "git diff changes modified files"),
    ("git commit", "git commit stage push message"),
    ("git push", "git push remote upstream"),
    ("git pull", "git pull fetch sync remote"),
    ("git branches", "git branch list checkout create"),
    (
        "disk",
        "disk usage size cache target node_modules storage space",
    ),
    (
        "clean",
        "clean remove delete cache target node_modules free space",
    ),
    ("ps", "ports processes listening pid port network tcp"),
    ("kill-port", "kill port process stop terminate"),
    ("memory", "memory project notes session context agent"),
    ("search", "search semantic find files content workspace"),
    ("instinct", "instinct rule learned pattern add list suggest"),
    ("evolve", "evolve candidate promote instinct rule learning"),
    ("swarm", "swarm parallel worktree agent task isolated"),
    ("ci", "ci github actions workflow build status check"),
    ("task", "task dispatch agent claude gemini codex route"),
    ("version-info", "version semver tag release changelog"),
    (
        "version-bump",
        "bump version patch minor major changelog tag release",
    ),
    ("discover", "discover scan find new projects workspace"),
    ("new", "new project scaffold create template"),
    (
        "cortex-index",
        "cortex index reindex semantic search rebuild",
    ),
];

/// Score a query against a capability by word overlap (case-insensitive).
fn overlap_score(query: &str, keywords: &str) -> usize {
    let q_words: std::collections::HashSet<&str> = query
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| w.len() > 2)
        .collect();

    keywords
        .split_whitespace()
        .filter(|kw| q_words.contains(kw))
        .count()
}

/// Find the raios capability that best matches a natural language query.
/// Returns `None` if no capability scores > 0.
pub fn route_capability(query: &str) -> Option<String> {
    if query.trim().is_empty() {
        return None;
    }
    let query_lower = query.to_lowercase();

    CAPABILITIES
        .iter()
        .map(|(name, keywords)| {
            let score = overlap_score(&query_lower, keywords);
            (*name, score)
        })
        .filter(|(_, score)| *score > 0)
        .max_by_key(|(_, score)| *score)
        .map(|(name, _)| name.to_string())
}

/// Legacy wrapper kept for MCP tools that may reference AgentRouter.
pub struct AgentRouter;

impl Default for AgentRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRouter {
    pub fn new() -> Self {
        Self
    }

    pub fn init() -> anyhow::Result<Self> {
        Ok(Self::new())
    }

    pub fn route(&self, task: &str) -> anyhow::Result<Option<String>> {
        Ok(route_capability(task))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_finds_deps_for_outdated_packages_query() {
        let result = route_capability("find outdated packages in my rust project");
        assert!(result.is_some(), "Should match a capability");
        let cap = result.unwrap();
        assert!(
            cap.contains("deps") || cap.contains("outdated"),
            "Expected deps-related capability, got: {cap}"
        );
    }

    #[test]
    fn route_finds_security_for_owasp_query() {
        let result = route_capability("scan for sql injection vulnerabilities");
        assert!(result.is_some());
        let cap = result.unwrap();
        assert!(cap.contains("security"), "Expected security, got: {cap}");
    }

    #[test]
    fn route_returns_none_for_empty_query() {
        let result = route_capability("");
        assert!(result.is_none());
    }
}
