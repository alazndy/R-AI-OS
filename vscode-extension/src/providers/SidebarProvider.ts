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

    /* Git Status */
    .git-meta { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; min-height: 28px; }
    .git-badge {
      font-size: 10px; font-weight: 600; padding: 2px 7px; border-radius: 10px;
    }
    .git-badge.clean { background: rgba(16,185,129,0.15); color: var(--success-color); }
    .git-badge.dirty { background: rgba(245,158,11,0.15); color: var(--warning-color); }
    .git-stat { font-size: 11px; color: var(--text-muted); }
    .git-stat b { color: var(--text-main); font-weight: 600; }

    /* Swarm */
    .swarm-item {
      display: flex; align-items: flex-start; gap: 8px;
      padding: 7px 0; border-bottom: 1px solid rgba(255,255,255,0.03);
    }
    .swarm-item:last-child { border-bottom: none; }
    .swarm-dot {
      width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; margin-top: 3px;
    }
    .swarm-dot.running       { background: var(--warning-color); box-shadow: 0 0 6px var(--warning-color); }
    .swarm-dot.awaiting_review { background: #f97316; box-shadow: 0 0 6px #f97316; }
    .swarm-dot.initializing  { background: var(--gray-color); }
    .swarm-body { flex: 1; min-width: 0; }
    .swarm-desc { font-size: 12px; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .swarm-meta { font-size: 10px; color: var(--text-muted); margin-top: 2px; }
    .btn-approve {
      font-size: 10px; padding: 3px 8px; flex-shrink: 0;
      background: rgba(249,115,22,0.12); border: 1px solid rgba(249,115,22,0.3);
      color: #f97316; border-radius: 4px; cursor: pointer; transition: background 0.2s;
    }
    .btn-approve:hover { background: rgba(249,115,22,0.22); }

    /* Quick Actions */
    .quick-actions { display: flex; gap: 8px; margin-bottom: 12px; }
    .btn-action {
      flex: 1; background: rgba(255,255,255,0.06); border: 1px solid var(--panel-border);
      color: var(--text-main); border-radius: 6px; padding: 7px 8px;
      font-size: 11px; font-weight: 500; cursor: pointer;
      text-align: center; transition: background 0.2s;
    }
    .btn-action:hover { background: rgba(255,255,255,0.11); }

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

  <script>
    const vscode = acquireVsCodeApi();
    let requestIdCounter = 0;
    const pendingRequests = new Map();
    let currentWorkspacePath = "";

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
        if (message.workspacePath !== undefined) currentWorkspacePath = message.workspacePath;
        updateDashboard();
      } else if (message.type === "daemonSpawning") {
        document.getElementById("status-text").textContent = "Starting...";
      } else if (message.type === "daemonFailed") {
        document.getElementById("status-text").textContent = "Start failed";
        document.getElementById("status-dot").className = "status-dot status-disconnected";
      }
    });

    async function updateDashboard() {
      const statusDot    = document.getElementById("status-dot");
      const statusText   = document.getElementById("status-text");
      const approvalBox  = document.getElementById("approval-box");
      const offlineBox   = document.getElementById("offline-box");
      const gitBody      = document.getElementById("git-status-body");
      const plansList    = document.getElementById("plans-list");
      const tasksList    = document.getElementById("tasks-list");
      const swarmList    = document.getElementById("swarm-list");

      try {
        const health = await apiFetch("/api/health");
        statusDot.className = "status-dot status-connected";
        statusText.textContent = "Daemon Live";
        approvalBox.style.display = health.needs_human_approval ? "block" : "none";
        offlineBox.style.display = "none";

        // Git Status
        try {
          const pathParam = currentWorkspacePath ? "?path=" + encodeURIComponent(currentWorkspacePath) : "";
          const git = await apiFetch("/api/git-status" + pathParam);
          if (git.error) {
            document.getElementById("git-branch-label").textContent = "";
            gitBody.innerHTML = \`<div class="empty-state">\${esc(git.error)}</div>\`;
          } else {
            document.getElementById("git-branch-label").textContent = git.branch || "";
            const badgeClass = git.dirty ? "dirty" : "clean";
            const badgeText  = git.dirty ? "Dirty" : "Clean";
            let stats = "";
            if (git.staged    > 0) stats += \`<span class="git-stat"><b>\${git.staged}</b> staged</span>\`;
            if (git.modified  > 0) stats += \`<span class="git-stat"><b>\${git.modified}</b> modified</span>\`;
            if (git.untracked > 0) stats += \`<span class="git-stat"><b>\${git.untracked}</b> untracked</span>\`;
            gitBody.innerHTML = \`<div class="git-meta">
              <span class="git-badge \${badgeClass}">\${badgeText}</span>
              \${stats}
            </div>\`;
          }
        } catch {
          gitBody.innerHTML = '<div class="empty-state" style="color:var(--error-color)">Load failed</div>';
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
                  : p.total > 0 ? p.checked + "/" + p.total : "—";
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
              const id = esc(t.id || t.task_id || "");
              return \`<div class="task-item">
                <input type="checkbox" class="task-checkbox" \${done ? "checked" : ""} data-task-id="\${id}" data-done="\${done}" />
                <div class="task-content">
                  <span class="task-desc \${done ? "completed" : ""}">\${esc(t.description || t.title || "Task")}</span>
                </div>
              </div>\`;
            }).join("");

            // write-back: toggle task status on checkbox click
            tasksList.querySelectorAll(".task-checkbox").forEach(cb => {
              cb.addEventListener("change", (e) => {
                const el = e.target;
                const taskId = el.dataset.taskId;
                const completed = el.checked;
                if (!taskId) return;
                vscode.postMessage({ type: "toggleTask", taskId, completed });
                // optimistic UI update
                const desc = el.closest(".task-item").querySelector(".task-desc");
                if (desc) { desc.classList.toggle("completed", completed); }
              });
            });
          }
        } catch {
          tasksList.innerHTML = '<div class="empty-state" style="color:var(--error-color)">Load failed</div>';
        }

        // Swarm
        try {
          const swarmData = await apiFetch("/api/swarm");
          const swarmTasks = swarmData.tasks || [];
          document.getElementById("swarm-count").textContent = swarmTasks.length > 0 ? swarmTasks.length + " active" : "";
          if (swarmTasks.length === 0) {
            swarmList.innerHTML = '<div class="empty-state">No active swarm tasks</div>';
          } else {
            swarmList.innerHTML = swarmTasks.map(t => {
              const dotClass = t.status === "running" ? "running"
                : t.status === "awaiting_review" ? "awaiting_review" : "initializing";
              const approveBtn = t.status === "awaiting_review"
                ? \`<button class="btn-approve" data-id="\${esc(t.id)}">Approve</button>\`
                : "";
              return \`<div class="swarm-item">
                <div class="swarm-dot \${dotClass}"></div>
                <div class="swarm-body">
                  <div class="swarm-desc" title="\${esc(t.description)}">\${esc(t.description)}</div>
                  <div class="swarm-meta">\${esc(t.project)} · \${esc(t.agent)}</div>
                </div>
                \${approveBtn}
              </div>\`;
            }).join("");
            swarmList.querySelectorAll(".btn-approve").forEach(btn => {
              btn.addEventListener("click", async () => {
                btn.textContent = "...";
                btn.disabled = true;
                try {
                  await apiFetch("/api/approve", "POST", { task_id: btn.dataset.id });
                  updateSwarm();
                } catch {
                  btn.textContent = "Failed";
                }
              });
            });
          }
        } catch {
          swarmList.innerHTML = '<div class="empty-state" style="color:var(--error-color)">Load failed</div>';
        }

      } catch {
        statusDot.className = "status-dot status-disconnected";
        statusText.textContent = "Offline";
        approvalBox.style.display = "none";
        offlineBox.style.display = "block";
        if (gitBody)    gitBody.innerHTML   = '<div class="empty-state">—</div>';
        if (plansList)  plansList.innerHTML  = '<div class="empty-state">—</div>';
        if (tasksList)  tasksList.innerHTML  = '<div class="empty-state">—</div>';
        if (swarmList)  swarmList.innerHTML  = '<div class="empty-state">—</div>';
        document.getElementById("git-branch-label").textContent = "";
      }
    }

    async function updateSwarm() {
      const swarmList = document.getElementById("swarm-list");
      try {
        const swarmData = await apiFetch("/api/swarm");
        const swarmTasks = swarmData.tasks || [];
        document.getElementById("swarm-count").textContent = swarmTasks.length > 0 ? swarmTasks.length + " active" : "";
        if (swarmTasks.length === 0) {
          swarmList.innerHTML = '<div class="empty-state">No active swarm tasks</div>';
        }
      } catch { /* silent */ }
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

    document.getElementById("launch-btn").addEventListener("click", () => {
      const btn = document.getElementById("launch-btn");
      btn.textContent = "Starting...";
      btn.disabled = true;
      vscode.postMessage({ type: "startDaemon" });
    });

    document.getElementById("btn-build").addEventListener("click", () => {
      vscode.postMessage({ type: "runBuild" });
    });

    document.getElementById("btn-test").addEventListener("click", () => {
      vscode.postMessage({ type: "runTest" });
    });

    updateDashboard();
  </script>
</body>
</html>`;
  }
}
