# R-AI-OS Async Workflow and Inbox

This document explains the asynchronous nature of R-AI-OS, focusing on how the system handles background agent tasks and the human-in-the-loop approval mechanism.

## 1. The Non-Blocking Philosophy

R-AI-OS is built on a **Non-Blocking Philosophy**. In traditional AI tools, the user often has to wait for the LLM to finish generating code before they can continue working. R-AI-OS breaks this pattern by decoupling the AI's execution from the user interface.

### Key Principles:
- **UI Responsiveness:** The Terminal User Interface (TUI) runs on a dedicated high-priority thread. Even when multiple agents are performing heavy computations or disk I/O, the TUI remains interactive at 60 FPS.
- **Background Execution:** All agent tasks (indexing, refactoring, searching) are dispatched to the `aiosd` daemon's worker pool.
- **Parallelism:** Multiple agents can work on different parts of the codebase simultaneously without interfering with the user's current focus.

## 2. Diff Inbox Pattern

When an agent proposes a modification to the codebase, it does not overwrite files directly. Instead, it follows the **Diff Inbox Pattern**. This ensures safety and maintains the "Human-as-the-Master" authority.

### The Workflow:
1. **Proposal:** An agent generates a "Code Change" request.
2. **Queueing:** This request is sent to the daemon and stored in the **Inbox** with a `PENDING` status.
3. **Notification:** The TUI receives a background message (`BgMsg`) indicating a new inbox item.
4. **Header Alert:** A visual indicator (e.g., a pulsing "📥" or "1 PENDING") appears in the TUI Header to alert the user without interrupting their current task.

## 3. The Approval UI ('i' shortcut)

The **Approval UI** is the central hub for reviewing and applying agent-generated changes. It provides a safe environment to inspect AI suggestions before they touch the disk.

### Interaction Model:
- **Access:** Pressing the `i` key (Inbox) from anywhere in the TUI opens the Approval panel.
- **Side-by-Side View:** The UI presents a rich, syntax-highlighted diff view. The left pane shows the current file state, and the right pane shows the proposed changes.
- **Granular Review:** Users can scroll through the diffs to understand exactly what the agent is proposing.
- **Decision Mechanism:**
    - **Approve (Enter/A):** The change is applied to the filesystem, and the inbox item is cleared.
    - **Reject (Esc/R):** The change is discarded, and the agent is notified (if applicable) to rethink its approach.

## 4. Task Semantic Routing

R-AI-OS uses **Task Semantic Routing** to manage complex, multi-step operations across different specialized agents.

### How it Works:
- **Asynchronous Handoffs:** If the `Architect` agent decides that a task requires implementation, it doesn't wait for the `Coder` agent. It routes the task semantically to the `Coder`'s queue and moves on to its next architectural analysis.
- **Context Preservation:** When a task is routed, the full semantic context (relevant files, previous conversation, goals) is bundled with it.
- **State Tracking:** The `aiosd` daemon tracks the state of these routed tasks, allowing the user to see the progress of a multi-agent workflow in the Dashboard.
- **Event-Driven:** The system reacts to task completions by triggering downstream actions, such as notifying the `Tester` agent to verify a newly approved code change.

---

*R-AI-OS: High-velocity development with asynchronous intelligence and absolute control.*
