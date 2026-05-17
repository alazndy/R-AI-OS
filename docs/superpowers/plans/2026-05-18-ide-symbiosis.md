# Phase 9: IDE Symbiosis — VS Code Extension Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** R-AI-OS aiosd daemon'ını VS Code'a entegre eden "Thin Client" extension — Status Bar, Command Palette Bridge, Diff Inbox Overlay, Jump-to-Code.

**Architecture:** Extension, mevcut `aiosd` TCP IPC (`127.0.0.1:42069`, token auth, newline JSON-RPC) üzerinden konuşur. Sıfır AI logic extension'da. İki aşama: **9A (daemon dokunmadan MVP)** → **9B (tam entegrasyon)**.

**Tech Stack:** TypeScript, VS Code Extension API, Node.js `net` module (TCP), `child_process` (CLI bridge). Rust/aiosd tarafında Phase 9B için minimal endpoint ekleme.

---

## Mevcut Daemon Mimarisi (okumadan önce bil)

```
aiosd → TcpListener → 127.0.0.1:42069
  Auth:  client gönderir: "AUTH <token>\n"
  Token: ~/.config/raios/.ipc_token (Config::config_file().parent()/.ipc_token)
  Proto: newline-delimited JSON, her mesaj \n ile biter
  Push:  daemon broadcast → FileChanged, ActivePorts, HealthUpdate
  RPC:   client gönderir: {"method":"get_health","params":{"project":"R-AI-OS"}}
```

---

## File Map

### Phase 9A — Extension (yeni repo/klasör)

```
vscode-extension/
├── package.json              # Extension manifest
├── tsconfig.json
├── src/
│   ├── extension.ts          # Giriş noktası — activate/deactivate
│   ├── ipc/
│   │   └── DaemonClient.ts   # TCP bağlantı yöneticisi
│   ├── providers/
│   │   └── StatusBarProvider.ts  # Status bar item
│   └── commands/
│       └── CommandBridge.ts  # Command Palette bridge
└── .vscodeignore
```

### Phase 9B — Daemon (mevcut repo)

```
src/daemon/server.rs          # get_pending_diffs + approve_diff + reject_diff endpoint
vscode-extension/src/
├── providers/
│   └── DiffInboxProvider.ts  # Diff Inbox Overlay
└── bridge/
    └── JumpToCode.ts         # TUI → IDE bridge
```

---

## PHASE 9A: MVP (Daemon değişikliği yok)

---

## Task 1: Extension Scaffold + IPC Connection Manager

**Files:**
- Create: `vscode-extension/package.json`
- Create: `vscode-extension/tsconfig.json`
- Create: `vscode-extension/src/extension.ts`
- Create: `vscode-extension/src/ipc/DaemonClient.ts`

- [ ] **Step 1.1: Extension klasörü oluştur**

```bash
mkdir -p vscode-extension/src/ipc vscode-extension/src/providers vscode-extension/src/commands
```

- [ ] **Step 1.2: `package.json` oluştur**

```json
{
  "name": "raios",
  "displayName": "R-AI-OS",
  "description": "R-AI-OS Kernel integration for VS Code",
  "version": "0.1.0",
  "engines": { "vscode": "^1.85.0" },
  "categories": ["Other"],
  "activationEvents": ["onStartupFinished"],
  "main": "./out/extension.js",
  "contributes": {
    "commands": [
      {
        "command": "raios.healthCheck",
        "title": "R-AI-OS: Run Health Check"
      },
      {
        "command": "raios.commitPush",
        "title": "R-AI-OS: Commit & Push (Intelligent)"
      },
      {
        "command": "raios.dispatchTask",
        "title": "R-AI-OS: Dispatch Task to Agent..."
      },
      {
        "command": "raios.cortexIndex",
        "title": "R-AI-OS: Re-index Cortex"
      },
      {
        "command": "raios.securityScan",
        "title": "R-AI-OS: Security Scan"
      }
    ],
    "configuration": {
      "title": "R-AI-OS",
      "properties": {
        "raios.daemonPort": {
          "type": "number",
          "default": 42069,
          "description": "aiosd daemon port"
        },
        "raios.pollInterval": {
          "type": "number",
          "default": 30,
          "description": "Status bar refresh interval (seconds)"
        }
      }
    }
  },
  "scripts": {
    "compile": "tsc -p ./",
    "watch": "tsc -watch -p ./",
    "package": "vsce package"
  },
  "devDependencies": {
    "@types/node": "^20.0.0",
    "@types/vscode": "^1.85.0",
    "typescript": "^5.3.0"
  }
}
```

- [ ] **Step 1.3: `tsconfig.json` oluştur**

```json
{
  "compilerOptions": {
    "module": "commonjs",
    "target": "ES2020",
    "lib": ["ES2020"],
    "outDir": "./out",
    "rootDir": "./src",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true
  },
  "include": ["src"],
  "exclude": ["node_modules", ".vscode-test"]
}
```

- [ ] **Step 1.4: `DaemonClient.ts` oluştur**

```typescript
// vscode-extension/src/ipc/DaemonClient.ts
import * as net from "net";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";

type MessageHandler = (msg: Record<string, unknown>) => void;

export class DaemonClient {
  private socket: net.Socket | null = null;
  private buffer = "";
  private handlers: MessageHandler[] = [];
  private connected = false;
  private reconnectTimer: NodeJS.Timeout | null = null;

  constructor(
    private readonly port: number = 42069,
    private readonly host: string = "127.0.0.1"
  ) {}

  onMessage(handler: MessageHandler): void {
    this.handlers.push(handler);
  }

  connect(): void {
    if (this.connected) return;

    const token = this.readToken();
    if (!token) {
      console.error("[R-AI-OS] IPC token not found — is aiosd running?");
      this.scheduleReconnect();
      return;
    }

    this.socket = new net.Socket();

    this.socket.connect(this.port, this.host, () => {
      this.connected = true;
      // Auth handshake
      this.socket!.write(`AUTH ${token}\n`);
      console.log("[R-AI-OS] Connected to aiosd");
    });

    this.socket.on("data", (data: Buffer) => {
      this.buffer += data.toString();
      const lines = this.buffer.split("\n");
      this.buffer = lines.pop() ?? "";
      for (const line of lines) {
        if (!line.trim()) continue;
        try {
          const msg = JSON.parse(line) as Record<string, unknown>;
          this.handlers.forEach((h) => h(msg));
        } catch {
          // Ignore malformed JSON
        }
      }
    });

    this.socket.on("error", (err) => {
      console.error("[R-AI-OS] IPC error:", err.message);
    });

    this.socket.on("close", () => {
      this.connected = false;
      this.socket = null;
      this.scheduleReconnect();
    });
  }

  send(method: string, params: Record<string, unknown> = {}): void {
    if (!this.socket || !this.connected) return;
    const msg = JSON.stringify({ jsonrpc: "2.0", id: Date.now(), method, params });
    this.socket.write(msg + "\n");
  }

  disconnect(): void {
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.socket?.destroy();
    this.connected = false;
  }

  private readToken(): string | null {
    // ~/.config/raios/.ipc_token (Windows: %APPDATA%\raios\.ipc_token)
    const configDir = path.join(os.homedir(), "AppData", "Roaming", "raios");
    const tokenPath = path.join(configDir, ".ipc_token");
    try {
      return fs.readFileSync(tokenPath, "utf-8").trim();
    } catch {
      // Fallback: ~/.config/raios/.ipc_token (Linux/Mac)
      const altPath = path.join(os.homedir(), ".config", "raios", ".ipc_token");
      try {
        return fs.readFileSync(altPath, "utf-8").trim();
      } catch {
        return null;
      }
    }
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimer) return;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, 5000);
  }
}
```

- [ ] **Step 1.5: `extension.ts` iskeletini oluştur**

```typescript
// vscode-extension/src/extension.ts
import * as vscode from "vscode";
import { DaemonClient } from "./ipc/DaemonClient";
import { StatusBarProvider } from "./providers/StatusBarProvider";
import { CommandBridge } from "./commands/CommandBridge";

let client: DaemonClient;
let statusBar: StatusBarProvider;

export function activate(context: vscode.ExtensionContext): void {
  const config = vscode.workspace.getConfiguration("raios");
  const port = config.get<number>("daemonPort", 42069);
  const pollInterval = config.get<number>("pollInterval", 30);

  client = new DaemonClient(port);
  statusBar = new StatusBarProvider(client, pollInterval);
  const bridge = new CommandBridge(client);

  statusBar.activate(context);
  bridge.register(context);
  client.connect();

  context.subscriptions.push({ dispose: () => client.disconnect() });

  console.log("[R-AI-OS] Extension activated");
}

export function deactivate(): void {
  client?.disconnect();
}
```

- [ ] **Step 1.6: npm install ve compile**

```bash
cd vscode-extension && npm install && npm run compile
```

Beklenen: `out/` klasörü oluştu, hata yok.

- [ ] **Step 1.7: Commit**

```bash
git add vscode-extension/
git commit -m "feat(vscode): extension scaffold + DaemonClient TCP IPC"
```

---

## Task 2: Status Bar Provider

**Files:**
- Create: `vscode-extension/src/providers/StatusBarProvider.ts`

- [ ] **Step 2.1: `StatusBarProvider.ts` oluştur**

```typescript
// vscode-extension/src/providers/StatusBarProvider.ts
import * as vscode from "vscode";
import * as cp from "child_process";
import { DaemonClient } from "../ipc/DaemonClient";

interface HealthData {
  name: string;
  compliance_score?: number;
  compliance_grade: string;
  security_grade?: string;
  git_dirty?: boolean;
}

export class StatusBarProvider {
  private item: vscode.StatusBarItem;
  private timer: NodeJS.Timeout | null = null;

  constructor(
    private readonly client: DaemonClient,
    private readonly pollIntervalSecs: number
  ) {
    this.item = vscode.window.createStatusBarItem(
      vscode.StatusBarAlignment.Left,
      100
    );
    this.item.command = "raios.healthCheck";
    this.item.tooltip = "R-AI-OS — Click to run health check";
  }

  activate(context: vscode.ExtensionContext): void {
    context.subscriptions.push(this.item);
    this.item.show();
    this.refresh();

    // Poll on interval
    this.timer = setInterval(() => this.refresh(), this.pollIntervalSecs * 1000);

    // Also update on daemon push events
    this.client.onMessage((msg) => {
      if (msg["event"] === "HealthUpdate") {
        this.refresh();
      }
    });
  }

  private refresh(): void {
    const projectName = this.currentProjectName();
    if (!projectName) {
      this.item.text = "$(circle-slash) R-AI-OS";
      return;
    }

    // Use CLI for simplicity — no daemon endpoint required
    cp.exec(
      `raios --json health "${projectName}"`,
      { timeout: 10000 },
      (err, stdout) => {
        if (err || !stdout.trim()) {
          this.item.text = "$(warning) R-AI-OS";
          return;
        }
        try {
          const data = JSON.parse(stdout) as HealthData[];
          const h = data[0];
          if (!h) return;

          const score = h.compliance_score ?? "?";
          const grade = h.compliance_grade;
          const dirty = h.git_dirty ? " $(git-commit)" : "";
          const icon = grade === "A" ? "$(check)" : grade === "B" ? "$(info)" : "$(warning)";
          this.item.text = `${icon} R-AI-OS ${score}/100 (${grade})${dirty}`;
        } catch {
          this.item.text = "$(warning) R-AI-OS";
        }
      }
    );
  }

  private currentProjectName(): string | null {
    const folders = vscode.workspace.workspaceFolders;
    if (!folders || folders.length === 0) return null;
    return folders[0].name;
  }

  dispose(): void {
    if (this.timer) clearInterval(this.timer);
    this.item.dispose();
  }
}
```

- [ ] **Step 2.2: compile**

```bash
cd vscode-extension && npm run compile 2>&1
```

Beklenen: hata yok.

- [ ] **Step 2.3: Commit**

```bash
git add vscode-extension/src/providers/StatusBarProvider.ts vscode-extension/out/
git commit -m "feat(vscode): status bar with health polling"
```

---

## Task 3: Command Palette Bridge

**Files:**
- Create: `vscode-extension/src/commands/CommandBridge.ts`

- [ ] **Step 3.1: `CommandBridge.ts` oluştur**

```typescript
// vscode-extension/src/commands/CommandBridge.ts
import * as vscode from "vscode";
import * as cp from "child_process";
import { DaemonClient } from "../ipc/DaemonClient";

export class CommandBridge {
  constructor(private readonly client: DaemonClient) {}

  register(context: vscode.ExtensionContext): void {
    context.subscriptions.push(
      vscode.commands.registerCommand("raios.healthCheck", () =>
        this.runCli("health", [], "Health check complete")
      ),
      vscode.commands.registerCommand("raios.commitPush", () =>
        this.commitPushFlow()
      ),
      vscode.commands.registerCommand("raios.dispatchTask", () =>
        this.dispatchTask()
      ),
      vscode.commands.registerCommand("raios.cortexIndex", () =>
        this.runCli("cortex-index", [], "Cortex indexed")
      ),
      vscode.commands.registerCommand("raios.securityScan", () =>
        this.runCli("security", ["."], "Security scan complete")
      )
    );
  }

  private runCli(
    subcommand: string,
    args: string[],
    successMsg: string
  ): void {
    const projectPath = this.currentProjectPath();
    const cmd = ["raios", subcommand, ...args].join(" ");

    vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: `R-AI-OS: ${subcommand}…`,
        cancellable: false,
      },
      () =>
        new Promise<void>((resolve) => {
          cp.exec(
            cmd,
            { cwd: projectPath ?? undefined, timeout: 60000 },
            (err, stdout, stderr) => {
              if (err) {
                vscode.window.showErrorMessage(
                  `R-AI-OS error: ${stderr || err.message}`
                );
              } else {
                vscode.window.showInformationMessage(
                  `R-AI-OS: ${successMsg}`
                );
              }
              resolve();
            }
          );
        })
    );
  }

  private async commitPushFlow(): Promise<void> {
    const msg = await vscode.window.showInputBox({
      prompt: "Commit message (leave empty for auto)",
      placeHolder: "chore: raios auto-sync",
    });

    if (msg === undefined) return; // cancelled

    const args = msg ? ["--message", `"${msg}"`, "--push"] : ["--push"];
    this.runCli("commit", args, "Committed & pushed");
  }

  private async dispatchTask(): Promise<void> {
    const task = await vscode.window.showInputBox({
      prompt: "Task description for agent router",
      placeHolder: "Fix the auth bug in login flow",
    });

    if (!task) return;

    const projectPath = this.currentProjectPath();
    const cmd = `raios task "${task}"`;

    const terminal = vscode.window.createTerminal("R-AI-OS Task");
    terminal.show();
    if (projectPath) terminal.sendText(`cd "${projectPath}"`);
    terminal.sendText(cmd);
  }

  private currentProjectPath(): string | null {
    const folders = vscode.workspace.workspaceFolders;
    return folders?.[0]?.uri.fsPath ?? null;
  }
}
```

- [ ] **Step 3.2: compile + test**

```bash
cd vscode-extension && npm run compile
```

- [ ] **Step 3.3: Extension'ı VS Code'da test et**

`F5` (VS Code Extension Development Host) ile aç. Kontrol et:
- Status Bar görünüyor mu?
- `Ctrl+Shift+P` → `R-AI-OS:` komutları listede mi?
- `R-AI-OS: Run Health Check` çalışıyor mu?

- [ ] **Step 3.4: Commit**

```bash
git add vscode-extension/
git commit -m "feat(vscode): command palette bridge — health, commit, task, security"
```

---

## PHASE 9B: Tam Entegrasyon

---

## Task 4: Daemon — Diff Endpoints

**Files:**
- Modify: `src/daemon/server.rs`
- Modify: `src/daemon/state.rs`

- [ ] **Step 4.1: `DaemonState`'e diff queue ekle**

`src/daemon/state.rs`'de `DaemonState` struct'ına ekle:

```rust
use std::collections::VecDeque;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PendingDiff {
    pub id: String,           // UUID
    pub project: String,
    pub file_path: String,
    pub original: String,     // base64 encoded original content
    pub proposed: String,     // base64 encoded proposed content
    pub agent: String,
    pub description: String,
    pub created_at: String,   // ISO8601
}

// DaemonState içine ekle:
pub pending_diffs: VecDeque<PendingDiff>,
```

`DaemonState::new()` veya `Default`'a `pending_diffs: VecDeque::new()` ekle.

- [ ] **Step 4.2: `server.rs`'de `get_pending_diffs` endpoint ekle**

Mevcut RPC match bloğuna (genellikle `match method.as_str()`) ekle:

```rust
"get_pending_diffs" => {
    let state = state_for_client.read().await;
    let diffs: Vec<&PendingDiff> = state.pending_diffs.iter().collect();
    let result = serde_json::json!({ "diffs": diffs });
    RpcResponse::ok(id, result)
}

"approve_diff" => {
    let diff_id = params["id"].as_str().unwrap_or("").to_string();
    let mut state = state_for_client.write().await;
    if let Some(pos) = state.pending_diffs.iter().position(|d| d.id == diff_id) {
        let diff = state.pending_diffs.remove(pos).unwrap();
        // Apply: write proposed content back to file
        if let Ok(content) = base64_decode(&diff.proposed) {
            let _ = std::fs::write(&diff.file_path, content);
        }
        RpcResponse::ok(id, serde_json::json!({"status": "approved", "id": diff_id}))
    } else {
        RpcResponse::err(id, -32602, format!("diff {} not found", diff_id))
    }
}

"reject_diff" => {
    let diff_id = params["id"].as_str().unwrap_or("").to_string();
    let mut state = state_for_client.write().await;
    state.pending_diffs.retain(|d| d.id != diff_id);
    RpcResponse::ok(id, serde_json::json!({"status": "rejected", "id": diff_id}))
}
```

Helper ekle:
```rust
fn base64_decode(s: &str) -> anyhow::Result<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).map_err(Into::into)
}
```

- [ ] **Step 4.3: `base64` crate ekle**

`Cargo.toml`'a:
```toml
base64 = "0.22"
```

- [ ] **Step 4.4: `cargo check`**

```bash
cargo check 2>&1 | head -20
```

- [ ] **Step 4.5: Commit**

```bash
git add src/daemon/ Cargo.toml Cargo.lock
git commit -m "feat(daemon): get_pending_diffs + approve_diff + reject_diff endpoints"
```

---

## Task 5: Diff Inbox Overlay (VS Code)

**Files:**
- Create: `vscode-extension/src/providers/DiffInboxProvider.ts`
- Modify: `vscode-extension/src/extension.ts`

- [ ] **Step 5.1: `DiffInboxProvider.ts` oluştur**

```typescript
// vscode-extension/src/providers/DiffInboxProvider.ts
import * as vscode from "vscode";
import * as os from "os";
import * as fs from "fs";
import * as path from "path";
import { DaemonClient } from "../ipc/DaemonClient";

interface PendingDiff {
  id: string;
  project: string;
  file_path: string;
  original: string;   // base64
  proposed: string;   // base64
  agent: string;
  description: string;
}

export class DiffInboxProvider {
  constructor(private readonly client: DaemonClient) {}

  activate(context: vscode.ExtensionContext): void {
    // Poll for pending diffs every 10 seconds
    const timer = setInterval(() => this.checkPendingDiffs(), 10000);
    context.subscriptions.push({ dispose: () => clearInterval(timer) });

    // Also react to daemon push
    this.client.onMessage((msg) => {
      if (msg["event"] === "PendingDiff") {
        void this.showDiffNotification(msg["diff"] as PendingDiff);
      }
    });
  }

  private async checkPendingDiffs(): Promise<void> {
    this.client.send("get_pending_diffs");
  }

  async showDiffNotification(diff: PendingDiff): Promise<void> {
    const action = await vscode.window.showInformationMessage(
      `R-AI-OS (${diff.agent}): ${diff.description} — ${diff.file_path}`,
      "Review",
      "Reject"
    );

    if (action === "Review") {
      await this.openDiffEditor(diff);
    } else if (action === "Reject") {
      this.client.send("reject_diff", { id: diff.id });
      vscode.window.showInformationMessage(`R-AI-OS: Diff rejected`);
    }
  }

  private async openDiffEditor(diff: PendingDiff): Promise<void> {
    const original = Buffer.from(diff.original, "base64").toString("utf-8");
    const proposed = Buffer.from(diff.proposed, "base64").toString("utf-8");

    const tmpDir = os.tmpdir();
    const origPath = path.join(tmpDir, `raios-orig-${diff.id}.tmp`);
    const propPath = path.join(tmpDir, `raios-prop-${diff.id}.tmp`);

    fs.writeFileSync(origPath, original);
    fs.writeFileSync(propPath, proposed);

    const origUri = vscode.Uri.file(origPath);
    const propUri = vscode.Uri.file(propPath);

    await vscode.commands.executeCommand(
      "vscode.diff",
      origUri,
      propUri,
      `R-AI-OS Diff: ${path.basename(diff.file_path)} (${diff.agent})`
    );

    // Approve/Reject buttons in notification
    const decision = await vscode.window.showInformationMessage(
      `Accept changes from ${diff.agent}?`,
      "Accept",
      "Reject"
    );

    fs.unlinkSync(origPath);
    fs.unlinkSync(propPath);

    if (decision === "Accept") {
      this.client.send("approve_diff", { id: diff.id });
      vscode.window.showInformationMessage(
        `R-AI-OS: Changes applied to ${diff.file_path}`
      );
    } else {
      this.client.send("reject_diff", { id: diff.id });
    }
  }
}
```

- [ ] **Step 5.2: `extension.ts`'e DiffInboxProvider ekle**

```typescript
import { DiffInboxProvider } from "./providers/DiffInboxProvider";
// activate() içine:
const diffInbox = new DiffInboxProvider(client);
diffInbox.activate(context);
```

- [ ] **Step 5.3: compile**

```bash
cd vscode-extension && npm run compile
```

- [ ] **Step 5.4: Commit**

```bash
git add vscode-extension/
git commit -m "feat(vscode): diff inbox overlay with approve/reject"
```

---

## Task 6: Jump to Code Bridge (TUI → IDE)

**Files:**
- Modify: `src/daemon/server.rs` (yeni event type)
- Create: `vscode-extension/src/bridge/JumpToCode.ts`
- Modify: `vscode-extension/src/extension.ts`

- [ ] **Step 6.1: `JumpToCode.ts` oluştur**

```typescript
// vscode-extension/src/bridge/JumpToCode.ts
import * as vscode from "vscode";
import { DaemonClient } from "../ipc/DaemonClient";

export class JumpToCode {
  constructor(private readonly client: DaemonClient) {}

  activate(): void {
    this.client.onMessage((msg) => {
      if (msg["event"] !== "OpenFile") return;

      const filePath = msg["path"] as string;
      const line = (msg["line"] as number) ?? 1;
      const col = (msg["col"] as number) ?? 1;

      if (!filePath) return;

      const uri = vscode.Uri.file(filePath);
      const pos = new vscode.Position(Math.max(0, line - 1), Math.max(0, col - 1));

      void vscode.window.showTextDocument(uri, {
        selection: new vscode.Range(pos, pos),
        preserveFocus: false,
      });
    });
  }
}
```

- [ ] **Step 6.2: `extension.ts`'e JumpToCode ekle**

```typescript
import { JumpToCode } from "./bridge/JumpToCode";
// activate() içine:
const jumpToCode = new JumpToCode(client);
jumpToCode.activate();
```

- [ ] **Step 6.3: TUI `[o]` kısayolunu `OpenFile` event göndermek üzere ayarla**

`src/app/events/keyboard.rs`'de `[o]` veya `[O]` tuşu için daemon'a mesaj gönder:

```rust
// Mevcut [o] handler'ına veya yeni bir yere:
KeyCode::Char('o') => {
    if let Some(selected_file) = app.selected_file_path() {
        let msg = serde_json::json!({
            "event": "OpenFile",
            "path": selected_file,
            "line": app.selected_line().unwrap_or(1)
        });
        // Broadcast to all connected clients
        if let Some(ref tx) = app.broadcast_tx {
            let _ = tx.send(msg.to_string());
        }
    }
}
```

Not: `app.broadcast_tx` mevcut state'e eklenmelidir. Yoksa `aiosd` IPC üzerinden gönderilmeli.

- [ ] **Step 6.4: compile (Rust + TS)**

```bash
cargo check && cd vscode-extension && npm run compile
```

- [ ] **Step 6.5: Commit**

```bash
git add src/ vscode-extension/
git commit -m "feat(vscode,tui): jump-to-code bridge — TUI [o] opens file in VS Code"
```

---

## Task 7: Package & Yayın

**Files:**
- Create: `vscode-extension/.vscodeignore`
- Create: `vscode-extension/README.md`

- [ ] **Step 7.1: `.vscodeignore` oluştur**

```
.vscode/**
src/**
tsconfig.json
node_modules/**
.gitignore
```

- [ ] **Step 7.2: Extension'ı paketle**

```bash
cd vscode-extension && npx vsce package
```

Beklenen: `raios-0.1.0.vsix` oluşur.

- [ ] **Step 7.3: Local install test**

```bash
code --install-extension raios-0.1.0.vsix
```

VS Code'u aç, status bar'da R-AI-OS görünüyor mu?

- [ ] **Step 7.4: Final commit**

```bash
git add vscode-extension/
git commit -m "feat(vscode): package v0.1.0 — IDE Symbiosis Phase 9 complete"
```

---

## Özet

### Phase 9A (Daemon değişikliği yok) ✅
| Task | Özellik |
|------|---------|
| Task 1 | Scaffold + TCP DaemonClient |
| Task 2 | Status Bar (polling) |
| Task 3 | Command Palette Bridge |

### Phase 9B (Daemon extension gerekli) ✅
| Task | Özellik |
|------|---------|
| Task 4 | Daemon diff endpoints |
| Task 5 | Diff Inbox Overlay |
| Task 6 | Jump-to-Code bridge |
| Task 7 | Package & deploy |

### IPC Protokol Özeti
```
Client → Daemon: AUTH <token>\n
Daemon → Client: {"event":"AuthOk"}\n
Client → Daemon: {"method":"get_pending_diffs","params":{}}\n
Daemon → Client: {"event":"PendingDiff","diff":{...}}\n
Client → Daemon: {"method":"approve_diff","params":{"id":"uuid"}}\n
Daemon → Client: {"event":"OpenFile","path":"/abs/path","line":42}\n
```
