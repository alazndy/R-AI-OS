# Phase 9: IDE Symbiosis (VS Code Extension) Plan

## 1. Vision & Objective
**Goal:** To seamlessly integrate the R-AI-OS Kernel (`aiosd` daemon) into the developer's natural habitat (VS Code), eliminating the need for a separate GUI application or constant context-switching to the terminal for micro-management tasks.

**Philosophy:** R-AI-OS TUI is the "Strategic Command Center" (Helicopter view of 140+ projects). The VS Code Extension is the "Tactical Battlefield" (Micro-interactions, single-project focus, rich diffs).

## 2. Architecture: "Thin Client" Model
The VS Code extension will contain **zero AI logic and zero indexing logic**. It acts purely as a presentation and interaction layer (Thin Client) connected to the R-AI-OS Daemon (`aiosd`).

### 2.1. Communication Protocol (Aura IPC over WebSocket/TCP)
- `aiosd` will expose a local WebSocket or TCP port (e.g., `localhost:31415`).
- The VS Code extension connects using the standard `~/.config/raios/.ipc_token` for Zero-Trust authentication.
- **Payloads:** JSON-RPC format (same as the existing daemon proxy logic).

## 3. Core Extension Features (The MVP)

### Feature 1: The Health & Status Bar
- **Location:** VS Code Bottom Status Bar.
- **Function:** Real-time reflection of the current project's health.
- **Display:** `R-AI-OS: 🟢 94/100 (A)` | `Aura: Shield ON` | `Pending Diffs: 2`
- **Interaction:** Clicking the status bar opens the "Inbox" or runs a quick `raios health` scan.

### Feature 2: The Diff Inbox Overlay (Crucial for Asynchronous Flow)
- **Problem:** TUI diffs are hard to read for massive refactors.
- **Solution:** When an agent finishes a task and requires approval, the extension shows a non-intrusive toast notification.
- **Action:** Clicking "Review" opens VS Code's native, rich Side-by-Side Diff Editor.
- **Buttons:** Accept (`[v]`) or Reject (`[x]`). This sends an RPC message back to `aiosd` to merge or discard the changes.

### Feature 3: Command Palette Bridge (`Ctrl+Shift+P`)
Expose R-AI-OS CLI commands directly within VS Code:
- `R-AI-OS: Run Health Check`
- `R-AI-OS: Commit & Push (Intelligent)`
- `R-AI-OS: Dispatch Task to Agent...` (Opens an input box, sends to `aiosd` Router).
- `R-AI-OS: View Graphify Map` (Opens `graph.html` in a VS Code Webview).

### Feature 4: "Jump to Code" (TUI to IDE Bridge)
- **Mechanism:** When the user presses `[o]` (Open) on a file or an error line in the R-AI-OS TUI, `aiosd` sends an IPC signal to the extension.
- **Result:** VS Code immediately focuses the exact file and line number.

## 4. Technical Implementation Steps

### Step 1: Daemon Preparation (Rust)
- [ ] Implement a WebSocket/JSON-RPC server layer in `src/daemon/server.rs`.
- [ ] Create endpoints for `get_project_health`, `get_pending_diffs`, `approve_diff`, `reject_diff`.

### Step 2: Extension Scaffolding (TypeScript)
- [ ] Generate standard VS Code Extension (`yo code`).
- [ ] Setup IPC connection manager that reads the token from `~/.config/raios/.ipc_token`.
- [ ] Implement auto-reconnect logic if `aiosd` restarts.

### Step 3: UI Integration (VS Code API)
- [ ] Register Status Bar Item.
- [ ] Register Commands (`package.json` contributes).
- [ ] Implement the Custom Diff Viewer Provider utilizing VS Code's `vscode.diff` command.

## 5. Future Extensibility
- **Webview Dashboards:** Porting the Ratatui Dashboard to a rich React-based Webview inside VS Code for detailed memory and instinct management.
- **Cursor/Windsurf Compatibility:** Ensuring the extension works seamlessly alongside existing AI editors, acting as an orchestrator rather than a competitor.

---
*Status: Planned (Phase 9)* | *Approved via RBJ Cycle 001*