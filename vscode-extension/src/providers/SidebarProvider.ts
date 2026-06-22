import * as vscode from "vscode";
import { TokenBridge } from "../ipc/TokenBridge";
import { DaemonManager } from "../ipc/DaemonManager";

export class SidebarProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "raios.sidebar";
  private _view?: vscode.WebviewView;
  private _daemonManager?: DaemonManager;

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly tokenBridge: TokenBridge,
    private readonly outputChannel: vscode.OutputChannel
  ) {}

  public setDaemonManager(dm: DaemonManager): void {
    this._daemonManager = dm;
  }

  public resolveWebviewView(
    webviewView: vscode.WebviewView,
    _context: vscode.WebviewViewResolveContext,
    _token: vscode.CancellationToken
  ): void {
    this._view = webviewView;

    webviewView.webview.options = {
      enableScripts: true,
      localResourceRoots: [this.context.extensionUri]
    };

    webviewView.webview.html = this.getHtmlContent(webviewView.webview);

    webviewView.webview.onDidReceiveMessage(async (message) => {
      if (message.type === "runBuild") {
        const cwd = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
        const term = vscode.window.createTerminal({ name: "R-AI-OS: Build", cwd });
        term.sendText("cargo build");
        term.show();
        return;
      }
      if (message.type === "runTest") {
        const cwd = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
        const term = vscode.window.createTerminal({ name: "R-AI-OS: Test", cwd });
        term.sendText("cargo test");
        term.show();
        return;
      }
      if (message.type === "toggleTask") {
        const { taskId, completed } = message;
        if (!taskId) { return; }
        const status = completed ? "completed" : "pending";
        const cwd = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
        const term = vscode.window.createTerminal({ name: "R-AI-OS: Task", cwd, hideFromUser: true });
        term.sendText(`raios task-update "${taskId}" --status ${status} && exit`);
        // refresh sidebar after a short delay so DB write settles
        setTimeout(() => this.triggerRefresh(), 1200);
        return;
      }
      if (message.type === "startDaemon") {
        const dm = this._daemonManager;
        if (!dm) { return; }
        webviewView.webview.postMessage({ type: "daemonSpawning" });
        const ok = await dm.spawn();
        webviewView.webview.postMessage({ type: ok ? "refresh" : "daemonFailed" });
        return;
      }
      await this.tokenBridge.handleMessage(message, webviewView.webview);
    });

    webviewView.onDidChangeVisibility(() => {
      if (webviewView.visible) {
        this.triggerRefresh();
      }
    });

    this.triggerRefresh();
  }

  public triggerRefresh(): void {
    if (this._view) {
      const wsPath = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? "";
      this._view.webview.postMessage({ type: "refresh", workspacePath: wsPath });
    }
  }

  private getHtmlContent(webview: vscode.Webview): string {
    const cspSource = webview.cspSource;
    const styleUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this.context.extensionUri, "media", "sidebar.css")
    );
    const scriptUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this.context.extensionUri, "media", "sidebar.js")
    );

    return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${cspSource} https://fonts.googleapis.com; font-src https://fonts.gstatic.com; script-src ${cspSource}; connect-src 'none';">
  <link href="https://fonts.googleapis.com/css2?family=Geist:wght@300;400;500;600;700&display=swap" rel="stylesheet">
  <link href="${styleUri}" rel="stylesheet">
</head>
<body>
  <div class="header">
    <div class="title">R-AI-OS</div>
    <div id="status-badge" class="status-badge">
      <span id="status-dot" class="status-dot status-disconnected"></span>
      <span id="status-text">Connecting...</span>
    </div>
  </div>

  <div id="approval-box" style="display: none;">
    <div class="approval-alert">⚠️ Action Required: Approval pending.</div>
  </div>

  <div id="offline-box" style="display: none;">
    <div class="approval-alert" style="background:rgba(239,68,68,0.08);border-color:rgba(239,68,68,0.25);color:var(--error-color);">
      Daemon not running.
      <button id="launch-btn" class="btn" style="margin-top:8px;padding:6px 14px;font-size:11px;">
        Launch Daemon
      </button>
    </div>
  </div>

  <!-- Git Status -->
  <div class="card">
    <div class="card-title">
      <span>Git Status</span>
      <span id="git-branch-label" style="font-size:10px;color:var(--text-muted);font-family:monospace;"></span>
    </div>
    <div id="git-status-body"><div class="empty-state">Loading...</div></div>
  </div>

  <!-- Plans -->
  <div class="card">
    <div class="card-title">
      <span>Plans</span>
      <span id="plans-count" style="font-size:10px; color: var(--text-muted);"></span>
    </div>
    <div id="plans-list"><div class="empty-state">Loading...</div></div>
  </div>

  <!-- Tasks -->
  <div class="card">
    <div class="card-title">Tasks</div>
    <div id="tasks-list"><div class="empty-state">Loading...</div></div>
  </div>

  <!-- Swarm -->
  <div class="card">
    <div class="card-title">
      <span>Swarm</span>
      <span id="swarm-count" style="font-size:10px;color:var(--text-muted);"></span>
    </div>
    <div id="swarm-list"><div class="empty-state">Loading...</div></div>
  </div>

  <!-- Quick Actions -->
  <div class="quick-actions">
    <button id="btn-build" class="btn-action">Build</button>
    <button id="btn-test" class="btn-action">Test</button>
  </div>

  <div>
    <button id="refresh-btn" class="btn btn-secondary">Refresh</button>
  </div>

  <script src="${scriptUri}"></script>
</body>
</html>`;
  }
}
