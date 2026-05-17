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

---

### 🚀 Getting Started
If you haven't installed R-AI-OS yet, please refer to the [Quick Start section in the README](../../README.md#-quick-start).

> "R-AI-OS is the bridge between human creativity and autonomous execution." 🦾🛡️⚔️
