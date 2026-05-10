# R-AI-OS

**The Agentic Operating System Control Center**

Built with **Rust** and [Ratatui](https://ratatui.rs), R-AI-OS is a high-performance terminal interface designed for orchestrating autonomous AI agents, managing complex file systems, and providing real-time system health metrics.

---

## 🚀 The Vision

Moving beyond the limitations of legacy terminal tools, R-AI-OS (Rust AI OS) is engineered for the future of "Agentic Computing." It provides a bridge between human intent and AI execution through a secure, high-speed, and beautifully crafted TUI.

- **Rust-Powered:** Memory safety and zero-cost abstractions ensure R-AI-OS runs with minimal overhead.
- **Agent-First:** Native support for agent configurations, memory management, and automated rule compliance.
- **Performance-Driven:** Designed to handle 40+ concurrent projects with sub-millisecond status updates.

---

## 🛠️ Key Pillars

### 🖥️ TUI Mastery
- **Interactive Dashboard:** 6-module control panel with real-time project health scores.
- **Advanced File Explorer:** Integrated file viewer with syntax awareness and line-by-line scrolling.
- **In-Terminal Editor:** Custom-built line editor with `Ctrl+S` saving and seamless agent interaction.

### 🧠 Cortex & Daemon Intelligence
- **aiosd (Daemon):** An invisible background service that handles state synchronization and project discovery.
- **Cortex Engine:** Vector-ready architecture for RAG (Retrieval-Augmented Generation) workflows.
- **Hybrid Search:** Combines keyword search with vector retrieval for pinpoint accuracy across your codebase.

### 🛡️ Security & Compliance
- **MCP Server Integration:** Built-in Model Context Protocol server for secure tool-sharing with LLMs.
- **Compliance Engine:** Automatic scanning of project rules (e.g., `GEMINI.md`, `hardware-rules.md`) to ensure architectural integrity.

---

## ⌨️ Quick Usage

### Launch the TUI
```bash
cargo run --release
# Or run the pre-built binary
./raios
```

### Agent CLI Mode (Headless)
R-AI-OS can be used by AI agents as a command-line tool:
```bash
raios rules                   # List all system rules
raios memory aios             # Fetch specific project memory
raios projects --json         # Get full project inventory in JSON
raios view MASTER.md          # Print any file content for context
```

---

## 🗺️ Roadmap

- [ ] **Ghost Protocol Integration:** Advanced proxy isolation for agent execution.
- [ ] **SQLite Migration:** Transitioning from flat JSON files to a robust SQLite backend.
- [ ] **Visual Telemetry:** Real-time resource usage graphs via `ratatui-widgets`.

---

## 📦 Installation

```bash
# Ensure Rust is installed (https://rustup.rs)
git clone https://github.com/alazndy/R-AI-OS.git
cd R-AI-OS
cargo build --release
```

**License:** MIT  
**Author:** alazndy <goktugturhan74@gmail.com>
