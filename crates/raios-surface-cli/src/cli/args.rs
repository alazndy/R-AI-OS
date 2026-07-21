use super::action_types::*;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

// ─── CLI types ────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "raios",
    about = "AI OS Terminal Control Center — Rust Edition",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
    #[arg(short, long, global = true)]
    pub json: bool,
    /// Quick scan of the current directory for refactor candidates
    #[arg(long)]
    pub refactor: bool,
    /// Connect TUI to a remote R-AI-OS Hub (e.g. 100.x.x.x or cortex.ts.net)
    #[arg(long, global = true)]
    pub remote: Option<String>,
}

#[derive(Subcommand)]
pub enum InstinctCmd {
    /// Add a rule manually to global instincts + project memory.md
    Add {
        rule: String,
        #[arg(short, long)]
        path: Option<PathBuf>,
    },
    /// List all instincts (global + current project)
    List {
        /// Project path or name (optional)
        path: Option<PathBuf>,
    },
    /// Suggest instincts from health analysis with interactive approval
    Suggest { project: Option<String> },
}

#[derive(Subcommand)]
pub enum Commands {
    /// Print master rule files
    Rules { name: Option<String> },
    /// Semantic search or print project memory.md files
    Memory {
        project: Option<String>,
        #[arg(short, long)]
        query: Option<String>,
        #[arg(short = 'n', long, default_value = "5")]
        top: usize,
    },
    /// Print mempalace.yaml
    Mempalace,
    /// List all projects with memory.md
    Projects,
    /// List agent config files and their status
    Agents,
    /// View any known file by name or path
    View { name: String },
    /// Run discovery engine to find new projects
    Discover,
    /// Get health report for a project (dirty, compliance, etc.)
    Health { project: Option<String> },
    /// Tiered health check for a provider or agent
    Doctor {
        /// Provider/agent name (e.g. claude, codex, opencode, agy)
        agent: String,
        /// Maximum tier to test (offline, auth, full)
        #[arg(short, long)]
        tier: Option<String>,
    },
    /// Print current version
    Version,
    /// Run as MCP server (stdio transport)
    #[command(name = "mcp-server")]
    McpServer,
    /// Run an agent as a child process with execution proxy
    Run {
        agent: String,
        #[arg(short, long)]
        project: Option<String>,
        #[arg(short, long)]
        timeout: Option<u64>,
        /// Extra flags forwarded verbatim to the agent binary (e.g. --model opus --print "hi")
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra: Vec<String>,
    },
    /// Record an explicit interactive note for the current wrapper child session.
    #[command(name = "wrapper-note")]
    WrapperNote {
        /// User decision or progress note (1..=500 characters; secrets rejected)
        note: String,
    },
    /// Commit dirty projects in bulk (optionally push)
    Commit {
        #[arg(short, long)]
        project: Option<String>,
        #[arg(short, long)]
        message: Option<String>,
        #[arg(long)]
        push: bool,
        #[arg(long)]
        dry_run: bool,
    },
    /// Show workspace portfolio statistics
    Stats,
    /// Search the current project (Semantic + BM25). Pass --dir to scan a different directory fully.
    Search {
        query: String,
        #[arg(short, long, default_value = "8")]
        top_k: usize,
        #[arg(long)]
        reindex: bool,
        /// Directory to scan (defaults to the current working directory)
        #[arg(long)]
        dir: Option<std::path::PathBuf>,
    },
    /// Exact/regex search over the trigram index (grep-equivalent, exhaustive within scope)
    Locate {
        pattern: String,
        /// Directory to scan (defaults to the current working directory)
        #[arg(long)]
        dir: Option<std::path::PathBuf>,
        #[arg(short = 'i', long)]
        ignore_case: bool,
        #[arg(long)]
        reindex: bool,
    },
    /// Scan dependency licenses for copyleft (GPL/AGPL/LGPL) and unknown licenses
    License {
        /// Project name or path (omit for current directory)
        project: Option<String>,
    },
    /// Run Google Lighthouse web audit on a URL
    Audit {
        /// URL to audit (e.g. https://example.com)
        url: String,
        /// Fail if any score is below this threshold (0-100)
        #[arg(short, long)]
        threshold: Option<u8>,
    },
    /// Run OWASP security scan — pass a path or project name, or omit for all projects
    Security {
        /// Project name filter or absolute path (omit to scan all)
        target: Option<String>,
        #[arg(long)]
        full: bool,
        #[arg(short, long)]
        watch: bool,
    },
    /// Scan source files for refactor candidates (large files, risky patterns, deep nesting)
    Refactor {
        /// Project name or absolute path (omit for current directory)
        target: Option<String>,
        /// Default line count threshold for HIGH severity (default: 500)
        #[arg(long, default_value_t = 500)]
        high_lines: usize,
        /// Default line count threshold for MEDIUM severity (default: 300)
        #[arg(long, default_value_t = 300)]
        medium_lines: usize,
        /// Default risky-pattern count for HIGH severity (default: 10)
        #[arg(long, default_value_t = 10)]
        high_unwrap: usize,
        /// Default risky-pattern count for MEDIUM severity (default: 5)
        #[arg(long, default_value_t = 5)]
        medium_unwrap: usize,
        /// Default nesting depth threshold for HIGH severity (default: 10)
        #[arg(long, default_value_t = 10)]
        high_nesting: usize,
        /// Default nesting depth threshold for MEDIUM severity (default: 8)
        #[arg(long, default_value_t = 8)]
        medium_nesting: usize,
        /// Per-extension threshold overrides as JSON, e.g. '{"rs":{"high_lines":600},"kt":{"high_lines":800}}'
        #[arg(long)]
        ext_config: Option<String>,
    },
    /// Scaffold a new project following MASTER.md rules
    New {
        name: String,
        #[arg(short, long, default_value = "")]
        category: String,
        #[arg(long)]
        github: bool,
        #[arg(long)]
        no_vault: bool,
    },
    /// Automatically route a task to the best specialist agent
    Task {
        description: String,
        #[arg(short, long)]
        project: Option<String>,
        #[arg(short, long)]
        agent: Option<String>,
    },
    /// Atomically hand a task off to another agent via the control plane (no STATE.json)
    Handoff {
        /// Target agent identity
        #[arg(long)]
        to: HandoffTarget,
        /// Outcome of the work being handed off
        #[arg(long)]
        status: HandoffStatus,
        /// Verbatim context for the next agent — no filler. Mutually exclusive
        /// with --report; either one is required.
        #[arg(long)]
        msg: Option<String>,
        /// Path to a JSON file with a structured HandoffReport (findings,
        /// evidence, edge_cases_considered, open_questions, confidence,
        /// what_i_did_not_check) instead of a bare --msg string.
        #[arg(long)]
        report: Option<PathBuf>,
        #[arg(short, long)]
        project: Option<String>,
    },
    /// Install/Bootstrap the entire ECC, Maestro, and system architecture
    Bootstrap,
    /// Bump project version (semver) and optionally update CHANGELOG
    VersionBump {
        level: String,
        project: Option<String>,
        #[arg(long)]
        changelog: bool,
        #[arg(long)]
        tag: bool,
    },
    /// Show current version and changelog since last tag
    VersionInfo { project: Option<String> },
    /// Analyze disk usage of a project or all projects
    Disk { project: Option<String> },
    /// Remove build artifacts (target/, node_modules/, __pycache__, etc.)
    Clean {
        project: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        all: bool,
    },
    /// List all listening ports with PID and process name
    Ps {
        #[arg(short, long)]
        procs: bool,
        #[arg(short, long, default_value = "15")]
        top: usize,
    },
    /// Show local usage/quota signals for Codex, Claude, and Antigravity
    Usage,
    /// Kill a process by port number
    KillPort { port: u16 },
    /// Check .env files: missing keys, empty values, undocumented secrets
    Env {
        project: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// Check dependencies: outdated packages and CVE vulnerabilities
    Deps {
        project: Option<String>,
        #[arg(long)]
        audit: bool,
        #[arg(long)]
        all: bool,
    },
    /// Build a project (auto-detects Rust/Node/Python/Go/Android)
    Build {
        project: Option<String>,
        /// Android: assembleRelease instead of assembleDebug
        #[arg(long)]
        release: bool,
        /// Android: compileDebugKotlin (type-check only, no APK)
        #[arg(long)]
        check: bool,
    },
    /// Run tests for a project (auto-detects Rust/Node/Python/Go/Android)
    Test {
        project: Option<String>,
        /// Test all projects in portfolio
        #[arg(long)]
        all: bool,
        /// Android: run connectedAndroidTest (requires connected device/emulator)
        #[arg(long)]
        instrumented: bool,
    },
    /// Git operations on any project
    Git {
        #[command(subcommand)]
        cmd: GitCommands,
    },
    /// Manage project instincts (learned rules)
    Instinct {
        #[command(subcommand)]
        cmd: InstinctCmd,
    },
    /// Show GitHub Actions CI/CD status for a project
    Ci { project: Option<String> },
    /// Index or re-index the Cortex semantic memory store
    CortexIndex {
        #[arg(long)]
        force: bool,
    },
    /// Manage parallel swarm tasks in isolated git worktrees
    Swarm {
        #[command(subcommand)]
        action: SwarmAction,
    },
    /// Semantically route a query to the best matching raios capability
    Route { query: String },
    /// Manage evolutionary instinct candidates learned from job outcomes
    Evolve {
        #[command(subcommand)]
        action: EvolveAction,
    },
    /// Verify the integrity of the audit ledger hash chain (tamper detection)
    #[command(name = "verify-chain")]
    VerifyChain {
        /// Show last N entries before the result
        #[arg(short = 'n', long, default_value = "0")]
        last: usize,
    },
    /// Show MCP tool rate-limit configuration from raios-policy.toml
    #[command(name = "rate-status")]
    RateStatus,
    /// Show or reset the MCP tool manifest pin (supply-chain tamper detection)
    #[command(name = "pin-reset")]
    PinReset,
    /// Show the current pinned tool manifest hash
    #[command(name = "pin-status")]
    PinStatus,
    /// Manage the MCP tool call quarantine queue (Phase 14)
    Quarantine {
        #[command(subcommand)]
        action: QuarantineAction,
    },
    /// Manage TTL-scoped secret leases for MCP tools (Phase 12)
    Secret {
        #[command(subcommand)]
        action: SecretAction,
    },
    /// Update a task's status (used by VS Code sidebar write-back)
    #[command(name = "task-update")]
    TaskUpdate {
        /// Task ID (cp_tasks.id)
        id: String,
        /// New status: pending | in_progress | completed | cancelled
        #[arg(long)]
        status: String,
    },
    /// Manage autonomous scheduled agent jobs
    Cron {
        #[command(subcommand)]
        action: CronAction,
    },
    /// Install/remove shell wrapper functions that route agent commands through raios
    #[command(name = "agent-wrapper")]
    AgentWrapper {
        #[command(subcommand)]
        action: AgentWrapperCmd,
    },
    /// Show recent wrapper-routed agent sessions
    Sessions {
        /// Filter by agent name (claude, codex, opencode, agy)
        agent: Option<String>,
        /// Number of sessions to show (default: 15)
        #[arg(short = 'n', long, default_value = "15")]
        top: usize,
        /// Render one session's event stream as a Mermaid canvas
        #[arg(long)]
        canvas: Option<String>,
    },
    /// Show per-agent performance stats: run count, success rate, average
    /// duration, exit_reason distribution — aggregated from cp_agent_runs.
    /// Does not report token usage or repetition (not tracked anywhere today).
    #[command(name = "agent-stats")]
    AgentStats {
        /// Agent identity to filter to (e.g. claude_kaira). Omit for all agents.
        agent: Option<String>,
    },
    /// Generate a memory.md Change Log entry from the last Claude session transcript
    #[command(name = "memory-gen")]
    MemoryGen {
        /// Project path or name (omit for current directory)
        project: Option<String>,
    },
    /// Manage the always-on R-AI-OS Cortex Hub (aiosd daemon)
    Hub {
        #[command(subcommand)]
        action: HubAction,
    },
    /// Manage raios-native memory items (stored in raios DB, LLM-independent)
    #[command(name = "mem")]
    Mem {
        #[command(subcommand)]
        action: MemAction,
    },
    /// Record and search operational command outcomes and fix context
    Trace {
        #[command(subcommand)]
        action: TraceAction,
    },
    /// Read or safely execute local Product Factory lifecycle commands
    Factory {
        #[command(subcommand)]
        action: FactoryAction,
    },
    /// Search historical agent transcripts through the read-only ANKA cache
    Anka {
        #[command(subcommand)]
        action: AnkaAction,
    },
    /// Manage the MCP/WS tool-call security policy (raios-policy.toml)
    Policy {
        #[command(subcommand)]
        action: PolicyCmd,
    },
    /// Workspace-wide health reflection: dirty projects, stale docs, score
    Reflect,
    /// Pre-commit gate: staged check, secrets, CVE audit, security scan
    #[command(name = "pre-flight")]
    PreFlight {
        /// Project name or path (omit for current directory)
        project: Option<String>,
    },
    /// Run a raios extension command  (raios ext <name> <subcommand> [args...])
    Ext {
        /// Extension name, or 'list' to show all discovered extensions
        name: String,
        /// Subcommand and its arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}
