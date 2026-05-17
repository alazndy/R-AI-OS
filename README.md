# ⚡ R-AI-OS: The AI OS Kernel

<p align="center">
  <img src="https://raw.githubusercontent.com/alazndy/r-ai-os/master/assets/raios-logo.png" width="220" alt="R-AI-OS Logo">
</p>

<p align="center">
  <strong>The First High-Performance, LLM-Native Operating System Kernel for Autonomous Swarms</strong>
</p>

<p align="center">
  <a href="https://github.com/alazndy/r-ai-os/releases"><img src="https://img.shields.io/github/v/release/alazndy/r-ai-os?style=for-the-badge&color=blue" alt="Version"></a>
  <a href="https://rust-lang.org"><img src="https://img.shields.io/badge/Built%20with-Rust-orange?style=for-the-badge&logo=rust" alt="Rust"></a>
  <a href="https://github.com/alazndy/r-ai-os/blob/master/LICENSE"><img src="https://img.shields.io/github/license/alazndy/r-ai-os?style=for-the-badge" alt="License"></a>
  <a href="https://owasp.org/www-project-top-ten/"><img src="https://img.shields.io/badge/Security-Hardened-green?style=for-the-badge" alt="Security"></a>
</p>

<p align="center">
  <a href="#-the-vision">Vision</a> •
  <a href="docs/WIKI/Home.md">Documentation</a> •
  <a href="#-core-kernel-modules">Core Modules</a> •
  <a href="#-aura-hardened-edition-v130">Aura Hardened</a> •
  <a href="#-quick-start">Quick Start</a> •
  <a href="#-cli-reference">CLI</a> •
  <a href="#-roadmap">Roadmap</a>
</p>

---

## 🔭 The Vision

R-AI-OS is not just a CLI tool; it is a **Kernel**. While traditional Operating Systems manage hardware (CPU, RAM, Disk), R-AI-OS is designed for the **AI Era**. 

It serves as the **Intelligence Orchestration Layer** that sits between the human user and a decentralized swarm of 90+ autonomous specialists. R-AI-OS handles the complexity of **Context Economics, Semantic Routing, and Zero-Zero-Trust Agent Security**, allowing you to focus on high-level architecture while your "factory" does the heavy lifting.

## 📖 Deep Dive Documentation

For detailed technical guides, architectural maps, and security protocols, visit our **[Official Wiki](docs/WIKI/Home.md)**:

*   🏛️ **[Architecture](docs/WIKI/01-Architecture-Deep-Dive.md)** - Daemon/Client design.
*   🛡️ **[Security](docs/WIKI/02-Security-Model.md)** - AgentShield & Token Auth.
*   🧠 **[Memory & Context](docs/WIKI/03-Hybrid-Memory-and-Context.md)** - Cortex & Sigmap.
*   ⌨️ **[CLI Reference](docs/WIKI/06-CLI-Commands-Reference.md)** - Detailed command guide.

---

## 🧠 Core Kernel Modules

### 🎯 1. Unified Agent Router (The Brain)
The Router solves the "Over-Agenting" problem. With 90+ specialists available, R-AI-OS selects the right one instantly.
- **Semantic Dispatch:** Maps task descriptions to agent metadata using local neural indexing.
- **Maestro & ECC Bridge:** Native integration with both major agentic frameworks.

### 🛡️ 2. Universal AgentShield (The Guard)
Safety is non-negotiable. AgentShield acts as a low-level syscall filter.
- **Command Interception:** Blocks destructive actions (`rm -rf /`, etc.) before they execute.
- **Secret Leak Protection:** Sanitizes `.env`, `.pem`, and API keys in real-time.

### 📉 3. Token Budgeter & Context Manager (The Economist)
Context is the new currency. R-AI-OS ensures you don't go bankrupt.
- **Sigmap Synergy:** Up to **97% token reduction** via high-density signature mapping.
- **Neural Budgeting:** Automatically prevents raw file ingestion for large directories.

### 🧬 4. Autonomous Instinct Engine (The Memory)
Memory is not static; it is evolutionary.
- **Behavioral Persistence:** Learns your style, favorite libraries, and project-specific quirks.
- **Cross-Session Injection:** Learned "Instincts" follow you across projects and sessions.

---

## 🦾 Aura Hardened Edition (v1.3.0)

R-AI-OS has evolved into its most stable and secure version yet:

*   **🛡️ IPC Hardening:** Random UUID-based token authentication for `aiosd` daemon.
*   **📥 Diff Inbox Pattern:** Non-blocking, asynchronous change approval workflow.
*   **🏗️ Daemon-Centric:** All heavy indexing and sync tasks are handled by the background daemon.
*   **🔍 Neural Search:** Advanced BM25 + Vector hybrid search across your entire workspace.

---

## 🚀 Quick Start

### Installation
Ensure you have Rust installed, then:
```bash
git clone https://github.com/alazndy/r-ai-os.git
cd r-ai-os
cargo install --path . --force
```

### The "One-Touch" Setup
Replicate your entire AI software factory (90+ agents, 180+ skills) on any machine:
```bash
raios bootstrap
```

---

## 💻 CLI Reference

| Command | Usage | Description |
| :--- | :--- | :--- |
| **`task`** | `raios task "optimize db"` | Routes to the best specialist agent. |
| **`health`** | `raios health <project>` | Scans for compliance and security leaks. |
| **`search`** | `raios search "auth logic"` | Semantic search across your portfolio. |
| **`commit`** | `raios commit --push` | Intelligent bulk commit for dirty projects. |
| **`new`** | `raios new "ProjectName"` | Scaffolds a project following official rules. |

---

## 🗺️ Roadmap

- [x] **Phase 1: Core Evolution** - Workspace mapping and health.
- [x] **Phase 2: AI OS Kernel** - Router, Shield, Instincts, Universal Bootstrap.
- [x] **Phase 3: TUI Mission Control** - Aura Hardened IPC & Diff Inbox.
- [ ] **Phase 4: SQLite Migration** - High-concurrency state management.
- [ ] **Phase 5: Agent Swarm Mesh** - Multi-node agent orchestration support.
- [ ] **Phase 6: Edge-Intelligence & Local Routing** - Integration of **Needle** for ultra-fast, zero-cost local function calling.
- [ ] **Phase 7: Evolutionary Intelligence (Experimental)** - Self-evolving skills and autonomous instinct refinement based on task success/failure (Inspired by **OpenSpace**).
- [ ] **Phase 8: Recursive Reasoning (Experimental)** - Deep task decomposition and recursive logic flows for complex architectural problems (Inspired by **RLM**).

---

## 🔗 Research References

These projects are foundational references for the future evolution of R-AI-OS:

*   **[agent-skills](https://github.com/addyosmani/agent-skills):** Engineering discipline and agent verification protocols.
*   **[needle](https://github.com/cactus-compute/needle):** Ultra-fast local function calling and edge-intelligence.
*   **[OpenSpace](https://github.com/HKUDS/OpenSpace):** Self-evolving skills and spatial intelligence.
*   **[rlm](https://github.com/alexzhang13/rlm):** Recursive language models for deep logical reasoning.

---

**R-AI-OS is the bridge between human creativity and autonomous execution.** 🦾🛡️⚔️
