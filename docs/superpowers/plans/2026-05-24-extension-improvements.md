# VS Code Extension Improvements Plan (v0.2.0 → v0.3.0)

> **For agentic workers:** Use superpowers:executing-plans to implement task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Correctness fixes, UX polish, and targeted feature additions to the R-AI-OS VS Code extension. No sidebar/WebView — scope is deliberate.

**PowerShell env for all steps:**
```powershell
$env:PATH += ";$env:USERPROFILE\.cargo\bin;C:\Program Files\nodejs;C:\Users\turha\AppData\Roaming\npm;C:\Program Files\Git\bin"
cd "c:\Users\turha\Desktop\Dev_Ops_New\07_DevTools_&_Productivity\CLI_Tools\R-AI-OS"
```

---

## Tier 1 — Correctness (bugs and missing guard-rails)

### Task 1 — Fix duplicate `publisher` key in `package.json`

**File:** `vscode-extension/package.json`

Lines 3 and 7 both define `"publisher": "alazndy"`. Duplicate keys are silently ignored by JSON parsers but are invalid JSON and will cause `vsce` to warn.

- [ ] Remove the second `"publisher"` field (line 7)
- [ ] `vsce package --no-dependencies` — confirm no warnings
- [ ] Commit: `fix(extension): remove duplicate publisher key in package.json`

---

### Task 2 — DiagnosticProvider: scan saved file, not full project path

**File:** `vscode-extension/src/providers/DiagnosticProvider.ts`

**Current problem:** `onSave(filePath)` passes the saved file path directly to `raios security`. But `raios security` expects a directory, not a single file. Passing a single file works accidentally only if the file is in the current directory. On a deep nested file the path resolves to a parent directory and scans far too much.

**Fix:** When a single file is saved:
1. Pass the file's **parent directory** to `raios security` (scans only that folder)
2. After `applyDiagnostics`, filter the `DiagnosticCollection` to only update entries for files inside that directory — don't clear diagnostics for unrelated files

```typescript
// In onSave — derive parent dir
import * as path from "path";
const dir = path.dirname(filePath);
this.scanPath(dir);
```

And in `applyDiagnostics`, instead of `this.collection.clear()` (clears everything), merge by file:
```typescript
// Only set/clear files that appear in this scan's output
for (const [fp, diags] of byFile) {
  this.collection.set(vscode.Uri.file(fp), diags);
}
// Clear files in the scanned dir that are now clean
// (track previously seen files per dir)
```

- [ ] Add `private seenFiles = new Map<string, Set<string>>()` — keyed by scanned dir, value = set of file URIs with diagnostics
- [ ] In `applyDiagnostics(raw, scannedDir)` — update only files in scanned dir
- [ ] Pass `scannedDir` through from `onSave` → `scanPath` → `applyDiagnostics`
- [ ] Test: save a clean file → diagnostics for other files in project are unaffected
- [ ] Commit: `fix(extension): scope diagnostic updates to scanned directory on save`

---

### Task 3 — Add `raios.diagnosticsEnabled` setting

**File:** `vscode-extension/package.json` + `DiagnosticProvider.ts`

Heavy workspaces (large mono-repos) may not want on-save scans. Add opt-out.

- [ ] Add to `package.json` contributes.configuration:
  ```json
  "raios.diagnosticsEnabled": {
    "type": "boolean",
    "default": true,
    "description": "Run security scan on file save and show results in Problems panel"
  },
  "raios.diagnosticsDebounceMs": {
    "type": "number",
    "default": 800,
    "description": "Debounce delay (ms) before triggering on-save security scan"
  }
  ```
- [ ] In `DiagnosticProvider.onSave`: check `vscode.workspace.getConfiguration("raios").get<boolean>("diagnosticsEnabled", true)` before proceeding
- [ ] Read `diagnosticsDebounceMs` from config instead of hardcoded `800`
- [ ] Commit: `feat(extension): add diagnosticsEnabled and diagnosticsDebounceMs settings`

---

### Task 4 — StatusBarProvider: pipe CLI output to OutputChannel

**File:** `vscode-extension/src/providers/StatusBarProvider.ts`

Currently `StatusBarProvider` has its own `cp.execFile` call but doesn't write to the shared `OutputChannel`. Errors silently disappear.

- [ ] Pass `OutputChannel` into `StatusBarProvider` constructor
- [ ] In `refresh()` callback: `outputChannel.appendLine(...)` on both success and error paths
- [ ] Update `extension.ts` to pass `outputChannel` when constructing `StatusBarProvider`
- [ ] Commit: `fix(extension): pipe StatusBarProvider output to shared OutputChannel`

---

## Tier 2 — UX Polish

### Task 5 — "Scanning…" status bar indicator during diagnostic scan

**File:** `vscode-extension/src/providers/DiagnosticProvider.ts`

Users get no feedback that a scan is in progress. Add a transient status bar item.

- [ ] In `scanPath()`, before spawning: `vscode.window.setStatusBarMessage("$(sync~spin) R-AI-OS scanning…", new Promise(r => child.on("close", r)))`
  - `setStatusBarMessage` accepts a `Thenable` — it auto-clears when the process closes
- [ ] Commit: `feat(extension): show scanning spinner in status bar during diagnostic scan`

---

### Task 6 — `raios.scanCurrentFile` command + file context menu

**File:** `vscode-extension/src/commands/CommandBridge.ts` + `package.json`

Right now context menu only works on folders. Add a command to scan the currently active file (or right-clicked file).

- [ ] Register `raios.scanCurrentFile` command:
  ```typescript
  vscode.commands.registerCommand("raios.scanCurrentFile", (uri?: vscode.Uri) => {
    const target = uri?.fsPath ?? vscode.window.activeTextEditor?.document.uri.fsPath;
    if (!target) return;
    diagnosticProvider.scanPath(path.dirname(target));
  });
  ```
  - Note: `DiagnosticProvider` instance needs to be accessible — pass via constructor or make `scanPath` public and share the instance
- [ ] Add to `package.json` commands:
  ```json
  { "command": "raios.scanCurrentFile", "title": "R-AI-OS: Scan This File" }
  ```
- [ ] Add to `package.json` menus:
  ```json
  "editor/context": [
    { "command": "raios.scanCurrentFile", "when": "editorIsOpen", "group": "raios@1" }
  ],
  "explorer/context": [
    { "command": "raios.scanCurrentFile", "when": "!explorerResourceIsFolder", "group": "raios@1" }
  ]
  ```
- [ ] Commit: `feat(extension): add raios.scanCurrentFile command with editor/explorer context menu`

---

### Task 7 — Health check result shown in OutputChannel

**File:** `vscode-extension/src/commands/CommandBridge.ts`

`raios.healthCheck` currently shows only a toast notification "Health check complete". The actual health output (score, grade, issues) goes nowhere visible.

- [ ] For `healthCheck` specifically, use `--json` flag and pretty-print the result to `outputChannel`
- [ ] After printing, `outputChannel.show(true)` — bring the channel into view without stealing focus
- [ ] Same pattern for `licenseCheck`
- [ ] Commit: `feat(extension): show health and license results in OutputChannel`

---

### Task 8 — DiffInboxProvider: skip polling when daemon disconnected

**File:** `vscode-extension/src/providers/DiffInboxProvider.ts`

The 15-second poll timer fires regardless of daemon connection state, generating silent no-ops and wasted IPC writes.

- [ ] Pass `DaemonClient` connection state check into poll:
  ```typescript
  private poll(): void {
    if (!this.client.isConnected) return;
    this.client.sendRaw({ command: "GetPendingDiffs" });
  }
  ```
- [ ] Add `get isConnected(): boolean` getter to `DaemonClient`
- [ ] Commit: `fix(extension): skip DiffInboxProvider poll when daemon disconnected`

---

## Tier 3 — Feature additions

### Task 9 — Keybindings

**File:** `vscode-extension/package.json`

- [ ] Add to `contributes`:
  ```json
  "keybindings": [
    {
      "command": "raios.securityScan",
      "key": "ctrl+shift+r s",
      "mac": "cmd+shift+r s"
    },
    {
      "command": "raios.healthCheck",
      "key": "ctrl+shift+r h",
      "mac": "cmd+shift+r h"
    },
    {
      "command": "raios.scanCurrentFile",
      "key": "ctrl+shift+r f",
      "mac": "cmd+shift+r f"
    }
  ]
  ```
- [ ] Commit: `feat(extension): add keyboard shortcuts for common raios commands`

---

### Task 10 — `raios.openMemory` command

**File:** `vscode-extension/src/commands/CommandBridge.ts` + `package.json`

Open the current project's `memory.md` in the editor. Simple but high daily-use value.

- [ ] Register command:
  ```typescript
  vscode.commands.registerCommand("raios.openMemory", async () => {
    const projectPath = this.currentProjectPath();
    if (!projectPath) return;
    const memPath = path.join(projectPath, "memory.md");
    try {
      await vscode.window.showTextDocument(vscode.Uri.file(memPath));
    } catch {
      vscode.window.showWarningMessage("R-AI-OS: memory.md not found in this project");
    }
  });
  ```
- [ ] Add to `package.json` commands
- [ ] Commit: `feat(extension): add raios.openMemory command to open project memory.md`

---

## Final step — Build, package, push

- [ ] `node .\node_modules\typescript\bin\tsc -p ./` — confirm 0 errors
- [ ] `vsce package --no-dependencies` — produces `raios-0.3.0.vsix`
- [ ] `Copy-Item raios-0.3.0.vsix raios-latest.vsix`
- [ ] Bump `package.json` version to `0.3.0`
- [ ] `cargo clippy -- -D warnings` on Rust side — confirm still clean
- [ ] `git push origin master`

---

## Summary table

| Task | Tier | File(s) | Effort |
|------|------|---------|--------|
| 1 — Fix duplicate publisher | Bug | package.json | 1 line |
| 2 — Scope diagnostics to dir | Bug | DiagnosticProvider.ts | ~20 lines |
| 3 — diagnosticsEnabled setting | Guard-rail | package.json + DiagnosticProvider.ts | ~15 lines |
| 4 — StatusBar → OutputChannel | Bug | StatusBarProvider.ts + extension.ts | ~10 lines |
| 5 — Scanning spinner | UX | DiagnosticProvider.ts | 3 lines |
| 6 — scanCurrentFile command | UX | CommandBridge.ts + package.json | ~20 lines |
| 7 — Health/license in channel | UX | CommandBridge.ts | ~20 lines |
| 8 — Skip poll when disconnected | Bug | DiffInboxProvider.ts + DaemonClient.ts | ~10 lines |
| 9 — Keybindings | Feature | package.json | 12 lines |
| 10 — openMemory command | Feature | CommandBridge.ts + package.json | ~15 lines |
