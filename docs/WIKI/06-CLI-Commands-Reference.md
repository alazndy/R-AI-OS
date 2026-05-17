# CLI Commands Reference

This document provides a comprehensive guide to the `raios` command-line interface. R-AI-OS provides a powerful set of tools for managing your Dev Ops workspace, performing security scans, searching across projects, and orchestrating AI agents.

## Usage

```bash
raios [COMMAND] [FLAGS]
```

If no command is provided, `raios` launches the **Interactive TUI Dashboard**.

### Global Flags
- `-j, --json`: Output results in JSON format for programmatic use.
- `-h, --help`: Show help information for a command.
- `-V, --version`: Print the current version of R-AI-OS.

---

## Core Commands

### `task`
Automatically routes a task description to the best specialist agent (e.g., Coder, Architect, Security Reviewer).

**Usage:**
```bash
raios task "<description>" [FLAGS]
```

**Flags:**
- `-p, --project <path>`: Specify the project directory to context-ground the task.

**Example:**
```bash
raios task "Implement a JWT authentication middleware" --project ./my-api
```

---

### `health`
Generates a health report for one or all projects, checking for git status, compliance with `MASTER.md`, and security scores.

**Usage:**
```bash
raios health [project_name] [FLAGS]
```

**Example:**
```bash
raios health my-project
```

---

### `search`
Performs a hybrid search (Semantic Vector + BM25 Keyword) across the entire Dev Ops workspace.

**Usage:**
```bash
raios search "<query>" [FLAGS]
```

**Flags:**
- `-t, --top-k <number>`: Number of results to return (default: 8).
- `--reindex`: Force a full re-indexing of the workspace before searching.

**Example:**
```bash
raios search "how to configure postgres in rust" --top-k 5
```

---

### `commit`
Bulk commits dirty projects across the workspace.

**Usage:**
```bash
raios commit [FLAGS]
```

**Flags:**
- `-p, --project <name>`: Filter to a single project.
- `-m, --message <msg>`: Custom commit message (default: "chore: raios auto-sync").
- `--push`: Automatically push to remote after committing.
- `--dry-run`: Show which projects would be committed without performing the action.

**Example:**
```bash
raios commit --message "refactor: update dependencies" --push
```

---

### `new`
Scaffolds a new project following the strict rules defined in `MASTER.md`.

**Usage:**
```bash
raios new <name> [FLAGS]
```

**Flags:**
- `-c, --category <category>`: Specify the category folder (e.g., `01_Web_Apps`).
- `--github`: Create a private GitHub repository and push the initial scaffold.
- `--no-vault`: Skip updating the Obsidian Vault project atlas.

**Example:**
```bash
raios new my-awesome-app --category 01_Web_Apps --github
```

---

### `memory`
Manages project `memory.md` files and performs semantic search across all project memories, `AGENTS.md`, and `MASTER.md`.

**Usage:**
```bash
raios memory [project_name] [FLAGS]
```

**Flags:**
- `-q, --query <text>`: Perform a semantic search across all memory files.
- `-n, --top <number>`: Number of search results to show (default: 5).

**Example:**
```bash
raios memory -q "deployment strategy for esp32"
```

---

### `security`
Runs an OWASP-aligned security scan on projects to identify vulnerabilities and hardcoded secrets.

**Usage:**
```bash
raios security [FLAGS]
```

**Flags:**
- `-p, --project <name>`: Scan a specific project.
- `--full`: Show the detailed list of issues instead of just a summary.
- `--path <dir>`: Scan a specific directory directly (bypassing the project registry).
- `-w, --watch`: **Sentinel Mode** — Continuously monitor file changes and report security issues in real-time via system notifications.

**Example:**
```bash
raios security --project my-app --full
raios security --watch
```

---

### `instinct`
Manages "Instincts" — learned rules and project-specific patterns that guide AI agents.

**Usage:**
```bash
raios instinct <SUBCOMMAND>
```

**Subcommands:**
- `add "<rule>"`: Manually add a rule to global instincts and project `memory.md`.
- `list`: List all global and project-specific instincts.
- `suggest`: Analyze project health and suggest new instincts (interactive).

**Example:**
```bash
raios instinct add "Always use pnpm for frontend projects" --path ./my-web-app
raios instinct suggest my-project
```

---

### `bootstrap`
The "One-Click" installer for the entire R-AI-OS ecosystem. It installs necessary CLI tools, configures Gemini/Claude extensions, and syncs 180+ skills and 90+ agents.

**Usage:**
```bash
raios bootstrap
```

---

## Advanced Commands

| Command | Description |
| :--- | :--- |
| `stats` | Displays workspace portfolio statistics (Grade distribution, categories, etc.). |
| `discover` | Scans the Dev Ops directory to find and register new projects. |
| `cortex-index` | Manually triggers a re-indexing of the semantic memory store. |
| `version-bump` | Bumps project version (patch/minor/major) and updates CHANGELOG.md. |
| `disk` | Analyzes disk usage, identifying large `target` or `node_modules` folders. |
| `clean` | Removes build artifacts and caches to free up space. |
| `ps` | Lists all listening ports and associated processes. |
| `env` | Validates `.env` files for missing keys or undocumented secrets. |
| `deps` | Checks for outdated dependencies and known CVE vulnerabilities. |
| `mcp-server` | Starts the R-AI-OS MCP server for integration with Claude Code/Gemini. |

---

## Advanced Flags & Output

### JSON Output (`--json`)
Most commands support the `--json` flag, which is ideal for piping into other tools like `jq` or for use in custom scripts.

```bash
raios health --json | jq '.[0].compliance_score'
```

### Watch Mode (`--watch`)
The `security` command's watch mode uses the **Sentinel** engine to provide real-time feedback. It is recommended to keep a terminal open with `raios security --watch` during active development.
