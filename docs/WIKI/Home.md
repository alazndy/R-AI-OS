# 📖 R-AI-OS Official Wiki

Welcome to the official documentation of **R-AI-OS: The AI OS Kernel**. R-AI-OS is designed as a high-performance orchestration layer for autonomous agents, focusing on security, context efficiency, and asynchronous workflows.

Explore the technical depths of the system through the chapters below:

---

### 🏛️ [01. Architecture Deep Dive](01-Architecture-Deep-Dive.md)
Learn about the **Daemon-Centric** design, the client/server split between `aiosd` and `raios`, and the **Aura Hardened** IPC protocol that ensures high-performance connectivity.

### 🛡️ [02. Security Model](02-Security-Model.md)
Understand how we enforce safety in an autonomous world. This chapter covers **AgentShield**, command interception, secret leak protection, and zero-trust authentication.

### 🧠 [03. Hybrid Memory & Context](03-Hybrid-Memory-and-Context.md)
Dive into the brain of R-AI-OS. Discover how the **Cortex Engine** combines Vector Search (HNSW) with BM25, and how **Sigmap** reduces token costs by up to 97%.

### 📥 [04. Async Workflow & Inbox](04-Async-Workflow-and-Inbox.md)
See how R-AI-OS maintains a non-blocking UI. Learn about the **Diff Inbox Pattern**, the approval workflow, and the semantic routing of tasks between specialists.

### ⚙️ [05. Installation and Setup](05-Installation-and-Setup.md)
A comprehensive guide to setting up R-AI-OS, from Rust toolchain prerequisites to the `bootstrap` command and security token configuration.

### ⌨️ [06. CLI Commands Reference](06-CLI-Commands-Reference.md)
A comprehensive guide to the `raios` CLI, including core commands, examples, and advanced flags.

### 🤝 [07. Contributing](07-Contributing.md)
Guidelines for contributors, including the "Anayasa" (Constitution), agent compliance, memory formats, and PR rules.

### 🧭 [Control Plane Blueprint](../superpowers/specs/2026-06-10-control-plane-blueprint.md)
The control-plane target architecture for evolving R-AI-OS into a dependable project tracker and agent harness. Covers canonical entities, state machines, scheduler design, run contracts, approvals, budgets, and protocol mapping.

### 🗺️ [Control Plane Remaining Work Plan](../superpowers/specs/2026-06-10-control-plane-remaining-work-plan.md)
The implementation plan for the remaining control-plane migration. Covers ordered phases, file targets, acceptance criteria, and handoff guidance for follow-up coding agents.

---

### 🚀 Getting Started
If you haven't installed R-AI-OS yet, please refer to the [Quick Start section in the README](../../README.md#-quick-start).

> "R-AI-OS is the bridge between human creativity and autonomous execution." 🦾🛡️⚔️
