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

    webviewView.webview.onDidReceiveMessage(async (message) => {
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
      --gray-color: #6b7280;
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

    ::-webkit-scrollbar { width: 6px; }
    ::-webkit-scrollbar-track { background: transparent; }
    ::-webkit-scrollbar-thumb { background: rgba(255,255,255,0.1); border-radius: 3px; }
    ::-webkit-scrollbar-thumb:hover { background: rgba(255,255,255,0.2); }

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
      background: rgba(255,255,255,0.05);
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

    .card {
      background: var(--panel-bg);
      border: 1px solid var(--panel-border);
      border-radius: 10px;
      padding: 12px;
      margin-bottom: 12px;
      backdrop-filter: blur(12px);
      box-shadow: 0 4px 6px rgba(0,0,0,0.1);
    }

    .card:hover { border-color: rgba(255,255,255,0.15); }

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

    /* Projects */
    .project-item {
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding: 6px 0;
      border-bottom: 1px solid rgba(255,255,255,0.03);
    }
    .project-item:last-child { border-bottom: none; }
    .project-name { font-weight: 500; }
    .project-meta { font-size: 11px; color: var(--text-muted); }

    /* Tasks */
    .task-item {
      display: flex;
      align-items: flex-start;
      padding: 8px 0;
      border-bottom: 1px solid rgba(255,255,255,0.03);
    }
    .task-item:last-child { border-bottom: none; }
    .task-checkbox { margin-right: 8px; margin-top: 2px; }
    .task-content { flex: 1; }
    .task-desc { font-weight: 400; }
    .task-desc.completed { text-decoration: line-through; color: var(--text-muted); }

    /* Plans */
    .plan-item {
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 7px 0;
      border-bottom: 1px solid rgba(255,255,255,0.03);
    }
    .plan-item:last-child { border-bottom: none; }

    .plan-dot {
      width: 8px;
      height: 8px;
      border-radius: 50%;
      flex-shrink: 0;
    }
    .plan-dot.done         { background-color: var(--success-color); box-shadow: 0 0 6px var(--success-color); }
    .plan-dot.in_progress  { background-color: var(--warning-color); box-shadow: 0 0 6px var(--warning-color); }
    .plan-dot.not_started  { background-color: var(--error-color); }
    .plan-dot.no_tasks     { background-color: var(--gray-color); }

    .plan-body { flex: 1; min-width: 0; }

    .plan-title {
      font-size: 12px;
      font-weight: 500;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }

    .plan-meta {
      display: flex;
      align-items: center;
      gap: 6px;
      margin-top: 2px;
    }

    .plan-date { font-size: 10px; color: var(--text-muted); }

    .plan-progress-bar {
      flex: 1;
      height: 3px;
      background: rgba(255,255,255,0.08);
      border-radius: 2px;
      overflow: hidden;
    }

    .plan-progress-fill {
      height: 100%;
      border-radius: 2px;
      transition: width 0.3s ease;
    }
    .plan-progress-fill.done        { background: var(--success-color); }
    .plan-progress-fill.in_progress { background: var(--warning-color); }
    .plan-progress-fill.not_started { background: var(--error-color); }
    .plan-progress-fill.no_tasks    { background: var(--gray-color); }

    .plan-pct {
      font-size: 10px;
      color: var(--text-muted);
      min-width: 28px;
      text-align: right;
      flex-shrink: 0;
    }

    /* Approval */
    .approval-alert {
      background: rgba(245,158,11,0.1);
      border: 1px solid rgba(245,158,11,0.3);
      color: var(--warning-color);
      border-radius: 6px;
      padding: 8px;
      margin-bottom: 10px;
      font-size: 12px;
    }

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
    .btn:hover { filter: brightness(1.1); }
    .btn:active { transform: scale(0.98); }
    .btn-secondary {
      background: rgba(255,255,255,0.08);
      border: 1px solid var(--panel-border);
      color: var(--text-main);
    }
    .btn-secondary:hover { background: rgba(255,255,255,0.12); }

    .empty-state {
      color: var(--text-muted);
      text-align: center;
      padding: 16px 0;
      font-style: italic;
    }

    .loading-spinner {
      border: 2px solid rgba(255,255,255,0.1);
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
      0%   { transform: rotate(0deg); }
      100% { transform: rotate(360deg); }
    }
  </style>
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

  <!-- Projects -->
  <div class="card">
    <div class="card-title">
      <span>Active Projects</span>
      <span id="project-count" style="font-size:10px; color: var(--text-muted);"></span>
    </div>
    <div id="projects-list"><div class="empty-state">Loading...</div></div>
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

  <div style="margin-top: 16px;">
    <button id="refresh-btn" class="btn btn-secondary">Refresh</button>
  </div>

  <script>
    const vscode = acquireVsCodeApi();
    let requestIdCounter = 0;
    const pendingRequests = new Map();

    function apiFetch(endpoint, method = "GET", body = null) {
      const requestId = requestIdCounter++;
      return new Promise((resolve, reject) => {
        pendingRequests.set(requestId, { resolve, reject });
        vscode.postMessage({ type: "fetch", requestId, endpoint, method, body });
      });
    }

    window.addEventListener("message", event => {
      const message = event.data;
      if (message.type === "fetchResponse") {
        const req = pendingRequests.get(message.requestId);
        if (req) {
          pendingRequests.delete(message.requestId);
          message.success ? req.resolve(message.data) : req.reject(new Error(message.error));
        }
      } else if (message.type === "refresh") {
        updateDashboard();
      }
    });

    async function updateDashboard() {
      const statusDot  = document.getElementById("status-dot");
      const statusText = document.getElementById("status-text");
      const projectsList = document.getElementById("projects-list");
      const plansList    = document.getElementById("plans-list");
      const tasksList    = document.getElementById("tasks-list");
      const approvalBox  = document.getElementById("approval-box");

      try {
        const health = await apiFetch("/api/health");
        statusDot.className = "status-dot status-connected";
        statusText.textContent = "Daemon Live";
        approvalBox.style.display = health.needs_human_approval ? "block" : "none";

        // Projects
        try {
          const projects = await apiFetch("/api/projects");
          document.getElementById("project-count").textContent = (projects.length || 0) + " total";
          if (!projects || projects.length === 0) {
            projectsList.innerHTML = '<div class="empty-state">No active projects</div>';
          } else {
            projectsList.innerHTML = projects.map(p => {
              const name = p.name || (p.path || "").split(/[\\\\/]/).pop() || "Unknown";
              return \`<div class="project-item">
                <span class="project-name">\${esc(name)}</span>
                <span class="project-meta">\${esc(p.language || "Rust")}</span>
              </div>\`;
            }).join("");
          }
        } catch {
          projectsList.innerHTML = '<div class="empty-state" style="color:var(--error-color)">Load failed</div>';
        }

        // Plans
        try {
          const planData = await apiFetch("/api/plans");
          const plans = planData.plans || [];
          document.getElementById("plans-count").textContent = plans.length + " plans";

          if (plans.length === 0) {
            plansList.innerHTML = '<div class="empty-state">No plans found</div>';
          } else {
            plansList.innerHTML = plans.map(p => {
              const pctLabel = p.status === "done"
                ? "Done"
                : p.status === "not_started"
                  ? "Not started"
                  : p.total > 0
                    ? p.checked + "/" + p.total
                    : "—";
              const barWidth = p.status === "done" ? 100 : (p.total > 0 ? Math.round(p.checked * 100 / p.total) : 0);
              return \`<div class="plan-item">
                <div class="plan-dot \${esc(p.status)}"></div>
                <div class="plan-body">
                  <div class="plan-title" title="\${esc(p.title)}">\${esc(p.title)}</div>
                  <div class="plan-meta">
                    <span class="plan-date">\${esc(p.date)}</span>
                    <div class="plan-progress-bar">
                      <div class="plan-progress-fill \${esc(p.status)}" style="width:\${barWidth}%"></div>
                    </div>
                    <span class="plan-pct">\${esc(pctLabel)}</span>
                  </div>
                </div>
              </div>\`;
            }).join("");
          }
        } catch {
          plansList.innerHTML = '<div class="empty-state" style="color:var(--error-color)">Load failed</div>';
        }

        // Tasks
        try {
          const taskData = await apiFetch("/api/tasks");
          const tasks = taskData.tasks || [];
          if (tasks.length === 0) {
            tasksList.innerHTML = '<div class="empty-state">No tasks</div>';
          } else {
            tasksList.innerHTML = tasks.map(t => {
              const done = t.status === "completed" || t.status === "Completed" || t.completed === true;
              return \`<div class="task-item">
                <input type="checkbox" class="task-checkbox" \${done ? "checked" : ""} disabled />
                <div class="task-content">
                  <span class="task-desc \${done ? "completed" : ""}">\${esc(t.description || t.title || "Task")}</span>
                </div>
              </div>\`;
            }).join("");
          }
        } catch {
          tasksList.innerHTML = '<div class="empty-state" style="color:var(--error-color)">Load failed</div>';
        }

      } catch {
        statusDot.className = "status-dot status-disconnected";
        statusText.textContent = "Offline";
        approvalBox.style.display = "none";
        projectsList.innerHTML = '<div class="empty-state">Daemon offline. Run "raios daemon start"</div>';
        plansList.innerHTML    = '<div class="empty-state">—</div>';
        tasksList.innerHTML    = '<div class="empty-state">—</div>';
      }
    }

    function esc(str) {
      return String(str ?? "")
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;");
    }

    setInterval(updateDashboard, 10000);

    document.getElementById("refresh-btn").addEventListener("click", () => {
      const btn = document.getElementById("refresh-btn");
      btn.innerHTML = '<span class="loading-spinner"></span>Refreshing...';
      updateDashboard().finally(() => { btn.textContent = "Refresh"; });
    });

    updateDashboard();
  </script>
</body>
</html>`;
  }
}
