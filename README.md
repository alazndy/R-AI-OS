# ⚡ Raios: The AI OS Kernel

<p align="center">
  <img src="https://raw.githubusercontent.com/alazndy/r-ai-os/master/assets/raios-logo.png" width="220" alt="Raios Logo">
</p>

<p align="center">
  <strong>The First High-Performance, LLM-Native Operating System Kernel for Autonomous Swarms</strong>
</p>

<p align="center">
  <a href="https://github.com/alazndy/r-ai-os/releases"><img src="https://img.shields.io/github/v/release/alazndy/r-ai-os?style=for-the-badge&color=blue" alt="Version"></a>
  <a href="https://rust-lang.org"><img src="https://img.shields.io/badge/Built%20with-Rust-orange?style=for-the-badge&logo=rust" alt="Rust"></a>
  <a href="https://github.com/alazndy/r-ai-os/blob/master/LICENSE"><img src="https://img.shields.io/github/license/alazndy/r-ai-os?style=for-the-badge" alt="License"></a>
</p>

<p align="center">
  <a href="#-the-vision">Vision</a> •
  <a href="#-core-modules">Core Modules</a> •
  <a href="#-technical-specifications">Technical Specs</a> •
  <a href="#-get-started">Get Started</a> •
  <a href="#-cli-reference">CLI</a> •
  <a href="#-roadmap">Roadmap</a>
</p>

---

## 🔭 The Vision

Raios is not just a CLI tool; it is a **Kernel**. While traditional Operating Systems (Windows, macOS, Linux) were designed to manage hardware resources (CPU, RAM, Disk), Raios is designed for the **AI Era**. 

It serves as the **Intelligence Orchestration Layer** that sits between the human user and a decentralized swarm of 90+ autonomous specialists. Raios handles the complexity of **Context Economics, Semantic Routing, and Zero-Trust Agent Security**, allowing you to focus on high-level architecture while your "factory" does the heavy lifting.

---

## 🧠 Core Kernel Modules

### 🎯 1. Unified Agent Router (The Brain)
The Router solves the "Over-Agenting" problem. With 90+ specialists available from Maestro and ECC ecosystems, selecting the right one is impossible for humans. 
- **Semantic Dispatch:** Uses the local `Cortex` engine to map task descriptions to agent metadata.
- **Dynamic Delegation:** `raios task "Fix this Rust bug"` automatically initializes a `rust-build-resolver` followed by a `tester`.
- **Maestro & ECC Bridge:** Native integration with both major agentic frameworks.

### 🛡️ 2. Universal AgentShield (The Guard)
In an autonomous world, safety is non-negotiable. AgentShield acts as a low-level syscall filter.
- **Command Interception:** Pre-scans every command an agent attempts to run. Blocks destructive actions (`rm -rf /`, `mkfs`, etc.).
- **Secret Leak Protection:** Identifies and sanitizes `.env`, `.pem`, and API keys before they are exposed to the agent's context.
- **Pre-flight Health Checks:** Scans project directories for security vulnerabilities before any agent is allowed to start.

### 📉 3. Token Budgeter & Context Manager (The Economist)
Context is the new currency. Raios ensures you don't go bankrupt.
- **Automatic Compaction:** If a project directory exceeds **300KB**, Raios prevents raw file ingestion.
- **Sigmap Sinerjisi:** Automatically runs `Sigmap` to generate a lightweight signature map (up to **97% token reduction**).
- **Neural Budgeting:** Injects `RAIOS_CONTEXT_MODE=compact` into agent environments, forcing them to use high-density summaries.

### 🧬 4. Autonomous Instinct Engine (The Memory)
Memory should not be static. It should be evolutionary.
- **Behavioral Persistence:** Raios learns your coding style, favorite libraries, and project-specific quirks.
- **Cross-Session Injection:** Learned "Instincts" are stored in a global `instincts.json` and automatically injected into future sessions.
- **Seamless Continuity:** Gemini or Claude will remember how you prefer error handling even if you switch projects.

---

## 🛠️ Technical Specifications

| Component | Technology | Benefit |
| :--- | :--- | :--- |
| **Language** | 🦀 Pure Rust | Memory safety, zero-cost abstractions, extreme speed. |
| **Vector Engine** | 🧬 Cortex (HNSW) | Privacy-first local embeddings (MiniLM) for agent discovery. |
| **Search** | 🔗 Hybrid (BM25 + Vector) | Combines keyword precision with semantic understanding. |
| **Deployment** | 🪄 Universal Bootstrap | Cross-platform (Win/Mac/Linux) deployment in seconds. |
| **Security** | 🛡️ OWASP-Mapped Scans | Static analysis for hardcoded secrets and injection risks. |
| **Architecture** | 🧊 Modular State | Namespaced domain sub-structs for high-performance TUI updates. |

---

## 🚀 Get Started

### Quick Install
Ensure you have Rust installed, then:
```bash
git clone https://github.com/alazndy/r-ai-os.git
cd r-ai-os
cargo install --path . --force
```

### The "One-Touch" Setup
Replicate your entire AI software factory (90+ agents, 182 skills) on any machine:
```bash
raios bootstrap
```

---

## 💻 CLI Reference

| Command | Usage | Description |
| :--- | :--- | :--- |
| **`task`** | `raios task "optimize db"` | **[NEW]** Routes to the best specialist (e.g., `database_admin`). |
| **`bootstrap`** | `raios bootstrap` | **[NEW]** Installs Maestro, ECC, Sigmap, and all configs. |
| **`health`** | `raios health <project>` | Scans for MASTER.md compliance and security leaks. |
| **`search`** | `raios search "auth logic"` | Semantic search across your entire workspace portfolio. |
| **`commit`** | `raios commit --push` | Intelligent bulk commit/push for all dirty projects. |
| **`new`** | `raios new "MyProject"` | Scaffolds a new project following the official anayasa. |

---

## 🗺️ Roadmap

- [x] **Phase 1: Core Evolution** (v1.0 - v1.1) - Workspace mapping and health.
- [x] **Phase 2: AI OS Kernel** (v1.2.0) - Router, Shield, Instincts, Universal Bootstrap, Modular Namespaced State.
- [ ] **Phase 3: TUI Mission Control** (v1.3.0) - Real-time visual monitoring (State-synchronized TUI).
- [ ] **Phase 4: Agent Swarm Mesh** (v1.4.0) - Distributed kernel support for multi-node agent orchestration.

---

**Raios is the bridge between human creativity and autonomous execution.** 🦾🛡️⚔️
