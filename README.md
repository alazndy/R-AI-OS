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
  ║ ╔╝   · · ·   R - A I - O S   KERNEL  v3.8   ╚╗ ║
  ╚═╝ ══════════════════════════════════════════ ╚═╝
</pre>
</p>

<p align="center">
  <strong>A Hardened, LLM-Native OS Kernel for Autonomous Agent Swarms</strong>
</p>

<p align="center">
  <a href="https://github.com/alazndy/r-ai-os/releases"><img src="https://img.shields.io/badge/version-v3.9.0-blue?style=for-the-badge" alt="Version"></a>
  <a href="https://rust-lang.org"><img src="https://img.shields.io/badge/Built%20with-Rust-orange?style=for-the-badge&logo=rust" alt="Rust"></a>
  <a href="https://github.com/alazndy/r-ai-os/blob/master/LICENSE"><img src="https://img.shields.io/github/license/alazndy/r-ai-os?style=for-the-badge" alt="License"></a>
  <a href="#-security-kernel"><img src="https://img.shields.io/badge/Security-Hardened-green?style=for-the-badge" alt="Security"></a>
  <a href="#-vs-code-extension"><img src="https://img.shields.io/badge/VS%20Code-v0.9.0-blueviolet?style=for-the-badge&logo=visualstudiocode" alt="VS Code"></a>
</p>

<p align="center">
  <a href="#-the-vision">Vision</a> •
  <a href="#-security-kernel">Security</a> •
  <a href="#-tri-protocol-interface">Protocols</a> •
  <a href="#-core-modules">Modules</a> •
  <a href="#-vs-code-extension">VS Code</a> •
  <a href="#%EF%B8%8F-system-tray-raios-tray">System Tray</a> •
  <a href="#-quick-start">Quick Start</a> •
  <a href="#-cli-reference">CLI</a> •
  <a href="#-roadmap">Roadmap</a>
</p>

---

## 🔭 The Vision

R-AI-OS is not a CLI tool — it is a **Kernel**. While traditional operating systems manage hardware, R-AI-OS manages the **AI layer**: a decentralized swarm of autonomous specialists running across Claude Code, Codex CLI, OpenCode, Antigravity (`agy`), and any MCP-compatible agent. `raios new` scaffolds a project; `raios bootstrap` provisions whatever global tools, Claude Code marketplaces/plugins, and rule-sync repos you've explicitly listed under `[bootstrap]` in `config.toml` — see [CLI Reference](#-cli-reference) for what's actually installed on your machine.

It solves the fundamental problem of **unsupervised agent execution**: agents that run unchecked can leak secrets, corrupt files, and make unauthorized network calls. R-AI-OS sits between the human and the swarm as a hardened control plane — enforcing policies, auditing every action, and managing context economics.

```
Human → [ R-AI-OS Kernel ] → Agent Swarm (Claude / Codex / OpenCode / AGY / MCP)
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
crates/raios-core/src/security/
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
default_action = "confirm"

[[tools.rules]]
name = "list_projects"
action = "allow"

[[tools.rules]]
name = "run_build"
action = "deny"
```

Matching is by exact tool `name`, not by a path glob — per-path filesystem access is a separate concept (`[tools.rules.capabilities]`, declarative today, see `security::capabilities`), not a rule action.

### Phase 3 — Audit Chain

Every allow/deny decision is written to a tamper-evident, SHA-256 hash-chained SQLite ledger. Each entry links to the previous entry's hash — any tampering is immediately detectable.

Concurrent writers are serialized at the predecessor-read boundary: standalone appends acquire an immediate SQLite write transaction before reading the current tail, while audit rows participating in a larger domain transaction remain atomic with that transaction. Two connections therefore cannot legitimately create separate children of the same predecessor.

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
| `GET` | `/api/notifications/important?client_id=<id>` | Newly observed important events plus the server cursor timestamp |
| `GET` | `/api/notifications/digest?client_id=<id>` | Interval-gated summary of routine background activity |
| `GET` | `/api/plans` | Plans from `docs/superpowers/plans/*.md` with checkbox progress |
| `GET` | `/api/git-status?path=<dir>` | Git branch + dirty/staged/modified/untracked for a workspace |
| `GET` | `/api/swarm` | Active (non-terminal) swarm tasks |
| `POST` | `/api/approve` | Approve a swarm task (merge branch) or pending diff (write file) |
| `GET` | `/api/stream` | WebSocket — real-time kernel event stream |

Notification client IDs must be 1–128 ASCII characters from `A-Z`, `a-z`, `0-9`, `-`, `_`, `.`, and `:`. The tray persists a per-install UUID in `~/.config/raios/notification-client-id`. Delivery cursors are stored server-side and advance by monotonic event ID, so distinct events sharing the same timestamp are not dropped. Repeated High/Critical security findings are emitted once while active and become eligible again only after they resolve and reappear.

---

## 🧠 Core Modules

### 📉 Cortex — Token Budgeter & Context Manager

- **Sigmap:** Up to 97% token reduction via high-density signature mapping (`SIGMAP.md`)
- **BM25 persistence:** Index survives restarts via mtime-based invalidation
- **Trigram grep:** Exact/regex search via shared SQLite trigram postings, with regex verification and full-scan fallback for ambiguous patterns
- **Vector store:** Binary SQLite BLOBs — transaction-safe, no JSON drift
- **Session memory:** Per-agent `memory.md` auto-append

### 🎯 Unified Agent Router

Maps natural-language task descriptions to the right specialist using local BM25 + vector hybrid indexing — entirely local, no external routing dependency. `[bootstrap]` config can optionally point at external rule-sync repos (e.g. community skill/rule packs) to seed a workspace, but that's an opt-in setup step, not something the router itself bridges or depends on.

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

For evidence-rich handoffs, use a JSON report instead of `--msg` (the two flags are mutually exclusive):

```bash
raios handoff --to codex-kaira --status success --report handoff-report.json
```

The report carries `findings`, `evidence`, `edge_cases_considered`, `open_questions`, `confidence`, and `what_i_did_not_check`. It is stored in the existing handoff artifact metadata, rendered by the Inbox panel, and delivered with the normal handoff context; legacy `--msg` handoffs remain supported.

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

### 🪶 ANKA — Historical Transcript Recall

ANKA (*Agent Narrative Knowledge Archive*) searches local coding-agent history
through a separate, rebuildable cache; it never writes raw transcripts into
`workspace.db` or curated memory.

```bash
raios anka status
raios anka index --harness codex
raios anka search "JWT refresh rotation" --project R-AI-OS
raios anka blame crates/raios-core/src/db/mem.rs
raios anka forget <record-id>
```

- Only local history is indexed; recognized secret-shaped values are redacted before cache writes.
- Cache records are owner-only, support project exclusions and local tombstones, and can be rebuilt from sources.
- No automatic context injection, synchronization, sharing, or curated-memory promotion occurs.
- MCP exposes only read-only `anka_recall`, capped at eight results and framed as untrusted historical evidence.

See [`docs/ANKA.md`](docs/ANKA.md) for cache and authority boundaries.

### ⏳ Lifecycle Worker

Background daemon task (`crates/raios-runtime/src/daemon/lifecycle.rs`) that keeps project status honest without manual upkeep. Every `lifecycle_interval_secs`, it checks each tracked project's last commit time and transitions status automatically:

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

## 🖥️ VS Code Extension (v0.9.0)

R-AI-OS ships a native VS Code extension that turns the IDE into a **Hybrid UI** — the control panel for your agent swarm directly in your sidebar.

```
vscode-extension/
├── src/
│   ├── extension.ts              # Activation + provider wiring
│   ├── ipc/
│   │   ├── DaemonClient.ts       # TCP :42069 connection
│   │   ├── TokenBridge.ts        # Session token proxy (XSS-safe)
│   │   └── DaemonManager.ts      # Systemd-first startup, authenticated readiness
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
- **Daemon startup:** on Linux, `DaemonManager` asks the systemd user service to start `aiosd` and waits for an authenticated TCP handshake. Direct detached spawning is a non-systemd fallback, preventing duplicate daemons during desktop login.
- **Host validation:** All HTTP calls include the `Host: localhost` header, enforced by the Axum auth middleware.

### Install

No `.vsix` is committed to the repo. Every [GitHub Release](https://github.com/alazndy/R-AI-OS/releases/latest) attaches a prebuilt `raios-<version>.vsix` as a release asset — download it and install directly, no Node toolchain required:

```bash
code --install-extension raios-0.9.0.vsix --force
```

To build from source instead, use the bundled script — it compiles, repackages, **uninstalls any existing `alazndy.raios` install first**, then installs the fresh `.vsix`. This guarantees only one version is ever registered, no matter how many times you re-run it:

```bash
cd vscode-extension && ./install.sh
```

Manual equivalent, if you need the individual steps:

```bash
cd vscode-extension
 pnpm install        # pulls in typescript + @vscode/vsce devDependencies
 pnpm run compile
 pnpm run package
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

## 🖥️ System Tray (`raios-tray`)

R-AI-OS provides a desktop system tray application built with PySide6 (`tools/raios-tray/raios-tray.py`) for persistent status monitoring and quick agent orchestration. See [tools/raios-tray/README.md](tools/raios-tray/README.md) for setup and configuration details.

```
tools/raios-tray/
├── raios-tray.py           # PySide6 system tray application
├── raios-tray.service      # User systemd service template
├── requirements.txt        # Runtime dependencies
└── requirements.lock.txt   # Pinned dependency lockfile
```

### Features

- **Daemon Health Polling**: Polls `aiosd`'s `/api/health` on an interval; the tray icon switches to an urgent-update glyph and shows a dirty-project count in its title whenever any managed project has uncommitted changes.
- **Project Quick Launcher**: Open a terminal running an agent (`claude`/`codex`/`opencode`/`agy`) in a project's directory, or launch VS Code — auto-detects the first available terminal emulator (`ptyxis`, `gnome-terminal`, `konsole`, `xfce4-terminal`, or `x-terminal-emulator` on Linux; `Terminal.app` on macOS; PowerShell on Windows).
- **Project Manager**: Add, edit, remove, and pin tracked projects from a dedicated dialog; pinned projects surface at the top of the tray menu.
- **Memory & Task Panels**: Shows recent `mem_items` and open tasks from the workspace database directly in the tray UI.
- **Systemd User Integration**: Managed as a user service via `raios-tray.service` pointing to its canonical virtual environment (`tools/raios-tray/.venv/bin/python`). The unit is enabled under `graphical-session.target`, preventing Qt from starting before the Wayland/X11 session exists and stopping the tray before the display disappears.
- **Structured Debug Logging**: Diagnostics written to `~/.config/raios/tray.log`.

---

## 🏭 Product Factory Visual Control Studio (`raios-factory-ui`)

R-AI-OS provides a modern, interactive React web application (`tools/raios-factory-ui/`) for visualizing, auditing, and orchestrating the Product Factory across all 10 lifecycle phases (Phases 0–9). See [tools/raios-factory-ui/README.md](tools/raios-factory-ui/README.md) for detailed setup and usage instructions.

```
tools/raios-factory-ui/
├── src/
│   ├── components/
│   │   ├── PipelineFlow.jsx          # 10-Phase Pipeline flowchart & inspector
│   │   ├── IntakeCharterStudio.jsx   # Product intake & Markdown Charter editor
│   │   ├── ChangeControlGraph.jsx    # CR & AI Impact Assessment network graph
│   │   ├── CycleExecutionMatrix.jsx  # Stage task graph DAG & SHA-256 evidence
│   │   ├── QualityReleaseGate.jsx    # Closed-testing quality checklist & signoff
│   │   ├── SupportTriageDesk.jsx     # Support ticket triage & CR linking
│   │   └── CommandTerminal.jsx       # Interactive IPC terminal & command log
│   ├── App.jsx                       # Main workspace switcher & state engine
│   └── mockData.js                   # Product Factory domain projection data
```

### Features

- **10-Phase Pipeline Map**: Interactive 10-step lifecycle flowchart with security invariant validation.
- **Intake & Charter Studio**: Discovery questionnaire runner, versioned Charter editor, and requirement matrix.
- **Change Control Visualizer**: Network topology graph connecting Change Requests to affected requirements, code modules, and security risk levels.
- **Execution & Evidence Matrix**: Real-time cycle status controls (`Pause`, `Resume`, `Cancel`), stage task DAG, and content-addressed SHA-256 evidence inspector.
- **Quality & Release Sign-off Gate**: Closed-testing checklist (React Native Expo check, TypeScript clean, Rust 50% coverage floor), release blockers counter, and release approval workflow.
- **Interactive Control-Plane Terminal**: Collapsible live terminal logging typed CLI commands and daemon JSON payload contracts in real time.

---

## 🚀 Quick Start

### Fastest: install a release binary (Linux/macOS, x86_64)

```bash
curl -fsSL https://raw.githubusercontent.com/alazndy/R-AI-OS/master/scripts/get-raios.sh | sh
```

Downloads the latest signed release from [GitHub Releases](https://github.com/alazndy/R-AI-OS/releases), verifies its sha256 checksum, and installs `raios`/`aiosd` to `~/.local/bin` (override with `RAIOS_INSTALL_DIR`). Pin a specific version with `RAIOS_VERSION=v3.9.0 curl ... | sh`. No arm64/Apple Silicon build yet — Apple Silicon Macs and ARM Linux need the build-from-source path below.

### From source (any platform/architecture)

```bash
git clone https://github.com/alazndy/R-AI-OS.git
cd R-AI-OS
./install.sh
```

### Windows 10/11 (portable local install)

The core system runs natively on Windows: `raios.exe` is the CLI/TUI and
`aiosd.exe` is the background daemon. From PowerShell in the cloned repository:

```powershell
Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
Set-Location .\R-AI-OS
.\install-system.ps1
```

The installer builds locked release binaries, places them under
`%LOCALAPPDATA%\R-AI-OS\bin`, adds that directory to the user `PATH`, writes
config/token/policy under `%APPDATA%\raios`, and registers the daemon as the
per-user `RAIOS_Daemon` Scheduled Task. Start a new PowerShell afterwards:

```powershell
raios hub status
raios
```

Useful switches are `-SkipBuild`, `-NoScheduledTask`, `-NoPath`,
`-InstallRoot <path>`, and `-WorkspaceRoot <path>`. The bundled agent wrapper
uses the PowerShell profile (`$PROFILE`); external agent CLIs such as Claude,
Codex, OpenCode, and `agy` remain separate prerequisites.

### Reinstall / Upgrade (Linux/macOS)

Use the bundled `install.sh` instead of running the steps above by hand. It installs into the active `raios` PATH directory when one already exists, otherwise it uses Cargo's default bin directory (`~/.cargo/bin/{raios,aiosd}`). The install physically replaces the binaries in place, but a previously *running* `aiosd`/`raios-tray` process would otherwise keep serving the old code in memory until restarted. The script handles the full cycle:

```bash
./install.sh
```

What it does:
1. `cargo build --release --workspace --locked`
2. `cargo install --path crates/raios-surface-cli --locked --force` — replaces the existing binaries in the active install directory
3. Restarts `aiosd.service` and `raios-tray.service` via `systemctl --user` (if present) so the new binary actually takes effect, not just the file on disk
4. Warns if a stray `raios`/`aiosd` binary exists earlier on `$PATH` outside cargo's bin dir, which would silently shadow the freshly installed one

Start the daemon (powers the TUI, MCP server, and HTTP API):

```bash
aiosd
```

Tune background load in `~/.config/raios/config.toml` when needed, especially on Windows:

### Agent Wrapper memory isolation

Wrapper memory imports only transcripts that prove their project or workspace
scope. Claude project transcripts and AGY records carrying the active workspace
are eligible. Codex and OpenCode history formats are currently global and do
not carry enough project identity, so R-AI-OS skips their automatic history
import rather than risk cross-project fact contamination. For those agents
only, one explicit positional launch prompt (for example, `raios run codex
"use SQLite"`) may be captured after a successful session with the local
project path and wrapper run ID; flags, multi-argument invocations, oversized
input, and secret-like content are never captured. Interactive follow-up turns
remain outside automatic capture until an upstream project/session identity is
available. A hook or an agent can record an explicit follow-up safely with
`raios wrapper-note "decision or progress"`: the command is available only to
a live `raios run` child through its opaque `RAIOS_WRAPPER_RUN_ID`, verifies
that its current directory resolves to the run's registered project, limits
content to 500 characters, and rejects secret-like input before it reaches the
database. Accepted notes become wrapper events and pass through the same
project-bound L0→L3 memory pipeline.

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

The dashboard uses four top-level tabs (`NOW`, `WORK`, `EXPLORE`, and `GOVERN`).
Its shared terminal identity uses the supplied circular ASCII mark: hot orange
(`#FF8600`) around an electric-blue (`#00ACFF`) core on a near-black navy
canvas. The responsive six-row mark is rendered from one shared TUI brand
module in both the dashboard header and the recent-projects surface.
Use `1`–`4` or `Tab`/`Shift+Tab` from the keyboard, or click a tab with the
mouse. `Up`/`Down` stays within the focused route list, while `Left`/`Right`
switches the focused panel. Clicking route content selects an item; the mouse
wheel navigates the current route. Click the bottom Command Center bar (or
press `/`) to open the command palette. Safety-sensitive actions remain
explicit keyboard commands with the existing approval and audit flow.

`EXPLORE` has a direct read-only workspace search: press `/`, type a query,
then press `Enter`. Results arrive from the daemon index, remain selectable by
keyboard or mouse, and `Enter` opens a selected result only when its canonical
path is inside the local configured workspace. Remote result paths are never
opened locally. Queries are bounded to 240 characters, reject secret-like
content, and are sent as serialized JSON rather than interpolated protocol
text.

`GOVERN` exposes selected scheduler controls only through the existing audited
control-plane commands: focus a cron job, press `r` to schedule one immediate
run, or `p` to pause/resume it. The same two visible actions are clickable with
the mouse. Their idempotency keys include the current snapshot sequence, so a
double action in one view is deduplicated while a later refreshed view can
legitimately schedule another run.

`NOW` is an operational console: it combines the approval/blocker attention
queue, the selected project's live posture, and context-derived next actions.
Use `Space` to rotate among those three areas, `Up`/`Down` to select, and
`Enter` to run the selected read-only navigation or snapshot-refresh action.
`a` and `r` still resolve only the selected approval through the existing
owner-checked, idempotent control-plane command path; the console never opens
a direct shell or bypasses server-side authorization. `WORK` lists every
registered project with lifecycle status, Git state, and `memory.md`
availability; selecting a project (or one of its tasks) keeps it selected and
shows a bounded `memory.md` preview with the latest known project status. If
an older local daemon has not yet been restarted, the TUI safely fills a
missing preview from a project path only after confirming that it is inside the
configured workspace root.

For a registered selected project, `NOW` also offers **Launch Codex session**.
Choose it and press `Enter` (or select it by mouse, then press `Enter`) to open
an interactive terminal running the tracked `raios run codex` wrapper. The
server accepts only registered project paths and allowlisted agent identities,
keeps command idempotency and audit logging, rejects prompt-shaped CLI flags or
secret-like content, and launches the wrapper with direct argv boundaries — not
a client-assembled shell command.

`WORK` can now create canonical personal tasks for the selected project: press
`n`, type a title, adjust priority with `+`/`-`, then press `Enter`. Select a
TUI-managed task and use `i`, `b`, or `c` to mark it in progress, blocked, or
completed. The daemon validates title/path bounds, rejects secret-like titles,
stores the task and project association atomically, and refuses status changes
to agent, handoff, swarm, Factory, and legacy-Markdown task records.

The same `WORK` flow is mouse-aware: click a project or task using the visible
panels, then use the `Task Actions` strip for `New`, `In Progress`, `Block`, or
`Complete`. The composer exposes explicit `-`, `+`, `Cancel`, and `Create`
buttons; text input remains keyboard-based. Rendering and hit testing share one
layout model, so the factory summary above the task list cannot offset task-row
selection.

The command palette also opens dedicated full-screen views for local markdown
tasks (`/tasks`), Constitution editing (`/rules`), extensions (`/ext`), local
index search (`/search`), active daemon agents (`/logs`), and the local
activity timeline (`/timeline`). `/memory` is an alias for the full MemPalace
view. In `WORK`, Left/Right moves between Projects, the read-only Ocak summary,
and Tasks; press `Enter` on an Ocak summary line to draft the relevant `/ocak`
command in the palette. It never submits a Factory command automatically.

Provision a machine from your own `[bootstrap]` config: `raios bootstrap` prints a plan of whatever global npm tools, Claude Code marketplaces/plugins, rule-sync repos, and plugin-enables you've listed under `[bootstrap]` in `~/.config/raios/config.toml`, then asks for confirmation before running anything. With no `[bootstrap]` section configured it prints an empty plan and exits — a safe no-op, not an automatic sync of any external agent ecosystem. Re-running against an already-provisioned machine is idempotent: steps that are already done (marketplace already added, plugin already installed/enabled) are reported and skipped rather than treated as failures.

```bash
raios bootstrap             # prints the plan, asks for confirmation, then executes it
raios bootstrap --dry-run   # prints the plan only, never prompts or executes
raios bootstrap --yes       # skips the confirmation prompt (also: -y)
```

See [`[bootstrap]` Configuration](#bootstrap-configuration) below for the full config schema.

### Bootstrap Configuration

Nothing under `[bootstrap]` runs unless you put it there — this section is entirely opt-in and empty by default. Add it to `~/.config/raios/config.toml`:

```toml
[bootstrap]
# Global npm packages to install if missing (skipped if already on PATH).
global_npm_tools = ["sigmap", "ctx7"]

# Plugin names to enable from the official Claude Code marketplace.
enable_claude_plugins = ["github@claude-plugins-official"]

# Claude Code plugin marketplaces to add, each with its own plugins to install.
[[bootstrap.claude_marketplaces]]
url = "https://github.com/example/repo.git"
plugins = ["plugin@marketplace"]

# Git repos whose rules/ directory gets synced into local agent rule dirs.
[[bootstrap.rule_sync_repos]]
git_url = "https://github.com/example/rules.git"
targets = ["~/.claude/rules", "~/.antigravity/rules"]
```

`global_npm_tools` and `enable_claude_plugins` are plain string lists; `claude_marketplaces` and `rule_sync_repos` are each a list of tables (`[[bootstrap.claude_marketplaces]]` / `[[bootstrap.rule_sync_repos]]`, repeatable for multiple entries). `targets` paths support a leading `~/` for the current user's home directory. Field names come straight from `BootstrapConfig`/`ClaudeMarketplace`/`RuleSyncRepo` in `crates/raios-core/src/config.rs` — that's the source of truth if this drifts.

---

## 💻 CLI Reference

### Core Operations

| Command | Description |
| :--- | :--- |
| `raios health` | Portfolio health dashboard — scans all projects |
| `raios health <project>` | Single-project health scan |
| `raios db check [--full]` | Read-only SQLite quick check (or exhaustive integrity check) plus DB/WAL/free-page/snapshot metrics |
| `raios db backup [--keep 1..=10]` | Create an online, SHA-256-recorded, integrity-checked private snapshot; retains 3 by default |
| `raios db checkpoint [--truncate]` | Checkpoint committed WAL frames; passive by default, optionally truncating the WAL file |
| `raios usage` | Show local usage/quota signals across AI tools |
| `raios search "<query>"` | Semantic search across portfolio |
| `raios locate "<pattern>" [--dir <path>] [-i] [--reindex]` | Exhaustive exact/regex search over the trigram index (grep-equivalent) |
| `raios new "ProjectName"` | Scaffold a new project (follows MASTER rules); also syncs the new project's note into the Obsidian vault at `~/Obsidian` by default (unless `--no-vault`) |
| `raios obsidian-sync [--vault <path>] [--dry-run]` | Regenerate the Obsidian vault (`~/Obsidian` by default) from current raios project data — one note per project, a `<category>-MOC.md` per category, and a root `Proje Atlası.md` index. Every run fully overwrites project notes/MOCs/index; manual edits made directly inside a vault note are lost on the next sync |
| `raios task "<description>"` | Route task to best agent |
| `raios handoff --to <agent> --status <SUCCESS\|FAILED\|BLOCKER> --msg "<text>"` | Atomic agent-to-agent handoff via the control plane |
| `raios trace record/search/forget` | Store, recall, and delete local tool/session trace memory |
| `raios trace kg-export [query]` | Export trace memory as MemPalace-compatible KG triple JSON |
| `raios mem export-portable [output.json]` | Export all structured memory items as a portable JSON snapshot; refuses secret-like content |
| `raios mem import-portable [input.json]` | Atomically merge a portable memory snapshot; rejects secret-like content and inputs over 16 MiB or 50,000 items |
| `raios evolve from-traces` | Generate pending instinct candidates from successful trace fixes |
| `raios ocak overview [--json]` (alias: `factory`) | Read the canonical Ocak (Product Factory) snapshot without changing state |
| `raios ocak execute --file <command.json> [--json]` (alias: `factory`) | Dispatch one bounded local typed `FactoryCommand`; human-only approval, cancellation, stage activation/completion, requirement application, and release approval commands are rejected |
| `raios bootstrap [--dry-run] [--yes\|-y]` | Print (and, after confirmation, run) the plan from `[bootstrap]` in `config.toml` — empty/no-op unless configured |

Existing Git repositories can be attached through `FactoryCommand::AttachExistingProject`. The command accepts only an absolute repository root, verifies the Git worktree, `origin` remote, and `HEAD` SHA before persisting the owner-bound product source. HTTP(S) remotes containing embedded credentials are rejected.

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

Database snapshots and the operator-controlled offline restore procedure are
documented in [`docs/DATABASE_RECOVERY.md`](docs/DATABASE_RECOVERY.md).

---

## 📁 Project Structure

R-AI-OS is a Cargo workspace of 6 crates — there is no monolithic `src/` at the repo root:

```
crates/
├── raios-contracts/      # Shared types/DTOs used across every other crate
├── raios-core/           # Config, DB layer, Security Kernel (sandbox, policy, chain, egress)
├── raios-runtime/        # Daemon, intelligence/routing, cortex (BM25+vector), swarm, server (Axum),
│                         # sentinel (redaction), session_memory, bootstrap, agent_runner
├── raios-surface-cli/    # `raios` CLI — one module per subcommand (cli/*.rs)
├── raios-surface-mcp/    # MCP server — policy-gated tool call handler
└── raios-surface-tui/    # TUI — panels (dashboard, security, inbox, etc.) + setup wizard

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
- [x] **Ocak Phase 9 (Product Factory):** Local impact approval, immutable requirement revisions, and approved-plan lifecycle-cycle materialization (no autonomous execution)
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
- [x] **Phase 16:** Lifecycle Worker — git-activity-based auto active/beklemede/archived transitions (`crates/raios-runtime/src/daemon/lifecycle.rs`)
- [x] **Phase 17:** 4-Agent Matrix & Atomic Handoff — Gemini CLI retired; Claude/Codex/OpenCode/Antigravity (`agy`) as canonical identities; `raios handoff` on the control plane with real per-CLI prompt injection, secret scanning, diff-stat attachment, and stale-handoff supersede; new TUI **Inbox** panel
- [x] **Phase 18:** `aiosd` systemd user service auto-start on login — `aiosd.service` enabled via `systemctl --user enable aiosd`, `WantedBy=default.target`
- [x] **Phase 19:** Cortex Real Embeddings — `default = ["cortex"]`, fastembed all-MiniLM-L6-v2, adaptive CPU throttling in embed_batch
- [x] **Phase 20:** Autonomous Scheduler — `raios cron add/list/remove/pause/resume/run`, `cp_scheduled_jobs` control-plane table, atomic claim worker, detached spawn helpers, and a process-wide child reaper that consumes every scheduled child exit status
- [x] **Phase 21:** Trace Memory — `raios trace`, handoff/runtime recall, session-review auto trace, trace-to-evolution candidates, and MemPalace KG export
- [x] **Phase 22:** Layered Memory & Lineage (L0→L3) — `mem_nodes`/`mem_lineage` give `mem_items` real traceability; replace-and-archive `mem_upsert` (fixes unbounded body growth); deterministic L1 atomic facts, L2 daily scenes, L3 persona; `raios mem history`/`--layer`; Mermaid `raios sessions --canvas`
- [x] **Phase 23:** Operational Hardening — pattern-scan self-disclosure in `raios security`/`raios refactor`; `sigmap` config drift fix; `session_memory.rs` split into a focused module; `raios usage` reads live Claude Pro/Max quota via a statusLine cache bridge
- [x] **Phase 24:** Trigram Locate (renamed 2026-07-11 from `raios grep`) — `raios locate` + MCP `locate_search`: trigram-indexed, exhaustive exact/regex search at 0.015s warm with proven `grep -rn` parity; conservative literal extraction with full-scan fallback
- [x] **Phase 25:** Resident Cortex — long-lived model+HNSW worker inside `aiosd` (mpsc/oneshot, lazy dirty rebuilds); `raios search` delegates via TCP with silent in-process fallback — semantic search ~1.0s warm (was ~4-6s)
- [x] **Phase 26:** MCP Parity + Dart/Flutter — MCP `semantic_search` now delegates to the resident Cortex daemon too (was >60s in-process per call, now ~1s warm); Dart/Flutter ecosystem support; stale-worktree duplicate-match fix; `tool_pin` re-verified and re-pinned
- [x] **Phase 27:** Product Factory — Local lifecycle control now has two explicit modes: `quick` requires only problem, core outcome, and success metric before a compact Charter; `governed` retains the full five-question intake and every lifecycle control. Neither mode bypasses human plan/release approval, stage approval, ownership checks, audit logging, or external-distribution protections. Agents can set the mode through the same typed, idempotent, audited local MCP/TUI service, now exposed on the CLI as `raios ocak` (alias `factory`); no public HTTP write route, automatic executor, external integration, or store action is enabled.
- [x] **Phase 28:** ANKA (Agent Narrative Knowledge Archive) — read-only, redacted, rebuildable transcript recall (`raios anka status/index/search/blame/forget`), MCP `anka_recall` exposed as untrusted historical evidence only.
- [x] **Phase 29:** Repository productization pass — `LICENSE` (AGPL-3.0), `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, and GitHub issue templates added; scheduled-job retry-storm bug fixed (failed spawns now back off instead of retrying every scheduler tick).

---

**R-AI-OS is the bridge between human creativity and autonomous execution.**
