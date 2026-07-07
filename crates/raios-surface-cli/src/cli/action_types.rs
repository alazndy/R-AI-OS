use clap::Subcommand;

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
    /// Print the current API key (masked by default)
    Show {
        /// Print the full key instead of a masked preview
        #[arg(long)]
        reveal: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum PolicyCmd {
    /// Analyze the audit ledger and propose new [[tools.rules]] entries
    /// (learns from repeated allow/deny/confirm decisions instead of
    /// requiring every rule to be hand-written)
    Suggest {
        /// Minimum number of matching decisions before a rule is suggested
        #[arg(long, default_value_t = 20)]
        min_count: usize,
        /// Write accepted suggestions directly into raios-policy.toml
        /// (idempotent — existing rules for the same tool are left untouched)
        #[arg(long)]
        apply: bool,
    },
    /// Show the resolved policy currently in effect (default action + rules)
    Show,
    /// Show the resolved capability (fs/network/exec) for one or all tools —
    /// a TOML override if declared, otherwise the built-in default
    Caps {
        /// Tool name (omit to list all known tools)
        tool: Option<String>,
    },
    /// Preview what the policy engine would decide for a tool call — allow,
    /// confirm, or deny, and why — WITHOUT running the tool or touching the
    /// audit ledger. Answers "what would happen if I called this?" before it
    /// actually happens.
    Simulate {
        /// Tool name to simulate a call to
        tool: String,
        /// Optional JSON arguments, scanned the same way a real call's
        /// arguments would be (AgentShield pattern check)
        #[arg(long)]
        args: Option<String>,
    },
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
    /// Heuristic sync: scan latest agent transcript → extract items → write to DB + markdown
    Sync {
        /// Agent name (default: claude)
        #[arg(short, long, default_value = "claude")]
        agent: String,
        /// Project path (omit for current directory)
        #[arg(short, long)]
        project: Option<String>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum TraceAction {
    /// Record a command outcome and the fix context that made it useful later
    Record {
        #[arg(long)]
        project: String,
        #[arg(long)]
        command: String,
        #[arg(long, default_value = "")]
        context: String,
        #[arg(long, default_value = "")]
        outcome: String,
        #[arg(long, default_value = "")]
        error: String,
        #[arg(long, default_value = "")]
        fix: String,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long)]
        success: bool,
        #[arg(long, default_value_t = 0.5)]
        confidence: f64,
        #[arg(long)]
        task_id: Option<String>,
    },
    /// Search recorded tool traces for prior fixes
    Search {
        query: String,
        #[arg(short, long)]
        project: Option<String>,
        #[arg(long)]
        success_only: bool,
        #[arg(long)]
        tag: Option<String>,
        #[arg(short = 'n', long, default_value_t = 8)]
        limit: usize,
    },
    /// Export traces as knowledge-graph triples for MemPalace MCP ingestion
    #[command(name = "kg-export")]
    KgExport {
        /// Optional search text; omit to export recent successful traces
        query: Option<String>,
        #[arg(short, long)]
        project: Option<String>,
        #[arg(long, default_value_t = true)]
        success_only: bool,
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },
    /// Delete a trace by id
    Forget { id: String },
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
    /// Generate instinct candidates from successful trace memory rows
    #[command(name = "from-traces")]
    FromTraces {
        /// Filter traces by project name/path fragment
        #[arg(short, long)]
        project: Option<String>,
        /// Maximum number of trace rows to inspect
        #[arg(short = 'n', long, default_value = "50")]
        limit: usize,
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
