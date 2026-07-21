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

    function createEmptyState(text, isError = false) {
      const div = document.createElement("div");
      div.className = "empty-state";
      if (isError) {
        div.style.color = "var(--error-color)";
      }
      div.textContent = text;
      return div;
    }

    function setEmptyState(container, text, isError = false) {
      if (!container) return;
      container.replaceChildren(createEmptyState(text, isError));
    }

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
            setEmptyState(gitBody, git.error);
          } else {
            document.getElementById("git-branch-label").textContent = git.branch || "";
            const metaDiv = document.createElement("div");
            metaDiv.className = "git-meta";

            const badgeSpan = document.createElement("span");
            badgeSpan.className = `git-badge ${git.dirty ? "dirty" : "clean"}`;
            badgeSpan.textContent = git.dirty ? "Dirty" : "Clean";
            metaDiv.appendChild(badgeSpan);

            const addStat = (count, label) => {
              if (count > 0) {
                const statSpan = document.createElement("span");
                statSpan.className = "git-stat";
                const b = document.createElement("b");
                b.textContent = String(count);
                statSpan.appendChild(b);
                statSpan.appendChild(document.createTextNode(` ${label}`));
                metaDiv.appendChild(statSpan);
              }
            };
            addStat(git.staged, "staged");
            addStat(git.modified, "modified");
            addStat(git.untracked, "untracked");

            gitBody.replaceChildren(metaDiv);
          }
        } catch {
          setEmptyState(gitBody, "Load failed", true);
        }

        // Plans
        try {
          const planData = await apiFetch("/api/plans");
          const plans = planData.plans || [];
          document.getElementById("plans-count").textContent = plans.length + " plans";
          if (plans.length === 0) {
            setEmptyState(plansList, "No plans found");
          } else {
            plansList.replaceChildren();
            plans.forEach(p => {
              const pctLabel = p.status === "done"
                ? "Done"
                : p.status === "not_started"
                  ? "Not started"
                  : p.total > 0 ? p.checked + "/" + p.total : "—";
              const barWidth = p.status === "done" ? 100 : (p.total > 0 ? Math.round(p.checked * 100 / p.total) : 0);

              const item = document.createElement("div");
              item.className = "plan-item";

              const dot = document.createElement("div");
              dot.className = `plan-dot ${p.status || ""}`;

              const body = document.createElement("div");
              body.className = "plan-body";

              const title = document.createElement("div");
              title.className = "plan-title";
              title.title = p.title || "";
              title.textContent = p.title || "";

              const meta = document.createElement("div");
              meta.className = "plan-meta";

              const dateSpan = document.createElement("span");
              dateSpan.className = "plan-date";
              dateSpan.textContent = p.date || "";

              const bar = document.createElement("div");
              bar.className = "plan-progress-bar";

              const fill = document.createElement("div");
              fill.className = `plan-progress-fill ${p.status || ""}`;
              fill.style.width = `${barWidth}%`;
              bar.appendChild(fill);

              const pctSpan = document.createElement("span");
              pctSpan.className = "plan-pct";
              pctSpan.textContent = pctLabel;

              meta.appendChild(dateSpan);
              meta.appendChild(bar);
              meta.appendChild(pctSpan);

              body.appendChild(title);
              body.appendChild(meta);

              item.appendChild(dot);
              item.appendChild(body);

              plansList.appendChild(item);
            });
          }
        } catch {
          setEmptyState(plansList, "Load failed", true);
        }

        // Tasks
        try {
          const taskData = await apiFetch("/api/tasks");
          const tasks = taskData.tasks || [];
          if (tasks.length === 0) {
            setEmptyState(tasksList, "No tasks");
          } else {
            tasksList.replaceChildren();
            tasks.forEach(t => {
              const done = t.status === "completed" || t.status === "Completed" || t.completed === true;
              const id = t.id || t.task_id || "";

              const item = document.createElement("div");
              item.className = "task-item";

              const cb = document.createElement("input");
              cb.type = "checkbox";
              cb.className = "task-checkbox";
              if (done) cb.checked = true;
              cb.dataset.taskId = id;
              cb.dataset.done = done;

              const content = document.createElement("div");
              content.className = "task-content";

              const desc = document.createElement("span");
              desc.className = `task-desc ${done ? "completed" : ""}`;
              desc.textContent = t.description || t.title || "Task";
              content.appendChild(desc);

              item.appendChild(cb);
              item.appendChild(content);

              cb.addEventListener("change", (e) => {
                const el = e.target;
                const taskId = el.dataset.taskId;
                const completed = el.checked;
                if (!taskId) return;
                vscode.postMessage({ type: "toggleTask", taskId, completed });
                desc.classList.toggle("completed", completed);
              });

              tasksList.appendChild(item);
            });
          }
        } catch {
          setEmptyState(tasksList, "Load failed", true);
        }

        // Swarm
        try {
          const swarmData = await apiFetch("/api/swarm");
          const swarmTasks = swarmData.tasks || [];
          document.getElementById("swarm-count").textContent = swarmTasks.length > 0 ? swarmTasks.length + " active" : "";
          if (swarmTasks.length === 0) {
            setEmptyState(swarmList, "No active swarm tasks");
          } else {
            swarmList.replaceChildren();
            swarmTasks.forEach(t => {
              const dotClass = t.status === "running" ? "running"
                : t.status === "awaiting_review" ? "awaiting_review" : "initializing";

              const item = document.createElement("div");
              item.className = "swarm-item";

              const dot = document.createElement("div");
              dot.className = `swarm-dot ${dotClass}`;

              const body = document.createElement("div");
              body.className = "swarm-body";

              const desc = document.createElement("div");
              desc.className = "swarm-desc";
              desc.title = t.description || "";
              desc.textContent = t.description || "";

              const meta = document.createElement("div");
              meta.className = "swarm-meta";
              meta.textContent = `${t.project || ""} · ${t.agent || ""}`;

              body.appendChild(desc);
              body.appendChild(meta);

              item.appendChild(dot);
              item.appendChild(body);

              if (t.status === "awaiting_review") {
                const btn = document.createElement("button");
                btn.className = "btn-approve";
                btn.dataset.id = t.id || "";
                btn.textContent = "Approve";
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
                item.appendChild(btn);
              }

              swarmList.appendChild(item);
            });
          }
        } catch {
          setEmptyState(swarmList, "Load failed", true);
        }

      } catch {
        statusDot.className = "status-dot status-disconnected";
        statusText.textContent = "Offline";
        approvalBox.style.display = "none";
        offlineBox.style.display = "block";
        setEmptyState(gitBody, "—");
        setEmptyState(plansList, "—");
        setEmptyState(tasksList, "—");
        setEmptyState(swarmList, "—");
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
          setEmptyState(swarmList, "No active swarm tasks");
        }
      } catch { /* silent */ }
    }

    setInterval(updateDashboard, 10000);

    document.getElementById("refresh-btn").addEventListener("click", () => {
      const btn = document.getElementById("refresh-btn");
      btn.replaceChildren();
      const spinner = document.createElement("span");
      spinner.className = "loading-spinner";
      btn.appendChild(spinner);
      btn.appendChild(document.createTextNode("Refreshing..."));
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
