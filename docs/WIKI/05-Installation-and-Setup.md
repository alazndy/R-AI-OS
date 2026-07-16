# 05. Installation and Setup

This guide provides step-by-step instructions for installing R-AI-OS and setting up your development environment.

## Prerequisites

Before installing R-AI-OS, ensure your system meets the following requirements:

- **Rust & Cargo:** The core system is built with Rust. You need the latest stable version.
  - [Install Rust](https://www.rust-lang.org/tools/install)
- **Git:** Required for repository management and internal Git integration features.
- **Node.js & pnpm:** (Optional) Required only if you intend to build or modify the TUI/Web frontend components.
- **Operating System:** 
  - **Windows:** Primary development and testing platform.
  - **Linux/macOS:** Supported via standard Rust toolchains.

## Step-by-Step Installation

### 1. Clone the Repository
Start by cloning the R-AI-OS repository to your local machine:
```bash
git clone https://github.com/your-repo/R-AI-OS.git
cd R-AI-OS
```

### 2. Build the Binaries
Build the project using Cargo. We recommend the `--release` flag for production-ready performance:
```bash
cargo build --release
```
This will generate two main binaries in `target/release/`:
- `raios`: The primary CLI tool.
- `aiosd`: The background daemon.

### 3. Global Installation
To access the `raios` command from any directory, install the binaries to your Cargo bin path:
```bash
cargo install --path .
```
Ensure that `~/.cargo/bin` (or `%USERPROFILE%\.cargo\bin` on Windows) is included in your system's `PATH` environment variable.

### Windows one-command installer

For a normal Windows 10/11 machine, run the repository installer from
PowerShell instead of copying binaries manually:

```powershell
Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
.\install-system.ps1
```

It builds `raios.exe` and `aiosd.exe`, installs them under
`%LOCALAPPDATA%\R-AI-OS\bin`, prepares `%APPDATA%\raios\config.toml` and
`raios-policy.toml`, adds the binary directory to the user `PATH`, and creates
the `RAIOS_Daemon` logon task. Use `-NoScheduledTask` when the daemon should be
started manually.

## The Bootstrap Command

After installation, you must initialize the system using the bootstrap command:
```bash
raios bootstrap
```

### What happens during Bootstrap?
The `bootstrap` command is the "Day 0" operation that prepares the R-AI-OS kernel:

1.  **Maestro Orchestrator Setup:** Initializes the multi-agent orchestration layer and ensures the necessary agent rosters are available.
2.  **ECC (Encrypted Communication Channel):** Generates the initial security handshake tokens required for the CLI to talk to the Daemon.
3.  **Sigmap Generation:** Automatically runs a `sigmap` scan on the current workspace to create a `SIGNATURES.md` map, providing the AI with an immediate structural understanding of the codebase.
4.  **Directory Initialization:** Creates the `~/.config/raios/` directory and populates it with default configuration templates.

## Environment Configuration

R-AI-OS relies on a few key configuration files and security tokens to function correctly.

### 1. IPC Token (`.session_token`)
Security is handled via a rolling IPC token.
- **Location:** `~/.config/raios/.session_token` (Windows: `%APPDATA%\raios\.session_token`)
- **Behavior:** The `aiosd` daemon generates a random secret upon startup and writes it to this file. Local clients read this token to authenticate every request.
- **Security:** This file is created with restricted filesystem permissions to ensure only the local user can access it.

### 2. Configuration Files
- **Global Config:** `~/.config/raios/config.toml` (Windows: `%APPDATA%\raios\config.toml`) stores workspace and daemon settings.
- **Project Manifest:** You can place a `.raios.yaml` file in any project root to override global settings for that specific workspace.

### 3. Environment Variables
You can override certain behaviors using environment variables:
- `RAIOS_LOG_LEVEL`: Set to `trace`, `debug`, `info`, `warn`, or `error`.
- `RAIOS_STATE_DIR`: Custom path for session and state storage (defaults to `docs/maestro` within the project).

## Verification
To verify your installation, run:
```bash
raios health
```
If the system is correctly installed and the daemon is running, you should see a dashboard showing the status of your current project and the health of the R-AI-OS kernel.
