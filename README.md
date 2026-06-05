# ⚡ R-AI-OS: The AI OS Kernel

<p align="center">
  <img src="https://raw.githubusercontent.com/alazndy/r-ai-os/master/assets/raios-logo.png" width="220" alt="R-AI-OS Logo">
</p>

<p align="center">
  <strong>A Hardened, LLM-Native OS Kernel for Autonomous Agent Swarms</strong>
</p>

<p align="center">
  <a href="https://github.com/alazndy/r-ai-os/releases"><img src="https://img.shields.io/badge/version-v2.0.0--alpha-blue?style=for-the-badge" alt="Version"></a>
  <a href="https://rust-lang.org"><img src="https://img.shields.io/badge/Built%20with-Rust-orange?style=for-the-badge&logo=rust" alt="Rust"></a>
  <a href="https://github.com/alazndy/r-ai-os/blob/master/LICENSE"><img src="https://img.shields.io/github/license/alazndy/r-ai-os?style=for-the-badge" alt="License"></a>
  <a href="https://owasp.org/www-project-top-ten/"><img src="https://img.shields.io/badge/Security-Hardened-green?style=for-the-badge" alt="Security"></a>
</p>

<p align="center">
  <a href="#-the-vision">Vision</a> •
  <a href="#-security-kernel">Security Kernel</a> •
  <a href="#-core-modules">Core Modules</a> •
  <a href="#-vs-code-extension">VS Code Extension</a> •
  <a href="#-quick-start">Quick Start</a> •
  <a href="#-cli-reference">CLI</a> •
  <a href="#-roadmap">Roadmap</a>
</p>

---

## 🔭 The Vision

R-AI-OS is not just a CLI tool — it is a **Kernel**. While traditional operating systems manage hardware, R-AI-OS manages the **AI layer**: a decentralized swarm of 90+ autonomous specialists running across Claude Code, Gemini CLI, and any MCP-compatible agent.

It solves the fundamental problem of **unsupervised agent execution**: agents that run unchecked can leak secrets, corrupt files, and make network calls they shouldn't. R-AI-OS sits between the human and the swarm as a hardened control plane — enforcing policies, auditing every action, and managing context economics.

```
Human → [ R-AI-OS Kernel ] → Agent Swarm (Claude / Gemini / MCP)
              ↓
    [Security Kernel] [Context Manager] [Swarm Mesh] [Hybrid UI]
```

---

## 🛡️ Security Kernel (v2.0.0-alpha)

The Security Kernel is the core of R-AI-OS. It enforces a **zero-trust model** for all agent tool calls: every action is policy-gated, logged, and auditable. All 4 phases are complete and tested (239/239 tests green).

### Architecture

```
src/security/
├── sandbox.rs       # Filesystem Jail — canonicalize + boundary enforcement
├── policy.rs        # Policy Manager — TOML-based allow/deny/confirm engine
├── verify_chain.rs  # Audit Chain — SHA-256 hash-chained SQLite ledger
└── egress.rs        # Egress Filter — domain allowlist/blocklist, fail-closed
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
Every MCP tool call passes through a policy gate before execution. Rules are defined in `raios-policy.toml` and evaluated in order.

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

Fail-closed by design: `confirm` rules in daemon/stdio mode deny by default — no interactive prompt means no silent execution.

### Phase 3 — Audit Chain
Every allow/deny decision is written to a tamper-evident, SHA-256 hash-chained SQLite ledger. Each entry links to the previous entry's hash — any tampering is immediately detectable.

```bash
raios verify-chain          # verify full chain integrity
raios verify-chain -n 50    # show last 50 entries then verify
```

### Phase 4 — Egress Filter
Domain-level allowlist/blocklist for any HTTP/HTTPS calls made via MCP tools. Fail-closed: unrecognized domains are denied unless explicitly allowed.

```toml
[egress]
mode = "allowlist"
allowed = ["api.anthropic.com", "api.openai.com", "*.github.com"]
```

### Redaction Engine
Automatically masks sensitive values (API keys, GCP secrets, PII patterns) before they appear in logs or are forwarded to Sentry. Built on `regex` with 20+ detection patterns.

### Sentry Observability
Production-grade error tracking with contextual breadcrumbs and automatic panic capture. Every unhandled crash sends a structured report with session context.

---

## 🧠 Core Modules

### 🎯 Unified Agent Router
Maps natural-language task descriptions to the right specialist using local BM25 + vector hybrid indexing. Bridges Maestro (39 agents) and ECC (48 agents) ecosystems natively.

### 📉 Token Budgeter & Context Manager (Cortex)
- **Sigmap:** Up to 97% token reduction via high-density signature mapping (`SIGMAP.md`)
- **BM25 persistence:** Index survives restarts via mtime-based invalidation
- **Vector store:** Binary SQLite BLOBs — transaction-safe, no JSON drift
- **Session memory:** Per-agent `memory.md` auto-append

### 🔄 Agent Swarm Mesh
Parallel worktree-based agent execution with coordination primitives:
- **Lock Manager:** File and task-level locks with priority levels (User > Agent > Automation)
- **Radar Whispers:** Real-time context hints pushed to all connected agents (compile errors, security alerts, architectural violations)
- **Factory Mode:** Submit heavy jobs async; completion fires broadcast + optional webhook

### 🔌 Tri-Protocol Interface
All three protocols share one event bus:
- `TCP :42069` — Daemon IPC (UUID token auth, mandatory handshake)
- `TCP :42070` — MCP-over-TCP (agent tool calls, policy-gated)
- `CLI` — Direct commands (`raios <command>`)

### 📊 Portfolio Intelligence
- **Neural Search:** Semantic search across 140+ projects with BM25 + embeddings
- **Health Scanner:** Background scan for `memory.md` compliance, security leaks, git drift
- **GitHub Sync:** Live star counts and last-commit timestamps
- **Auto-Discovery:** Detects new workspace directories and updates `entities.json` automatically

---

## 🖥️ VS Code Extension (v0.4.0)

R-AI-OS ships a native VS Code extension that turns the IDE into a **Hybrid UI** — combining the real-time power of the TUI with the rich surface of the IDE.

```
vscode-extension/
├── src/extension.ts    # Extension host + TokenBridge proxy
├── src/sidebar/        # Webview Kanban dashboard (Geist Sans + glassmorphism)
└── raios-0.4.0.vsix    # Packaged extension
```

**Features:**
- **Activity Bar icon** — `raios-sidebar-view` panel always visible
- **Kanban Dashboard** — Read-only project status, health scores, active tasks
- **TokenBridge Proxy** — Extension host proxies all daemon requests with Bearer token; the session token never touches the Webview context (XSS-safe)
- **Status Bar** — Live daemon connection indicator

**Install:**
```bash
code --install-extension vscode-extension/raios-0.4.0.vsix
```

---

## 🚀 Quick Start

```bash
git clone https://github.com/alazndy/R-AI-OS.git
cd R-AI-OS
cargo install --path . --force
```

Start the daemon (background process that powers the TUI and MCP server):
```bash
aiosd
```

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
| `raios search "<query>"` | Semantic search across portfolio |
| `raios new "ProjectName"` | Scaffold a new project (follows MASTER rules) |
| `raios task "<description>"` | Route to best agent or specific one |
| `raios bootstrap` | Replicate AI factory on a new machine |

### Git Operations

| Command | Description |
| :--- | :--- |
| `raios git status` | Git status across portfolio |
| `raios git log` | Recent commits |
| `raios git diff` | Staged/unstaged diff |
| `raios git commit` | Intelligent bulk commit |

### Security

| Command | Description |
| :--- | :--- |
| `raios verify-chain` | Verify audit log hash-chain integrity |
| `raios verify-chain -n <N>` | Show last N entries then verify |
| `raios security` | OWASP security scan |

### Build & Dev

| Command | Description |
| :--- | :--- |
| `raios build` | Build current project |
| `raios test` | Run test suite |
| `raios deps` | Dependency audit |
| `raios env` | Environment variable scan |

### Agent Swarm

| Command | Description |
| :--- | :--- |
| `raios swarm start` | Start a parallel agent worktree |
| `raios swarm list` | List active swarm tasks |
| `raios swarm approve <id>` | Approve a pending swarm diff |

### Analysis

| Command | Description |
| :--- | :--- |
| `raios rbj --project <name>` | Red-Blue-Judge audit cycle |

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
│   ├── build/            # Build logic (10 sub-modules, language-specific)
│   └── deps/             # Dependency management (10 sub-modules)
├── cortex/               # Vector store, BM25 index, session memory
├── daemon/               # aiosd background daemon
├── intelligence/         # Agent routing, instinct engine, RBJ
├── mcp/                  # MCP server — policy-gated tool call handler
├── search/               # Neural search (BM25 + vector hybrid)
├── security/             # Security Kernel (sandbox, policy, chain, egress)
├── sentinel/             # Redaction engine, Sentry integration
├── server/               # HTTP/WebSocket server (Axum)
├── swarm/                # Parallel worktree agent management
└── ui/
    └── panels/           # TUI panels (13 modules — dashboard, security, etc.)
```

---

## 🗺️ Roadmap

- [x] **Phase 1–7:** Core TUI, workspace mapping, health dashboard, BM25 search
- [x] **Phase 8:** Universal Kernel — Tri-protocol, Lock Manager, Radar Whispers, Factory Mode
- [x] **Phase 9:** Refactor & Modularization — all large files split into focused modules
- [x] **Phase 10:** Hardened Kernel Alpha — Sentry, Redaction Engine, Audit Ledger
- [x] **Phase 10B:** Security Kernel (Faz 1–4) — Sandbox + Policy + Audit Chain + Egress ✅
- [x] **Phase IDE:** Hybrid UI — VS Code Sidebar WebView + TokenBridge Proxy ✅
- [ ] **Phase 11:** Tool Pinning & Drift Detection — MCP tool manifest hashing, supply chain tamper detection
- [ ] **Phase 12:** Secret Leasing — `raios secret grant <tool> <ENV_VAR>` with TTL-based auto-revoke
- [ ] **Phase 13:** Rate Limiting — Tool call frequency limiter for AI loop spam protection
- [ ] **Phase 14:** Quarantine Mode — Isolate suspicious agent calls, require human approval
- [ ] **Phase 15:** Write-Back Bridge — Sidebar Kanban → memory.md task state sync

---

## 🔗 Research References

- **[vigils](https://github.com/duncatzat/vigils)** — Agent control plane (Filesystem Jail, Egress Filter, Policy Manager, Hash-Chain)
- **[ruvos](https://github.com/dgdev25/ruvos)** — Agentic OS memory architecture reference
- **[bash-agent](https://github.com/lloydzhou/bash-agent)** — Lightweight agent worker patterns
- **[agent-skills](https://github.com/addyosmani/agent-skills)** — Engineering discipline and agent verification
- **[needle](https://github.com/cactus-compute/needle)** — Ultra-fast local function calling

---

**R-AI-OS is the bridge between human creativity and autonomous execution.** 🦾🛡️⚔️
