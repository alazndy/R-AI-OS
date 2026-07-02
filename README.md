# R-AI-OS Kernel

<p align="center">
  <img src="vscode-extension/icon.png" width="180" alt="R-AI-OS Logo"/>
</p>

<p align="center">
<pre>
  ╔═╗ ══════════════════════════════════════════ ╔═╗
  ║ ╚╗                                          ╔╝ ║
  ╚═╗║         ▄█████████████▄                  ║╔═╝
    ║║      ▄█▀  ┌─────────┐  ▀█▄               ║║
    ║║   ▄█▀  ───│   · │ · │───  ▀█▄            ║║
    ║║   █ ─────│   │ R │  │ ───── █           ║║
    ║║   ▀█▄  ───│   · │ · │───  ▄█▀            ║║
    ║║      ▀█▄  └─────────┘  ▄█▀               ║║
  ╔═╝║         ▀█████████████▀                  ║╚═╗
  ║ ╔╝   · · ·   R - A I - O S   KERNEL  v3.0   ╚╗ ║
  ╚═╝ ══════════════════════════════════════════ ╚═╝
</pre>
</p>

<p align="center">
  <strong>A Hardened, LLM-Native OS Kernel for Autonomous Agent Swarms</strong>
</p>

<p align="center">
  <a href="https://github.com/alazndy/r-ai-os/releases"><img src="https://img.shields.io/badge/version-v3.0.0-blue?style=for-the-badge" alt="Version"></a>
  <a href="https://rust-lang.org"><img src="https://img.shields.io/badge/Built%20with-Rust-orange?style=for-the-badge&logo=rust" alt="Rust"></a>
  <a href="https://github.com/alazndy/r-ai-os/blob/master/LICENSE"><img src="https://img.shields.io/github/license/alazndy/r-ai-os?style=for-the-badge" alt="License"></a>
  <a href="#-security-kernel"><img src="https://img.shields.io/badge/Security-Hardened-green?style=for-the-badge" alt="Security"></a>
  <a href="#-vs-code-extension"><img src="https://img.shields.io/badge/VS%20Code-v0.6.0-blueviolet?style=for-the-badge&logo=visualstudiocode" alt="VS Code"></a>
</p>

<p align="center">
  <a href="#-the-vision">Vision</a> •
  <a href="#-security-kernel">Security</a> •
  <a href="#-tri-protocol-interface">Protocols</a> •
  <a href="#-core-modules">Modules</a> •
  <a href="#-vs-code-extension">VS Code</a> •
  <a href="#-quick-start">Quick Start</a> •
  <a href="#-cli-reference">CLI</a> •
  <a href="#-roadmap">Roadmap</a>
</p>

---

## 🔭 The Vision

R-AI-OS is not a CLI tool — it is a **Kernel**. While traditional operating systems manage hardware, R-AI-OS manages the **AI layer**: a decentralized swarm of 90+ autonomous specialists running across Claude Code, Codex CLI, OpenCode, Antigravity (`agy`), and any MCP-compatible agent.

It solves the fundamental problem of **unsupervised agent execution**: agents that run unchecked can leak secrets, corrupt files, and make unauthorized network calls. R-AI-OS sits between the human and the swarm as a hardened control plane — enforcing policies, auditing every action, and managing context economics.

```
Human → [ R-AI-OS Kernel ] → Agent Swarm (Claude / Gemini / MCP)
              ↓
    ┌──────────────────────────────────────────────┐
    │  Security Kernel  │  Cortex  │  Swarm Mesh  │
    │  Policy Gate      │  BM25+V  │  Lock Mgr    │
    │  Audit Ledger     │  Sigmap  │  Factory Mode│
    └──────────────────────────────────────────────┘
         ↓ TCP :42069   ↓ MCP :42070   ↓ HTTP :42071
```

---

## 🛡️ Security Kernel

The Security Kernel is the core of R-AI-OS. It enforces a **zero-trust model** for all agent tool calls: every action is policy-gated, logged, and auditable. All 4 phases are implemented and tested.

### Architecture

```
src/security/
├── sandbox.rs       # Phase 1 — Filesystem Jail (canonicalize + boundary)
├── policy.rs        # Phase 2 — Policy Manager (TOML allow/deny/confirm)
├── verify_chain.rs  # Phase 3 — Audit Chain (SHA-256 hash-chained SQLite)
└── egress.rs        # Phase 4 — Egress Filter (domain allowlist, fail-closed)
```

### Phase 1 — Filesystem Jail

Prevents agents from reading or writing outside their designated workspace boundary. Uses path canonicalization to defeat traversal attacks.

```toml
# raios-policy.toml
[sandbox]
enabled = true
workspace_root = "/home/user/projects/my-app"
```

### Phase 2 — Policy Manager

Every MCP tool call passes through a policy gate before execution. Rules are defined in `raios-policy.toml` and evaluated in order. Fail-closed by design: `confirm` rules in headless mode deny without an interactive prompt.

```toml
[tools]
default = "allow"

[[tools.rules]]
tool = "bash"
action = "confirm"   # requires human approval

[[tools.rules]]
tool = "write_file"
path_glob = "/etc/**"
action = "deny"
```

### Phase 3 — Audit Chain

Every allow/deny decision is written to a tamper-evident, SHA-256 hash-chained SQLite ledger. Each entry links to the previous entry's hash — any tampering is immediately detectable.

```bash
raios verify-chain          # verify full chain integrity
raios verify-chain -n 50    # show last 50 entries then verify
```

### Phase 4 — Egress Filter

Domain-level allowlist/blocklist for HTTP/HTTPS calls made via MCP tools. Fail-closed: unrecognized domains are denied unless explicitly allowed.

```toml
[egress]
mode = "allowlist"
allowed = ["api.anthropic.com", "api.openai.com", "*.github.com"]
```

### Redaction Engine

Automatically masks sensitive values (API keys, GCP secrets, PII patterns) before they appear in logs or telemetry. Built on `regex` with 20+ detection patterns.

### Session Token Auth

All HTTP API calls require a Bearer token stored in the OS config directory under `raios/.session_token` (SHA-256, 8h TTL). The Host header is additionally validated to block DNS rebinding attacks.

---

## 🔌 Tri-Protocol Interface

All three protocols share one event bus and one security kernel:

| Protocol | Port | Purpose |
| :--- | :--- | :--- |
| `Daemon TCP` | `:42069` | IPC between CLI and background daemon — UUID token auth, mandatory handshake |
| `MCP-over-TCP` | `:42070` | Agent tool calls — policy-gated, every call logged to audit ledger |
| `HTTP / WebSocket` | `:42071` | VS Code extension + external integrations — Bearer auth + Host validation |

### HTTP API Endpoints

| Method | Path | Description |
| :--- | :--- | :--- |
| `GET` | `/api/health` | Daemon health + active agent count |
| `GET` | `/api/projects` | All tracked projects from DaemonState |
| `GET` | `/api/tasks` | Tasks from SQLite (grouped by project) |
| `GET` | `/api/usage` | Local usage/quota signals for Claude, Codex, OpenCode, Antigravity |
| `GET` | `/api/plans` | Plans from `docs/superpowers/plans/*.md` with checkbox progress |
| `GET` | `/api/git-status?path=<dir>` | Git branch + dirty/staged/modified/untracked for a workspace |
| `GET` | `/api/swarm` | Active (non-terminal) swarm tasks |
| `POST` | `/api/approve` | Approve a swarm task (merge branch) or pending diff (write file) |
| `GET` | `/api/stream` | WebSocket — real-time kernel event stream |

---

## 🧠 Core Modules

### 📉 Cortex — Token Budgeter & Context Manager

- **Sigmap:** Up to 97% token reduction via high-density signature mapping (`SIGMAP.md`)
- **BM25 persistence:** Index survives restarts via mtime-based invalidation
- **Vector store:** Binary SQLite BLOBs — transaction-safe, no JSON drift
- **Session memory:** Per-agent `memory.md` auto-append

### 🎯 Unified Agent Router

Maps natural-language task descriptions to the right specialist using local BM25 + vector hybrid indexing. Bridges Maestro (39 agents) and ECC (48 agents) ecosystems natively.

### 🔄 Agent Swarm Mesh

Parallel worktree-based agent execution with coordination primitives:

- **SwarmStore:** SQLite-backed task registry. States: `Initializing → Running → AwaitingReview → Merged / Rejected / Failed`
- **Lock Manager:** File and task-level locks with priority levels (User > Agent > Automation)
- **Radar Whispers:** Real-time context hints pushed to all connected agents
- **Factory Mode:** Submit heavy jobs async; completion fires broadcast + optional webhook

### 📊 Portfolio Intelligence

- **Neural Search:** Semantic search across 140+ projects with BM25 + embeddings
- **Health Scanner:** Background scan for `memory.md` compliance, security leaks, git drift
- **GitHub Sync:** Live star counts and last-commit timestamps
- **Auto-Discovery:** Detects new workspace directories and updates `entities.json`

### 📨 Agent Handoff — Atomic, Control-Plane-Backed

Agents hand work to each other through the same control plane that already tracks tasks, runs, artifacts, and approvals — not a side-channel state file:

```bash
raios handoff --to codex-kaira --status success --msg "skeleton ready, implement auth handlers"
```

- `--msg` is scanned for obvious secrets (AWS/Anthropic/OpenAI/GitHub keys, PEM blocks) and refused before it touches the DB or a process argument list.
- `git diff --stat HEAD` is attached automatically — the receiving agent sees what changed without being told.
- A new handoff to the same agent/project supersedes any still-pending one (old approval → `expired`, artifact → `superseded`, task → `cancelled`), so the queue never accumulates stale notes.
- Delivery is real, not an unread env var: the next `raios run`/`raios task` for that agent injects the `[HANDOVER CONTEXT]` via the CLI's own prompt flag — `claude --append-system-prompt`, `codex <prompt>`, `opencode --prompt`, `agy --prompt-interactive` — and marks it consumed only once the process actually starts.
- Visible at the terminal via the **Inbox** TUI panel (pending approvals, active runs, blocked tasks) or programmatically via the `get_inbox` MCP tool.

### 🧠 Trace Memory — Local Fix Recall

R-AI-OS can now store compact tool/session traces locally and recall them before repeating the same failure:

```bash
raios trace record --project R-AI-OS --command "cargo test -p raios-runtime" \
  --error "trace recall missed partial phrase" \
  --fix "fall back to significant query tokens before project fallback" \
  --tag trace --success
raios trace search "partial phrase" --project R-AI-OS --success-only
raios evolve from-traces --project R-AI-OS
raios trace kg-export "partial phrase" --project R-AI-OS
```

- Traces are stored in SQLite (`tool_traces`) with exact-content deduplication and confidence metadata.
- Secret-like inputs are refused before raw trace content is stored; redacted refusal rows keep an audit trail without persisting the secret.
- Handoffs automatically attach relevant successful trace memory, and `raios run` augments incoming `[HANDOVER CONTEXT]` with prior fixes.
- Post-run session reviews auto-record trace rows only when there is a failure, risk, or learned decision, avoiding noisy memory pollution.
- `raios evolve from-traces` converts useful trace fixes into pending instinct candidates; promotion remains a human-controlled step.
- `raios trace kg-export` emits MemPalace-compatible KG triple JSON for MCP ingestion without silently writing to an external semantic store.

### ⏳ Lifecycle Worker

Background daemon task (`src/daemon/lifecycle.rs`) that keeps project status honest without manual upkeep. Every `lifecycle_interval_secs`, it checks each tracked project's last commit time and transitions status automatically:

| Transition | Trigger |
| :--- | :--- |
| `active` → `beklemede` | No commit for `lifecycle_standby_days` (default: 14) |
| `beklemede` → `archived` | No commit for `lifecycle_archive_days` (default: 90) |
| `beklemede` / `archived` → `active` | A new commit is detected |

Manually pinned statuses (`production`, `early`, `legacy`) are never touched by the worker — only the automatic active/beklemede/archived cycle is managed. Configure via `~/.config/raios/config.toml`:

```toml
[daemon]
lifecycle_standby_days = 14
lifecycle_archive_days = 90
lifecycle_interval_secs = 3600
```

---

## 🖥️ VS Code Extension (v0.6.0)

R-AI-OS ships a native VS Code extension that turns the IDE into a **Hybrid UI** — the control panel for your agent swarm directly in your sidebar.

```
vscode-extension/
├── src/
│   ├── extension.ts              # Activation + provider wiring
│   ├── ipc/
│   │   ├── DaemonClient.ts       # TCP :42069 connection
│   │   ├── TokenBridge.ts        # Session token proxy (XSS-safe)
│   │   └── DaemonManager.ts      # Auto-spawn aiosd, poll token file
│   └── providers/
│       ├── SidebarProvider.ts    # Main WebviewView control panel
│       ├── StatusBarProvider.ts  # Live daemon indicator
│       ├── DiagnosticProvider.ts # File-save security scan
│       ├── RefactorProvider.ts   # Refactor surface analysis
│       └── DiffInboxProvider.ts  # Pending diff approvals
```

### Control Panel Cards

| Card | Source | Features |
| :--- | :--- | :--- |
| **Git Status** | `/api/git-status` | Branch name, dirty/clean badge, staged/modified/untracked counts |
| **Plans** | `/api/plans` | Live progress bars per plan file, status chips |
| **Tasks** | `/api/tasks` | Grouped by project, inline completion indicators |
| **Swarm** | `/api/swarm` | Active agent tasks, status dots, inline Approve button for `awaiting_review` |
| **Quick Actions** | Extension host | `cargo build` and `cargo test` via VS Code terminal |

### Security Properties

- **TokenBridge proxy:** The session token never enters the Webview context — all API calls go through the extension host. XSS in the webview cannot exfiltrate the token.
- **Auto-spawn:** `DaemonManager` starts `aiosd` automatically if the socket isn't listening. Polls the token file and triggers sidebar refresh when ready.
- **Host validation:** All HTTP calls include the `Host: localhost` header, enforced by the Axum auth middleware.

### Install

The packaged `.vsix` is committed to the repo (`vscode-extension/raios-0.6.0.vsix`) so it can be installed directly without a Node toolchain:

```bash
code --install-extension vscode-extension/raios-0.6.0.vsix --force
```

To rebuild from source and reinstall, use the bundled script — it compiles, repackages, **uninstalls any existing `alazndy.raios` install first**, then installs the fresh `.vsix`. This guarantees only one version is ever registered, no matter how many times you re-run it:

```bash
cd vscode-extension && ./install.sh
```

Manual equivalent, if you need the individual steps:

```bash
cd vscode-extension
npm install        # pulls in typescript + @vscode/vsce devDependencies
npm run compile
npx vsce package
code --uninstall-extension alazndy.raios   # drop the old version first
code --install-extension raios-*.vsix
```

**Keyboard shortcuts:**

| Action | Windows / Linux | macOS |
| :--- | :--- | :--- |
| Security Scan | `Ctrl+Shift+R S` | `Cmd+Shift+R S` |
| Health Check | `Ctrl+Shift+R H` | `Cmd+Shift+R H` |
| Scan Current File | `Ctrl+Shift+R F` | `Cmd+Shift+R F` |

---

## 🚀 Quick Start

```bash
git clone https://github.com/alazndy/R-AI-OS.git
cd R-AI-OS
cargo install --path . --force
```

### Reinstall / Upgrade (Linux/macOS)

Use the bundled `install.sh` instead of running the steps above by hand. `cargo install --force` already replaces the binary at its single fixed path (`~/.cargo/bin/{raios,aiosd}`) — it physically cannot leave two copies behind — but a previously *running* `aiosd`/`raios-tray` process would otherwise keep serving the old code in memory until restarted. The script handles the full cycle:

```bash
./install.sh
```

What it does:
1. `cargo build --release`
2. `cargo install --path . --force` — replaces the existing binaries in place
3. Restarts `aiosd.service` and `raios-tray.service` via `systemctl --user` (if present) so the new binary actually takes effect, not just the file on disk
4. Warns if a stray `raios`/`aiosd` binary exists earlier on `$PATH` outside cargo's bin dir, which would silently shadow the freshly installed one

Start the daemon (powers the TUI, MCP server, and HTTP API):

```bash
aiosd
```

Tune background load in `~/.config/raios/config.toml` when needed, especially on Windows:

```toml
[daemon]
startup_bm25_indexing = true
startup_cortex_indexing = false
enable_health_worker = true
health_interval_secs = 900
git_interval_secs = 300
enable_sentinel_worker = false
sentinel_interval_secs = 300
enable_port_monitor = true
port_monitor_interval_secs = 30
port_probe_timeout_ms = 75
```

Windows defaults are now intentionally calmer: no eager Cortex indexing, no periodic Sentinel compile loop, slower health/git/port polling.

Launch the TUI:

```bash
raios
```

Bootstrap your AI factory (replicates 90+ agents and 180+ skills):

```bash
raios bootstrap
```

---

## 💻 CLI Reference

### Core Operations

| Command | Description |
| :--- | :--- |
| `raios health` | Portfolio health dashboard — scans all projects |
| `raios health <project>` | Single-project health scan |
| `raios usage` | Show local usage/quota signals across AI tools |
| `raios search "<query>"` | Semantic search across portfolio |
| `raios new "ProjectName"` | Scaffold a new project (follows MASTER rules) |
| `raios task "<description>"` | Route task to best agent |
| `raios handoff --to <agent> --status <SUCCESS\|FAILED\|BLOCKER> --msg "<text>"` | Atomic agent-to-agent handoff via the control plane |
| `raios trace record/search/forget` | Store, recall, and delete local tool/session trace memory |
| `raios trace kg-export [query]` | Export trace memory as MemPalace-compatible KG triple JSON |
| `raios evolve from-traces` | Generate pending instinct candidates from successful trace fixes |
| `raios bootstrap` | Replicate AI factory on a new machine |

### Security

| Command | Description |
| :--- | :--- |
| `raios verify-chain` | Verify audit log hash-chain integrity |
| `raios verify-chain -n <N>` | Show last N entries then verify |
| `raios security` | OWASP security scan |

### Agent Swarm

| Command | Description |
| :--- | :--- |
| `raios swarm start` | Start a parallel agent worktree |
| `raios swarm list` | List active swarm tasks |
| `raios swarm approve <id>` | Approve a pending swarm diff (merge branch) |

### Git Operations

| Command | Description |
| :--- | :--- |
| `raios git status` | Git status across portfolio |
| `raios git log` | Recent commits |
| `raios git commit` | Intelligent bulk commit |

### Build & Dev

| Command | Description |
| :--- | :--- |
| `raios build` | Build current project |
| `raios test` | Run test suite |
| `raios deps` | Dependency audit |
| `raios env` | Environment variable scan |

`raios usage` intentionally separates exact quota data from local auth metadata. If a provider does not expose remaining/reset counters locally, R-AI-OS prints `unknown` instead of guessing.

---

## 📁 Project Structure

```
src/
├── bin/
│   ├── raios.rs          # CLI entrypoint
│   └── aiosd.rs          # Daemon entrypoint
├── app/
│   └── events/           # Event handling (actions, keyboard, commands)
│       └── keyboard/     # Keyboard module (6 sub-modules)
├── cli/                  # CLI command implementations
├── core/
│   ├── build/            # Build logic (language-specific, 10 sub-modules)
│   └── deps/             # Dependency management (10 sub-modules)
├── cortex/               # Vector store, BM25 index, session memory
├── daemon/               # aiosd background daemon
├── intelligence/         # Agent routing, instinct engine, RBJ
├── mcp/                  # MCP server — policy-gated tool call handler
├── search/               # Neural search (BM25 + vector hybrid)
├── security/             # Security Kernel (sandbox, policy, chain, egress)
├── sentinel/             # Redaction engine, Sentry integration
├── server/               # HTTP/WebSocket server (Axum, :42071)
├── swarm/                # Parallel worktree agent management + SQLite store
└── ui/
    └── panels/           # TUI panels (14 modules — dashboard, security, inbox, etc.)

vscode-extension/
├── src/
│   ├── extension.ts      # Extension activation
│   ├── ipc/              # DaemonClient, TokenBridge, DaemonManager
│   ├── providers/        # Sidebar, StatusBar, Diagnostics, Refactor, Diffs
│   ├── commands/         # CommandBridge
│   └── bridge/           # JumpToCode
├── icon.svg              # Master logo (512×512, source of truth)
├── icon.png              # Extension marketplace icon (512×512)
└── icon128.png           # Extension sidebar icon (128×128)
```

---

## 🗺️ Roadmap

- [x] **Phase 1–7:** Core TUI, workspace mapping, health dashboard, BM25 search
- [x] **Phase 8:** Universal Kernel — Tri-protocol, Lock Manager, Radar Whispers, Factory Mode
- [x] **Phase 9:** Refactor & Modularization — all large files split into focused modules
- [x] **Phase 10:** Hardened Kernel Alpha — Sentry, Redaction Engine, Audit Ledger
- [x] **Phase 10B:** Security Kernel (Phases 1–4) — Sandbox + Policy + Audit Chain + Egress
- [x] **Phase IDE:** VS Code Extension — Sidebar WebView + TokenBridge + DaemonManager + Refactor Tree
- [x] **Phase IDE v0.5:** Sidebar v2 — Git Status card, Swarm card with Approve, Quick Actions
- [x] **Phase 11:** Tool Pinning & Drift Detection — SHA-256 manifest pin, `-32028` on mismatch, `raios pin-reset / pin-status`
- [x] **Phase 12:** Secret Leasing — `raios secret grant/list/revoke <tool> <ENV_VAR>` with TTL-based auto-revoke
- [x] **Phase 13:** Rate Limiting — Fixed-window counter per tool, `-32029` on exceed, `raios rate-status`
- [x] **Phase 14:** Quarantine Mode — Pattern-matched quarantine queue, `-32027` on block, `raios quarantine list/approve/deny`
- [x] **Phase 15:** Write-Back Bridge — Sidebar checkboxes interactive, `raios task-update` CLI syncs back to `memory.md`
- [x] **Phase 16:** Lifecycle Worker — git-activity-based auto active/beklemede/archived transitions (`src/daemon/lifecycle.rs`)
- [x] **Phase 17:** 4-Agent Matrix & Atomic Handoff — Gemini CLI retired; Claude/Codex/OpenCode/Antigravity (`agy`) as canonical identities; `raios handoff` on the control plane with real per-CLI prompt injection, secret scanning, diff-stat attachment, and stale-handoff supersede; new TUI **Inbox** panel
- [x] **Phase 18:** `aiosd` systemd user service auto-start on login — `aiosd.service` enabled via `systemctl --user enable aiosd`, `WantedBy=default.target`
- [x] **Phase 19:** Cortex Real Embeddings — `default = ["cortex"]`, fastembed all-MiniLM-L6-v2, adaptive CPU throttling in embed_batch
- [x] **Phase 20:** Autonomous Scheduler — `raios cron add/list/remove/pause/resume/run`, `cp_scheduled_jobs` control-plane table, atomic claim worker, `spawn_agent_detached` helper
- [x] **Phase 21:** Trace Memory — `raios trace`, handoff/runtime recall, session-review auto trace, trace-to-evolution candidates, and MemPalace KG export

---

**R-AI-OS is the bridge between human creativity and autonomous execution.**
