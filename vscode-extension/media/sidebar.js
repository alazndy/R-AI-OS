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
            gitBody.innerHTML = `<div class="empty-state">${esc(git.error)}</div>`;
          } else {
            document.getElementById("git-branch-label").textContent = git.branch || "";
            const badgeClass = git.dirty ? "dirty" : "clean";
            const badgeText  = git.dirty ? "Dirty" : "Clean";
            let stats = "";
            if (git.staged    > 0) stats += `<span class="git-stat"><b>${git.staged}</b> staged</span>`;
            if (git.modified  > 0) stats += `<span class="git-stat"><b>${git.modified}</b> modified</span>`;
            if (git.untracked > 0) stats += `<span class="git-stat"><b>${git.untracked}</b> untracked</span>`;
            gitBody.innerHTML = `<div class="git-meta">
              <span class="git-badge ${badgeClass}">${badgeText}</span>
              ${stats}
            </div>`;
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
              return `<div class="plan-item">
                <div class="plan-dot ${esc(p.status)}"></div>
                <div class="plan-body">
                  <div class="plan-title" title="${esc(p.title)}">${esc(p.title)}</div>
                  <div class="plan-meta">
                    <span class="plan-date">${esc(p.date)}</span>
                    <div class="plan-progress-bar">
                      <div class="plan-progress-fill ${esc(p.status)}" style="width:${barWidth}%"></div>
                    </div>
                    <span class="plan-pct">${esc(pctLabel)}</span>
                  </div>
                </div>
              </div>`;
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
              return `<div class="task-item">
                <input type="checkbox" class="task-checkbox" ${done ? "checked" : ""} data-task-id="${id}" data-done="${done}" />
                <div class="task-content">
                  <span class="task-desc ${done ? "completed" : ""}">${esc(t.description || t.title || "Task")}</span>
                </div>
              </div>`;
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
                ? `<button class="btn-approve" data-id="${esc(t.id)}">Approve</button>`
                : "";
              return `<div class="swarm-item">
                <div class="swarm-dot ${dotClass}"></div>
                <div class="swarm-body">
                  <div class="swarm-desc" title="${esc(t.description)}">${esc(t.description)}</div>
                  <div class="swarm-meta">${esc(t.project)} · ${esc(t.agent)}</div>
                </div>
                ${approveBtn}
              </div>`;
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
