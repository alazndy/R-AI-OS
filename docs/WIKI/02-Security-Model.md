# Security Model

R-AI-OS is built with a "Security-First" philosophy, recognizing that autonomous AI agents require robust guardrails to prevent accidental or malicious damage to the host system and codebase. This document outlines the multi-layered security architecture of R-AI-OS.

## 1. Zero-Trust Agent Security

The core principle of R-AI-OS is **Zero-Trust**. No agent, regardless of its source or purpose, is granted implicit trust.

- **Process Isolation:** Agents do not run within the daemon's memory space. They are spawned as isolated child processes with restricted environments.
- **Human-in-the-Loop (HITL):** Critical actions—such as modifying source code, deleting files, or handing over tasks to other agents—are intercepted by the daemon and queued for human approval via the `raios` TUI.
- **Stateful Monitoring:** The daemon (`aiosd`) maintains a real-time state of all active agents, tracking their resource usage and command history.

## 2. AgentShield Guard

The **AgentShield** is a specialized security layer that acts as a firewall between the agent and the operating system.

### Syscall & Command Interception
Every shell command an agent attempts to execute is passed through the Shield's regex-based interception engine.
- **Dangerous Command Blacklist:** Commands like `rm -rf /`, `mkfs`, `dd`, and `curl | sh` are blocked instantly.
- **Pattern Matching:** The Shield uses high-performance regex to detect obfuscated or dangerous command sequences before they reach the shell.

### Secret Leak Protection
To prevent the accidental exposure of credentials, AgentShield performs:
- **Pre-flight Checks:** Before an agent starts, the Shield scans the target directory for sensitive files like `.env`, `.pem`, or `id_rsa`.
- **OWASP Static Analysis:** A comprehensive suite of patterns (aligned with OWASP Top 10) scans for hardcoded API keys (e.g., OpenAI, AWS), passwords, and authentication tokens in the codebase.
- **Git Integration:** Checks if sensitive files are being tracked by Git or missing from `.gitignore`.

## 3. IPC Token Authentication (Aura Hardened)

Communication between the client (`raios`) and the daemon (`aiosd`) is secured using the **Aura Hardened** protocol.

### Dynamic UUID Generation
- Upon every startup, `aiosd` generates a unique **UUID v4** token.
- This token is stored in a secure, local configuration file (`~/.config/raios/.ipc_token`) with restricted filesystem permissions.

### Handshake Process
1. **Connection:** The client opens a TCP connection to `127.0.0.1:42069`.
2. **Challenge:** The client must immediately send an `AUTH <UUID_TOKEN>` command.
3. **Validation:** The daemon compares the provided token with the session token.
4. **Enforcement:** If the first message is not a valid `AUTH` command, or if the token is incorrect, the daemon **immediately drops the connection** and logs a security alert.

## 4. Filesystem Boundaries

R-AI-OS enforces strict boundaries on how agents interact with the filesystem.

- **SafeIO Interception:** The `SafeIO` module intercepts file write operations. If the daemon is active, `safe_write` automatically converts a direct disk write into a `RequestFileChange` event, sending the diff to the user for approval.
- **Workspace Anchoring:** Agents are logically anchored to the workspace root. Any attempt to access paths outside the project scope (e.g., `/etc/passwd`, `~/.ssh`) is flagged by the Shield.
- **Sentinel Guard:** A background worker continuously monitors critical project files (like `Cargo.toml`, `package.json`, or `memory.md`). Any unauthorized modification triggers an immediate system-wide alert and pauses active agents.

---

*R-AI-OS: Hardening the future of autonomous development.*
