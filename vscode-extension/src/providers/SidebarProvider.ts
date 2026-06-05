import * as vscode from "vscode";
import { TokenBridge } from "../ipc/TokenBridge";

export class SidebarProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "raios.sidebar";
  private _view?: vscode.WebviewView;

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly tokenBridge: TokenBridge,
    private readonly outputChannel: vscode.OutputChannel
  ) {}

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

    // Handle messages from the webview
    webviewView.webview.onDidReceiveMessage(async (message) => {
      await this.tokenBridge.handleMessage(message, webviewView.webview);
    });

    // Handle view visibility changes
    webviewView.onDidChangeVisibility(() => {
      if (webviewView.visible) {
        this.triggerRefresh();
      }
    });

    // Initial load
    this.triggerRefresh();
  }

  /**
   * Triggers a message to the Webview to refresh its state.
   */
  public triggerRefresh(): void {
    if (this._view) {
      this._view.webview.postMessage({ type: "refresh" });
    }
  }

  private getHtmlContent(webview: vscode.Webview): string {
    const cspSource = webview.cspSource;

    return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${cspSource} 'unsafe-inline' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; script-src ${cspSource} 'unsafe-inline'; connect-src 'none';">
  <link href="https://fonts.googleapis.com/css2?family=Geist:wght@300;400;500;600;700&display=swap" rel="stylesheet">
  <style>
    :root {
      --bg-color: #0f1015;
      --panel-bg: rgba(255, 255, 255, 0.03);
      --panel-border: rgba(255, 255, 255, 0.08);
      --accent-gradient: linear-gradient(135deg, #6366f1, #a855f7);
      --text-main: #f3f4f6;
      --text-muted: #9ca3af;
      --success-color: #10b981;
      --error-color: #ef4444;
      --warning-color: #f59e0b;
    }

    body {
      font-family: 'Geist', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif;
      background-color: var(--bg-color);
      color: var(--text-main);
      margin: 0;
      padding: 12px;
      overflow-x: hidden;
      font-size: 13px;
    }

    /* Scrollbar Styling */
    ::-webkit-scrollbar {
      width: 6px;
    }
    ::-webkit-scrollbar-track {
      background: transparent;
    }
    ::-webkit-scrollbar-thumb {
      background: rgba(255, 255, 255, 0.1);
      border-radius: 3px;
    }
    ::-webkit-scrollbar-thumb:hover {
      background: rgba(255, 255, 255, 0.2);
    }

    /* Layout & Header */
    .header {
      display: flex;
      justify-content: space-between;
      align-items: center;
      margin-bottom: 16px;
      padding-bottom: 8px;
      border-bottom: 1px solid var(--panel-border);
    }

    .title {
      font-size: 16px;
      font-weight: 700;
      letter-spacing: -0.5px;
      background: var(--accent-gradient);
      -webkit-background-clip: text;
      -webkit-text-fill-color: transparent;
    }

    .status-badge {
      display: flex;
      align-items: center;
      font-size: 11px;
      font-weight: 500;
      padding: 3px 8px;
      border-radius: 12px;
      background: rgba(255, 255, 255, 0.05);
      border: 1px solid var(--panel-border);
    }

    .status-dot {
      width: 6px;
      height: 6px;
      border-radius: 50%;
      margin-right: 6px;
      display: inline-block;
    }

    .status-connected { background-color: var(--success-color); box-shadow: 0 0 8px var(--success-color); }
    .status-disconnected { background-color: var(--error-color); box-shadow: 0 0 8px var(--error-color); }

    /* Card Panels */
    .card {
      background: var(--panel-bg);
      border: 1px solid var(--panel-border);
      border-radius: 10px;
      padding: 12px;
      margin-bottom: 12px;
      backdrop-filter: blur(12px);
      box-shadow: 0 4px 6px rgba(0, 0, 0, 0.1);
      transition: transform 0.2s ease, border-color 0.2s ease;
    }

    .card:hover {
      border-color: rgba(255, 255, 255, 0.15);
    }

    .card-title {
      font-size: 11px;
      font-weight: 600;
      text-transform: uppercase;
      letter-spacing: 0.8px;
      color: var(--text-muted);
      margin-top: 0;
      margin-bottom: 10px;
      display: flex;
      justify-content: space-between;
      align-items: center;
    }

    /* Project List */
    .project-item {
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding: 6px 0;
      border-bottom: 1px solid rgba(255, 255, 255, 0.03);
    }

    .project-item:last-child {
      border-bottom: none;
    }

    .project-name {
      font-weight: 500;
    }

    .project-meta {
      font-size: 11px;
      color: var(--text-muted);
    }

    /* Tasks List */
    .task-item {
      display: flex;
      align-items: flex-start;
      padding: 8px 0;
      border-bottom: 1px solid rgba(255, 255, 255, 0.03);
    }

    .task-item:last-child {
      border-bottom: none;
    }

    .task-checkbox {
      margin-right: 8px;
      margin-top: 2px;
    }

    .task-content {
      flex: 1;
    }

    .task-desc {
      font-weight: 400;
    }

    .task-desc.completed {
      text-decoration: line-through;
      color: var(--text-muted);
    }

    /* Swarm/Approval Panel */
    .approval-alert {
      background: rgba(245, 158, 11, 0.1);
      border: 1px solid rgba(245, 158, 11, 0.3);
      color: var(--warning-color);
      border-radius: 6px;
      padding: 8px;
      margin-bottom: 10px;
      font-size: 12px;
      display: flex;
      align-items: center;
    }

    /* Button CSS */
    .btn {
      background: var(--accent-gradient);
      color: white;
      border: none;
      border-radius: 6px;
      padding: 8px 12px;
      font-weight: 500;
      cursor: pointer;
      width: 100%;
      text-align: center;
      display: inline-block;
      transition: filter 0.2s ease, transform 0.1s ease;
      font-size: 12px;
    }

    .btn:hover {
      filter: brightness(1.1);
    }

    .btn:active {
      transform: scale(0.98);
    }

    .btn-secondary {
      background: rgba(255, 255, 255, 0.08);
      border: 1px solid var(--panel-border);
      color: var(--text-main);
    }

    .btn-secondary:hover {
      background: rgba(255, 255, 255, 0.12);
    }

    .btn-xs {
      padding: 3px 6px;
      font-size: 10px;
      width: auto;
      margin-left: 8px;
    }

    /* Utilities */
    .empty-state {
      color: var(--text-muted);
      text-align: center;
      padding: 16px 0;
      font-style: italic;
    }

    .loading-spinner {
      border: 2px solid rgba(255, 255, 255, 0.1);
      border-top: 2px solid #6366f1;
      border-radius: 50%;
      width: 14px;
      height: 14px;
      animation: spin 1s linear infinite;
      display: inline-block;
      vertical-align: middle;
      margin-right: 6px;
    }

    @keyframes spin {
      0% { transform: rotate(0deg); }
      100% { transform: rotate(360deg); }
    }
  </style>
</head>
<body>
  <div class="header">
    <div class="title">R-AI-OS Kernel</div>
    <div id="status-badge" class="status-badge">
      <span id="status-dot" class="status-dot status-disconnected"></span>
      <span id="status-text">Connecting...</span>
    </div>
  </div>

  <div id="approval-box" style="display: none;">
    <div class="approval-alert">
      <span>⚠️ Action Required: Swarm or Diff approval pending.</span>
    </div>
  </div>

  <!-- Projects Card -->
  <div class="card">
    <div class="card-title">
      <span>Active Projects</span>
      <span id="project-count" class="project-meta"></span>
    </div>
    <div id="projects-list">
      <div class="empty-state">Loading projects...</div>
    </div>
  </div>

  <!-- Tasks Card -->
  <div class="card">
    <div class="card-title">Tasks & Status</div>
    <div id="tasks-list">
      <div class="empty-state">Loading tasks...</div>
    </div>
  </div>

  <!-- Actions -->
  <div style="margin-top: 16px;">
    <button id="refresh-btn" class="btn btn-secondary">Refresh Dashboard</button>
  </div>

  <script>
    const vscode = acquireVsCodeApi();
    let requestIdCounter = 0;
    const pendingRequests = new Map();

    /**
     * Broker API request through VS Code extension host (safe token forwarding)
     */
    function apiFetch(endpoint, method = "GET", body = null) {
      const requestId = requestIdCounter++;
      return new Promise((resolve, reject) => {
        pendingRequests.set(requestId, { resolve, reject });
        vscode.postMessage({ type: "fetch", requestId, endpoint, method, body });
      });
    }

    // Handle messages returned from Extension Host
    window.addEventListener("message", event => {
      const message = event.data;
      
      if (message.type === "fetchResponse") {
        const req = pendingRequests.get(message.requestId);
        if (req) {
          pendingRequests.delete(message.requestId);
          if (message.success) {
            req.resolve(message.data);
          } else {
            req.reject(new Error(message.error));
          }
        }
      } else if (message.type === "refresh") {
        updateDashboard();
      }
    });

    async function updateDashboard() {
      const statusDot = document.getElementById("status-dot");
      const statusText = document.getElementById("status-text");
      const projectsList = document.getElementById("projects-list");
      const tasksList = document.getElementById("tasks-list");
      const approvalBox = document.getElementById("approval-box");

      try {
        // 1. Fetch Health
        const health = await apiFetch("/api/health");
        
        statusDot.className = "status-dot status-connected";
        statusText.textContent = "Daemon Live";

        if (health.needs_human_approval) {
          approvalBox.style.display = "block";
        } else {
          approvalBox.style.display = "none";
        }

        // 2. Fetch Projects
        try {
          const projects = await apiFetch("/api/projects");
          document.getElementById("project-count").textContent = projects.length + " total";
          
          if (!projects || projects.length === 0) {
            projectsList.innerHTML = '<div class="empty-state">No active projects</div>';
          } else {
            projectsList.innerHTML = projects.map(p => {
              const name = p.name || p.path.split(/[\\/]/).pop() || "Unknown";
              return \`
                <div class="project-item">
                  <span class="project-name">\${name}</span>
                  <span class="project-meta">\${p.language || 'Rust'}</span>
                </div>
              \`;
            }).join('');
          }
        } catch (err) {
          projectsList.innerHTML = \`<div class="empty-state" style="color: var(--error-color);">Projects load failed</div>\`;
        }

        // 3. Fetch Tasks
        try {
          const taskData = await apiFetch("/api/tasks");
          const tasks = taskData.tasks || [];
          
          if (!tasks || tasks.length === 0) {
            tasksList.innerHTML = '<div class="empty-state">No tasks found</div>';
          } else {
            tasksList.innerHTML = tasks.map(t => {
              const isCompleted = t.status === "completed" || t.status === "Completed" || t.completed === true;
              return \`
                <div class="task-item">
                  <input type="checkbox" class="task-checkbox" \${isCompleted ? 'checked' : ''} disabled />
                  <div class="task-content">
                    <span class="task-desc \${isCompleted ? 'completed' : ''}">\${t.description || t.title || 'Task'}</span>
                  </div>
                </div>
              \`;
            }).join('');
          }
        } catch (err) {
          tasksList.innerHTML = \`<div class="empty-state" style="color: var(--error-color);">Tasks load failed</div>\`;
        }

      } catch (err) {
        statusDot.className = "status-dot status-disconnected";
        statusText.textContent = "Offline";
        approvalBox.style.display = "none";
        projectsList.innerHTML = '<div class="empty-state">Daemon not running. Run "raios daemon start"</div>';
        tasksList.innerHTML = '<div class="empty-state">Connection offline</div>';
      }
    }

    // Auto update every 10 seconds per plan
    setInterval(updateDashboard, 10000);

    // Refresh on button click
    document.getElementById("refresh-btn").addEventListener("click", () => {
      const btn = document.getElementById("refresh-btn");
      btn.innerHTML = '<span class="loading-spinner"></span>Refreshing...';
      updateDashboard().finally(() => {
        btn.textContent = "Refresh Dashboard";
      });
    });

    // Perform initial loading
    updateDashboard();
  </script>
</body>
</html>`;
  }
}
