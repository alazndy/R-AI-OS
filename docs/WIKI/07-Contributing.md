# 🤝 Contributing to R-AI-OS

Welcome to the R-AI-OS development community! As a kernel for autonomous agents, we maintain high standards for code quality, security, and architectural consistency. This guide outlines the rules and protocols every contributor (human or AI) must follow.

## 📜 The Anayasa (MASTER.md)
The "Anayasa" (Constitution) defines the core operational rules of the R-AI-OS ecosystem. All development must adhere to these five pillars:

1.  **Local Memory First:** Always read `AGENTS.md` (if available) and the project's `memory.md` at the start of every session. Update `memory.md` with your progress before finishing.
2.  **Skeleton-First:** Before writing implementation code, define the file/folder structure and get architectural approval.
3.  **Component-by-Component:** Do not attempt to generate the entire codebase at once. Build, test, and verify one component at a time.
4.  **Code Discipline:** Write functional code. Use clean code principles instead of excessive comments. Never ignore error handling.
5.  **Planning:** Create a plan and wait for approval before writing code or deleting files.

## 🤖 Agent Compliance
When adding or modifying agents within R-AI-OS, ensure they meet the following compliance standards:

-   **Naming:** Use `snake_case` for agent names (e.g., `security_engineer`, not `SecurityEngineer`).
-   **Methodology:** Every agent must have a defined methodology in its frontmatter.
-   **Tool Restrictions:** Limit agent tools to the minimum required for their specific domain.
-   **Identity:** Agents must identify themselves in the `memory.md` log.
-   **Security:** Agents must never bypass the **AgentShield** or **Sentinel** guardrails.

## 🧠 Memory Format
The `memory.md` file is the source of truth for project state. It must follow this structure:

```markdown
# [Project Name] Memory

## Current Status
- Date: YYYY-MM-DD
- Active agent: [Name] (vX.Y.Z)
- Version: vX.Y.Z
- Status: [Brief summary of health/readiness]

## [Agent Name]
### Achievements
- Bullet points of specific tasks completed.
- Technical details (e.g., "Implemented X using Y").

## Plan
### Completed
- [x] Task A
### In Progress
- [ ] Task B
### Next Steps
- [ ] Task C

## Decision Log
| Date | Agent | Decision | Rationale |
|------|-------|----------|-----------|
| ...  | ...   | ...      | ...       |

## Instincts
- Project-specific "gut feelings" or rules to remember (e.g., "Always use OnceLock for regex").
```

## 🛠️ Pull Request Rules
To maintain the "Aura Hardened" standard, all PRs must satisfy:

1.  **Zero Regressions:** All existing unit tests must pass (`cargo test`).
2.  **Security Scan:** Must pass `raios security` scan without critical vulnerabilities.
3.  **Linting:** Code must be formatted with `cargo fmt` and pass `cargo clippy` without warnings.
4.  **Documentation:** Any new feature must be documented in the `docs/WIKI/` or updated in `06-CLI-Commands-Reference.md`.
5.  **Sigmap Update:** If the project signature changes, the `SIGNATURES.md` or equivalent must be updated to maintain context efficiency.

---

> "Quality is not an act, it is a habit." — R-AI-OS Core Team 🦾🛡️
