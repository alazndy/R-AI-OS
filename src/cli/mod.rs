mod agent_wrapper;
mod audit;
mod dev;
mod ext;
mod git;
mod handoff;
mod health;
mod hub;
mod instinct;
mod new;
mod preflight;
mod reflect;
mod refactor;
mod search;
mod security;
mod swarm;
mod version;
mod workspace;
mod cron;

use crate::config::Config;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

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
    /// Search across the entire Dev Ops workspace (Semantic + BM25)
    Search {
        query: String,
        #[arg(short, long, default_value = "8")]
        top_k: usize,
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
        /// Verbatim context for the next agent — no filler
        #[arg(long)]
        msg: String,
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

#[derive(Subcommand, Debug, Clone)]
pub enum HubAction {
    /// Start aiosd in the background and write PID
    Start,
    /// Stop the running hub daemon
    Stop,
    /// Show hub status: PID, uptime, port health
    Status,
    /// Generate and install a systemd user service for auto-start at boot
    Install {
        /// Immediately enable and start via systemctl
        #[arg(long)]
        enable: bool,
    },
    /// Stream daemon logs (last 50 lines + follow)
    Logs {
        /// Number of historical lines to show before following
        #[arg(short = 'n', long, default_value = "50")]
        lines: usize,
    },
    /// Manage the remote access API key
    ApiKey {
        #[command(subcommand)]
        action: HubApiKeyAction,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum HubApiKeyAction {
    /// Generate a new API key (write key file + hash to policy.toml)
    Generate {
        /// Overwrite existing key
        #[arg(long)]
        force: bool,
    },
    /// Print the current API key
    Show,
}

#[derive(Subcommand, Debug, Clone)]
pub enum MemAction {
    /// List memory items for a project
    List {
        /// Project path (omit for current directory)
        #[arg(short, long)]
        project: Option<String>,
        /// Filter by type: user|feedback|project|reference
        #[arg(short = 't', long)]
        item_type: Option<String>,
    },
    /// Show a single memory item by slug
    Get {
        slug: String,
        #[arg(short, long)]
        project: Option<String>,
    },
    /// Add or update a memory item
    Add {
        #[arg(long)]
        item_type: String,
        #[arg(long)]
        slug: String,
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long, allow_hyphen_values = true)]
        body: String,
        #[arg(short, long)]
        project: Option<String>,
    },
    /// Delete a memory item by slug
    Delete {
        slug: String,
        #[arg(short, long)]
        project: Option<String>,
    },
    /// Export all DB memory items to ~/.claude/projects/<key>/memory/ markdown files
    Export {
        #[arg(short, long)]
        project: Option<String>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum AgentWrapperCmd {
    /// Install shell functions routing agent commands through raios (all agents by default)
    Install {
        /// Specific agents to wrap (omit for all: claude codex opencode agy)
        agents: Vec<String>,
    },
    /// Remove wrapper shell functions (all agents by default)
    Remove {
        /// Specific agents to remove (omit for all)
        agents: Vec<String>,
    },
    /// Show wrapper status for all agents
    Status,
}

#[derive(Subcommand, Debug, Clone)]
pub enum CronAction {
    /// Schedule a new recurring agent job
    Add {
        title: String,
        /// Interval: 30s | 5m | 6h | 1d
        #[arg(long)]
        every: String,
        /// Agent: claude | codex | opencode | agy
        #[arg(long, default_value = "claude")]
        agent: String,
        /// Task description injected as the agent's prompt
        #[arg(long)]
        task: String,
    },
    /// List all active scheduled jobs
    List,
    /// Remove a scheduled job (soft delete)
    Remove { id: String },
    /// Pause a scheduled job
    Pause { id: String },
    /// Resume a paused scheduled job
    Resume { id: String },
    /// Immediately trigger a job (bypasses next_run_at, fires via daemon on next tick)
    Run { id: String },
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffTarget {
    ClaudeKaira,
    CodexKaira,
    OpencodeKaira,
    AntigravityKaira,
}

impl HandoffTarget {
    pub fn as_str(&self) -> &'static str {
        match self {
            HandoffTarget::ClaudeKaira => "claude_kaira",
            HandoffTarget::CodexKaira => "codex_kaira",
            HandoffTarget::OpencodeKaira => "opencode_kaira",
            HandoffTarget::AntigravityKaira => "antigravity_kaira",
        }
    }
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffStatus {
    Success,
    Failed,
    Blocker,
}

impl HandoffStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            HandoffStatus::Success => "SUCCESS",
            HandoffStatus::Failed => "FAILED",
            HandoffStatus::Blocker => "BLOCKER",
        }
    }
}

#[derive(Subcommand)]
pub enum QuarantineAction {
    /// List pending quarantine items
    List,
    /// List all quarantine items (all statuses)
    All,
    /// Approve a queued tool call (agent must retry after approval)
    Approve { id: String },
    /// Deny a queued tool call
    Deny { id: String },
    /// Remove an entry from the queue entirely
    Clear { id: String },
}

#[derive(Subcommand)]
pub enum SecretAction {
    /// Grant a tool access to an env var for a limited time (e.g. --ttl 5m)
    Grant {
        /// Tool name that receives the lease
        tool: String,
        /// Environment variable to expose (value read from host env at call time)
        env_var: String,
        /// TTL duration: 30s, 5m, 2h, 1d
        #[arg(long, default_value = "5m")]
        ttl: String,
    },
    /// List all active secret leases
    List,
    /// List all leases including expired/revoked
    All,
    /// Revoke an active lease by ID
    Revoke { id: String },
}

#[derive(Subcommand)]
pub enum SwarmAction {
    /// Start a new swarm task in an isolated git worktree
    Start {
        #[arg(long)]
        project: String,
        #[arg(long)]
        path: String,
        #[arg(long)]
        description: String,
        #[arg(long, default_value = "claude")]
        agent: String,
    },
    /// List all active swarm tasks
    List,
    /// Approve and merge a completed swarm task
    Approve { task_id: String },
    /// Reject and discard a swarm task
    Reject { task_id: String },
}

#[derive(Subcommand)]
pub enum EvolveAction {
    /// List pending instinct candidates
    List {
        #[arg(long, default_value = "20")]
        limit: u64,
    },
    /// Promote a candidate rule to active instincts
    Promote { rule: String },
    /// Prune expired candidates
    Prune,
}

#[derive(Subcommand)]
pub enum GitCommands {
    /// Show working tree status
    Status { project: Option<String> },
    /// Show commit log
    Log {
        project: Option<String>,
        #[arg(short, long, default_value = "10")]
        count: usize,
    },
    /// Show diff
    Diff {
        project: Option<String>,
        #[arg(long)]
        staged: bool,
    },
    /// Stage all changes and commit
    Commit {
        message: String,
        project: Option<String>,
        #[arg(long)]
        push: bool,
    },
    /// Push commits to remote
    Push { project: Option<String> },
    /// Pull from remote
    Pull { project: Option<String> },
    /// List branches
    Branches { project: Option<String> },
    /// Checkout a branch
    Checkout {
        branch: String,
        project: Option<String>,
    },
    /// Create and checkout a new branch
    Branch {
        name: String,
        project: Option<String>,
    },
}

// ─── Config helper ────────────────────────────────────────────────────────────

fn load_cfg() -> Config {
    if let Some(cfg) = Config::load() {
        return cfg;
    }
    Config::from_detect_result(Config::auto_detect())
}

pub(crate) fn resolve_project_path(project: Option<String>, dev_ops: &Path) -> PathBuf {
    match project {
        None => std::env::current_dir().unwrap_or_else(|_| dev_ops.to_path_buf()),
        Some(ref p) => {
            let direct = Path::new(p);
            if direct.exists() {
                return direct.to_path_buf();
            }
            if let Ok(conn) = crate::db::open_db() {
                if let Ok(projects) = crate::db::load_all_projects(&conn) {
                    if let Some(found) = projects
                        .iter()
                        .find(|pr| pr.name.to_lowercase().contains(&p.to_lowercase()))
                    {
                        return PathBuf::from(&found.path);
                    }
                }
            }
            direct.to_path_buf()
        }
    }
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn run(cli: Cli) {
    let cfg = load_cfg();
    let cmd = cli.command.expect("Subcommand missing");
    match cmd {
        Commands::Rules { name } => workspace::cmd_rules(name, &cfg.master_md_path, cli.json),
        Commands::Memory {
            project,
            query,
            top,
        } => workspace::cmd_memory(project, query, top, &cfg.dev_ops_path, cli.json),
        Commands::Mempalace => workspace::cmd_mempalace(&cfg.dev_ops_path, cli.json),
        Commands::Projects => workspace::cmd_projects(&cfg.dev_ops_path, cli.json),
        Commands::Agents => workspace::cmd_agents(cli.json),
        Commands::View { name } => workspace::cmd_view(name, &cfg.master_md_path, cli.json),
        Commands::Discover => workspace::cmd_discover(&cfg.dev_ops_path, cli.json),
        Commands::Health { project } => health::cmd_health(project, &cfg.dev_ops_path, cli.json),
        Commands::Version => println!("raios v{}", env!("CARGO_PKG_VERSION")),
        Commands::McpServer => {
            if let Err(e) = crate::mcp_server::run_stdio() {
                eprintln!("MCP server error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Run {
            agent,
            project,
            timeout,
        } => {
            if let Err(e) = crate::agent_runner::run_agent(&agent, project, timeout) {
                eprintln!("Agent Runner Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Commit {
            project,
            message,
            push,
            dry_run,
        } => health::cmd_commit(project, message, push, dry_run, &cfg.dev_ops_path, cli.json),
        Commands::Stats => health::cmd_stats(&cfg.dev_ops_path, cli.json),
        Commands::Search {
            query,
            top_k,
            reindex,
        } => search::cmd_search(&query, top_k, reindex, &cfg.dev_ops_path, cli.json),
        Commands::License { project } => {
            security::cmd_license(project, &cfg.dev_ops_path, cli.json)
        }
        Commands::Audit { url, threshold } => {
            let exit = audit::cmd_audit(&url, threshold, cli.json);
            std::process::exit(exit);
        }
        Commands::Security {
            target,
            full,
            watch,
        } => security::cmd_security(target, full, watch, &cfg.dev_ops_path, cli.json),
        Commands::Refactor {
            target,
            high_lines,
            medium_lines,
            high_unwrap,
            medium_unwrap,
            high_nesting,
            medium_nesting,
            ext_config,
        } => refactor::cmd_refactor(
            target,
            &cfg.dev_ops_path,
            cli.json,
            high_lines,
            medium_lines,
            high_unwrap,
            medium_unwrap,
            high_nesting,
            medium_nesting,
            ext_config,
        ),
        Commands::New {
            name,
            category,
            github,
            no_vault,
        } => new::cmd_new(
            &name,
            &category,
            github,
            no_vault,
            &cfg.dev_ops_path,
            cli.json,
        ),
        Commands::Task {
            description,
            project,
            agent,
        } => new::cmd_task(&description, project, agent),
        Commands::Handoff {
            to,
            status,
            msg,
            project,
        } => {
            let project_path = resolve_project_path(project, &cfg.dev_ops_path);
            handoff::cmd_handoff(to, status, msg, &project_path, cli.json);
        }
        Commands::Bootstrap => new::cmd_bootstrap(),
        Commands::VersionBump {
            level,
            project,
            changelog,
            tag,
        } => {
            version::cmd_version_bump(&level, project, changelog, tag, &cfg.dev_ops_path, cli.json)
        }
        Commands::VersionInfo { project } => {
            version::cmd_version_info(project, &cfg.dev_ops_path, cli.json)
        }
        Commands::Disk { project } => dev::cmd_disk(project, &cfg.dev_ops_path, cli.json),
        Commands::Clean {
            project,
            dry_run,
            all,
        } => dev::cmd_clean(project, dry_run, all, &cfg.dev_ops_path, cli.json),
        Commands::Ps { procs, top } => dev::cmd_ps(procs, top, cli.json),
        Commands::Usage => dev::cmd_usage(cli.json),
        Commands::KillPort { port } => dev::cmd_kill_port(port, cli.json),
        Commands::Env { project, all } => dev::cmd_env(project, all, &cfg.dev_ops_path, cli.json),
        Commands::Deps {
            project,
            audit,
            all,
        } => dev::cmd_deps(project, audit, all, &cfg.dev_ops_path, cli.json),
        Commands::Build {
            project,
            release,
            check,
        } => dev::cmd_build(project, release, check, &cfg.dev_ops_path, cli.json),
        Commands::Test {
            project,
            all,
            instrumented,
        } => dev::cmd_test(project, all, instrumented, &cfg.dev_ops_path, cli.json),
        Commands::Git { cmd } => git::cmd_git(cmd, &cfg.dev_ops_path, cli.json),
        Commands::Instinct { cmd } => instinct::cmd_instinct(cmd, &cfg.dev_ops_path, cli.json),
        Commands::Ci { project } => dev::cmd_ci(project, &cfg.dev_ops_path, cli.json),
        Commands::CortexIndex { force } => {
            search::cmd_cortex_index(force, &cfg.dev_ops_path, cli.json)
        }
        Commands::Swarm { action } => swarm::cmd_swarm(action, cli.json),
        Commands::Route { query } => swarm::cmd_route(&query, cli.json),
        Commands::Evolve { action } => swarm::cmd_evolve(action, cli.json),
        Commands::VerifyChain { last } => security::cmd_verify_chain(last, cli.json),
        Commands::RateStatus => security::cmd_rate_status(cli.json),
        Commands::PinReset => security::cmd_pin_reset(cli.json),
        Commands::PinStatus => security::cmd_pin_status(cli.json),
        Commands::Quarantine { action } => security::cmd_quarantine(action, cli.json),
        Commands::Secret { action } => security::cmd_secret(action, cli.json),
        Commands::TaskUpdate { id, status } => cmd_task_update(&id, &status, cli.json),
        Commands::Cron { action } => cron::cmd_cron(action, cli.json),
        Commands::AgentWrapper { action } => {
            let a = match action {
                AgentWrapperCmd::Install { agents } => {
                    agent_wrapper::AgentWrapperAction::Install { agents }
                }
                AgentWrapperCmd::Remove { agents } => {
                    agent_wrapper::AgentWrapperAction::Remove { agents }
                }
                AgentWrapperCmd::Status => agent_wrapper::AgentWrapperAction::Status,
            };
            agent_wrapper::cmd_agent_wrapper(a, cli.json);
        }
        Commands::Sessions { agent, top } => cmd_sessions(agent.as_deref(), top, cli.json),
        Commands::MemoryGen { project } => {
            crate::session_memory::cmd_memory_gen(project.as_deref(), cli.json);
        }
        Commands::Mem { action } => cmd_mem(action, cli.json),
        Commands::Hub { action } => match action {
            HubAction::Start => hub::cmd_start(cli.json),
            HubAction::Stop => hub::cmd_stop(cli.json),
            HubAction::Status => hub::cmd_status(cli.json),
            HubAction::Install { enable } => hub::cmd_install(enable, cli.json),
            HubAction::Logs { lines } => hub::cmd_logs(lines),
            HubAction::ApiKey { action } => match action {
                HubApiKeyAction::Generate { force } => hub::cmd_api_key_generate(force),
                HubApiKeyAction::Show => hub::cmd_api_key_show(),
            },
        },
        Commands::Reflect => reflect::cmd_reflect(&cfg.dev_ops_path, cli.json),
        Commands::PreFlight { project } => {
            let ok = preflight::cmd_preflight(project, &cfg.dev_ops_path);
            if !ok {
                std::process::exit(1);
            }
        }
        Commands::Ext { name, args } => ext::cmd_ext(&name, &args, &cfg.dev_ops_path, cli.json),
    }
}

fn cmd_mem(action: MemAction, json: bool) {
    let project_key_for = |project: &Option<String>| -> String {
        let path = project
            .as_deref()
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| ".".to_string())
            });
        path.replace('/', "-")
    };

    let conn = match crate::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("DB error: {e}");
            return;
        }
    };

    match action {
        MemAction::List { project, item_type } => {
            let key = project_key_for(&project);
            let items = crate::db::mem_list(&conn, &key).unwrap_or_default();
            let items: Vec<_> = if let Some(t) = &item_type {
                items.into_iter().filter(|i| &i.item_type == t).collect()
            } else {
                items
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&items).unwrap_or_default());
                return;
            }
            if items.is_empty() {
                println!("  No memory items for {}", key);
                return;
            }
            println!("\n  MEMORY ITEMS  {}\n", key);
            for i in &items {
                println!("  [{:<10}] {}  \x1b[90m{}\x1b[0m", i.item_type, i.slug, i.description);
            }
            println!();
        }
        MemAction::Get { slug, project } => {
            let key = project_key_for(&project);
            match crate::db::mem_get(&conn, &key, &slug) {
                Ok(Some(item)) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&item).unwrap_or_default());
                    } else {
                        println!("\n  [{}/{}]\n  Type: {}\n  Description: {}\n\n{}\n",
                            item.project_key, item.slug, item.item_type,
                            item.description, item.body);
                    }
                }
                Ok(None) => eprintln!("  Not found: {}", slug),
                Err(e) => eprintln!("  Error: {e}"),
            }
        }
        MemAction::Add { item_type, slug, title, description, body, project } => {
            let key = project_key_for(&project);
            match crate::db::mem_upsert(&conn, &key, &item_type, &slug, &title, &description, &body, None) {
                Ok(()) => {
                    if json {
                        println!("{{\"ok\":true,\"slug\":\"{}\"}}", slug);
                    } else {
                        println!("  \x1b[32m✓\x1b[0m  {}/{}", key, slug);
                    }
                }
                Err(e) => eprintln!("  \x1b[31m✗\x1b[0m  {e}"),
            }
        }
        MemAction::Delete { slug, project } => {
            let key = project_key_for(&project);
            match crate::db::mem_delete(&conn, &key, &slug) {
                Ok(true) => println!("  \x1b[32m✓\x1b[0m  deleted {}/{}", key, slug),
                Ok(false) => eprintln!("  Not found: {}", slug),
                Err(e) => eprintln!("  \x1b[31m✗\x1b[0m  {e}"),
            }
        }
        MemAction::Export { project } => {
            let key = project_key_for(&project);
            let home = std::env::var("HOME").unwrap_or_default();
            let memory_dir = std::path::PathBuf::from(&home)
                .join(".claude/projects")
                .join(&key)
                .join("memory");
            match crate::db::mem_export(&conn, &key, &memory_dir) {
                Ok(n) => {
                    if json {
                        println!("{{\"exported\":{}}}", n);
                    } else {
                        println!("  \x1b[32m✓\x1b[0m  {} item(s) → {}", n, memory_dir.display());
                    }
                }
                Err(e) => eprintln!("  \x1b[31m✗\x1b[0m  {e}"),
            }
        }
    }
}

fn cmd_sessions(agent: Option<&str>, top: usize, json: bool) {
    let conn = match crate::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("DB error: {e}");
            return;
        }
    };
    let rows = match crate::db::cp_sessions_list(&conn, top) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Query error: {e}");
            return;
        }
    };
    let rows: Vec<_> = match agent {
        Some(filter) => {
            let canonical = match filter {
                "claude" => "claude_kaira",
                "codex" => "codex_kaira",
                "opencode" => "opencode_kaira",
                "agy" | "antigravity" => "antigravity_kaira",
                other => other,
            };
            rows.into_iter()
                .filter(|r| r.agent_name == canonical)
                .collect()
        }
        None => rows,
    };

    if json {
        let arr: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "run_id": r.run_id,
                    "agent": r.agent_name,
                    "status": r.status,
                    "started_at": r.started_at,
                    "ended_at": r.ended_at,
                    "exit_reason": r.exit_reason,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr).unwrap_or_default());
        return;
    }

    println!("\n  WRAPPER SESSIONS  (most recent first)\n");
    if rows.is_empty() {
        println!("  No sessions yet. Start an agent with: claude / codex / opencode / agy");
        println!("  (wrapper must be installed: raios agent-wrapper install)\n");
        return;
    }

    let col_w = [8usize, 16, 12, 19, 19, 12];
    println!(
        "  \x1b[90m{:<w0$}  {:<w1$}  {:<w2$}  {:<w3$}  {:<w4$}  {:<w5$}\x1b[0m",
        "RUN",
        "AGENT",
        "STATUS",
        "STARTED",
        "ENDED",
        "EXIT",
        w0 = col_w[0],
        w1 = col_w[1],
        w2 = col_w[2],
        w3 = col_w[3],
        w4 = col_w[4],
        w5 = col_w[5],
    );
    println!("  {}", "─".repeat(col_w.iter().sum::<usize>() + col_w.len() * 2));

    for r in &rows {
        let status_col = match r.status.as_str() {
            "running" => "\x1b[33mrunning \x1b[0m",
            "succeeded" => "\x1b[32msucceeded   \x1b[0m",
            "failed" => "\x1b[31mfailed  \x1b[0m",
            other => other,
        };
        println!(
            "  {:<w0$}  {:<w1$}  {}  {:<w3$}  {:<w4$}  {:<w5$}",
            &r.run_id[..8],
            r.agent_name,
            status_col,
            r.started_at,
            r.ended_at.as_deref().unwrap_or("—"),
            r.exit_reason.as_deref().unwrap_or("—"),
            w0 = col_w[0],
            w1 = col_w[1],
            w3 = col_w[3],
            w4 = col_w[4],
            w5 = col_w[5],
        );
    }
    println!();
}

fn cmd_task_update(id: &str, status: &str, json: bool) {
    let valid = ["pending", "in_progress", "completed", "cancelled"];
    if !valid.contains(&status) {
        if json {
            eprintln!("{{\"status\":\"error\",\"message\":\"invalid status: {status}\"}}");
        } else {
            eprintln!("Invalid status '{status}'. Valid: {}", valid.join(", "));
        }
        std::process::exit(1);
    }
    match crate::db::open_db() {
        Ok(conn) => {
            let now = chrono::Local::now().to_rfc3339();
            let res = conn.execute(
                "UPDATE cp_tasks SET status=?1, updated_at=?2 WHERE id=?3",
                rusqlite::params![status, now, id],
            );
            match res {
                Ok(rows) if rows > 0 => {
                    if json {
                        println!("{{\"status\":\"ok\",\"id\":\"{id}\",\"new_status\":\"{status}\"}}");
                    } else {
                        println!("Task {id} → {status}");
                    }
                }
                Ok(_) => {
                    if json {
                        eprintln!("{{\"status\":\"error\",\"message\":\"task not found: {id}\"}}");
                    } else {
                        eprintln!("Task not found: {id}");
                    }
                    std::process::exit(1);
                }
                Err(e) => {
                    if json {
                        eprintln!("{{\"status\":\"error\",\"message\":\"{e}\"}}");
                    } else {
                        eprintln!("DB error: {e}");
                    }
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to open DB: {e}");
            std::process::exit(1);
        }
    }
}

pub fn run_refactor_flag(json: bool) {
    let dev_ops_path = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    refactor::cmd_refactor(
        None, // Target is None to check the current directory
        &dev_ops_path,
        json,
        500,  // high_lines
        300,  // medium_lines
        10,   // high_unwrap
        5,    // medium_unwrap
        10,   // high_nesting
        8,    // medium_nesting
        None, // ext_config
    );
}
