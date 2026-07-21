import * as vscode from "vscode";
import * as cp from "child_process";
import * as path from "path";
import { DaemonClient } from "../ipc/DaemonClient";
import { resolveRaiosBinary } from "../utils/raiosBinary";
import { DiagnosticProvider } from "../providers/DiagnosticProvider";

export class CommandBridge {
  constructor(
    private readonly client: DaemonClient,
    private readonly outputChannel: vscode.OutputChannel,
    private readonly diagnosticProvider: DiagnosticProvider
  ) {}

  register(context: vscode.ExtensionContext): void {
    context.subscriptions.push(
      vscode.commands.registerCommand("raios.healthCheck", () =>
        this.runCliWithOutput(["--json", "health"], "Health check complete")
      ),
      vscode.commands.registerCommand("raios.commitPush", () =>
        this.commitPushFlow()
      ),
      vscode.commands.registerCommand("raios.dispatchTask", () =>
        this.dispatchTask()
      ),
      vscode.commands.registerCommand("raios.cortexIndex", () =>
        this.runCli(["cortex-index"], "Cortex indexed")
      ),
      vscode.commands.registerCommand("raios.securityScan", () => {
        const p = this.currentProjectPath();
        if (p) this.diagnosticProvider.scanPath(p);
      }),
      vscode.commands.registerCommand("raios.scanCurrentFile", (uri?: vscode.Uri) => {
        const fsPath = uri?.fsPath ?? vscode.window.activeTextEditor?.document.uri.fsPath;
        if (!fsPath) {
          vscode.window.showWarningMessage("R-AI-OS: No file selected");
          return;
        }
        this.diagnosticProvider.scanPath(path.dirname(fsPath));
      }),
      vscode.commands.registerCommand("raios.licenseCheck", () =>
        this.runCliWithOutput(["--json", "license"], "License check complete")
      ),
      vscode.commands.registerCommand("raios.auditPage", () =>
        this.auditFlow()
      ),
      vscode.commands.registerCommand("raios.openMemory", () =>
        this.openMemory()
      ),
      vscode.commands.registerCommand("raios.cronList", () =>
        this.runCliWithOutput(["--json", "cron", "list"], "Scheduler jobs loaded")
      ),
      vscode.commands.registerCommand("raios.cronAdd", () =>
        this.cronAddFlow()
      ),
      vscode.commands.registerCommand("raios.handoff", () =>
        this.handoffFlow()
      ),
      vscode.commands.registerCommand("raios.inboxStatus", () =>
        this.runCliWithOutput(["--json", "health"], "Inbox status loaded")
      )
    );
  }

  private runCli(args: string[], successMsg: string): void {
    const projectPath = this.currentProjectPath();
    this.outputChannel.appendLine(`[raios] running: raios ${args.join(" ")}`);

    vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title: `R-AI-OS: ${args.find(a => !a.startsWith("-"))}…`, cancellable: false },
      () => new Promise<void>((resolve) => {
        cp.execFile(
          resolveRaiosBinary(),
          args,
          { cwd: projectPath ?? undefined, timeout: 60000 },
          (err, stdout, stderr) => {
            if (stdout.trim()) this.outputChannel.appendLine(stdout.trim());
            if (err) {
              this.outputChannel.appendLine(`[raios] error: ${stderr || err.message}`);
              vscode.window.showErrorMessage(`R-AI-OS error: ${stderr || err.message}`);
            } else {
              vscode.window.showInformationMessage(`R-AI-OS: ${successMsg}`);
            }
            resolve();
          }
        );
      })
    );
  }

  private runCliWithOutput(args: string[], successMsg: string): void {
    const projectPath = this.currentProjectPath();
    this.outputChannel.appendLine(`[raios] running: raios ${args.join(" ")}`);

    vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title: `R-AI-OS: ${args.find(a => !a.startsWith("-"))}…`, cancellable: false },
      () => new Promise<void>((resolve) => {
        cp.execFile(
          resolveRaiosBinary(),
          args,
          { cwd: projectPath ?? undefined, timeout: 60000 },
          (err, stdout, stderr) => {
            if (err) {
              this.outputChannel.appendLine(`[raios] error: ${stderr || err.message}`);
              vscode.window.showErrorMessage(`R-AI-OS error: ${stderr || err.message}`);
            } else {
              // Pretty-print JSON output to channel and bring it into view
              try {
                const parsed = JSON.parse(stdout.trim());
                this.outputChannel.appendLine(JSON.stringify(parsed, null, 2));
              } catch {
                this.outputChannel.appendLine(stdout.trim());
              }
              this.outputChannel.show(true);
              vscode.window.showInformationMessage(`R-AI-OS: ${successMsg}`);
            }
            resolve();
          }
        );
      })
    );
  }

  private async auditFlow(): Promise<void> {
    const url = await vscode.window.showInputBox({
      prompt: "URL to audit with Lighthouse",
      placeHolder: "https://example.com",
      validateInput: (v) => {
        const trimmed = v.trim();
        try {
          const parsed = new URL(trimmed);
          if (parsed.protocol === "https:") {
            return null;
          }
          if (
            parsed.protocol === "http:" &&
            (parsed.hostname === "localhost" ||
              parsed.hostname === "127.0.0.1" ||
              parsed.hostname === "[::1]")
          ) {
            return null;
          }
      return "URL must use HTTPS; HTTP is allowed only for local loopback";
        } catch {
          return "Invalid URL format";
        }
      },
    });
    if (!url?.trim()) return;
    this.runCli(["audit", url.trim()], "Audit complete");
  }

  private async commitPushFlow(): Promise<void> {
    const msg = await vscode.window.showInputBox({
      prompt: "Commit message (leave empty for auto)",
      placeHolder: "chore: raios auto-sync",
    });
    if (msg === undefined) return;
    const args = msg.trim() ? ["commit", "--message", msg, "--push"] : ["commit", "--push"];
    this.runCli(args, "Committed & pushed");
  }

  private async dispatchTask(): Promise<void> {
    const task = await vscode.window.showInputBox({
      prompt: "Task description for agent router",
      placeHolder: "Fix the auth bug in login flow",
    });
    if (!task?.trim()) return;
    const projectPath = this.currentProjectPath();
    const terminal = vscode.window.createTerminal("R-AI-OS Task");
    terminal.show();
    if (projectPath) terminal.sendText(`cd "${projectPath}"`);
    const safeTask = task.trim().replace(/\\/g, "\\\\").replace(/"/g, '\\"');
    terminal.sendText(`raios task "${safeTask}"`);
  }

  private async openMemory(): Promise<void> {
    const projectPath = this.currentProjectPath();
    if (!projectPath) {
      vscode.window.showWarningMessage("R-AI-OS: No workspace folder open");
      return;
    }
    const memPath = path.join(projectPath, "memory.md");
    try {
      await vscode.window.showTextDocument(vscode.Uri.file(memPath));
    } catch {
      vscode.window.showWarningMessage("R-AI-OS: memory.md not found in this project");
    }
  }

  private async cronAddFlow(): Promise<void> {
    const title = await vscode.window.showInputBox({
      prompt: "Job title",
      placeHolder: "Daily health check",
    });
    if (!title?.trim()) return;

    const every = await vscode.window.showInputBox({
      prompt: "Interval (e.g. 30s, 5m, 6h, 1d)",
      placeHolder: "24h",
      validateInput: (v) =>
        /^\d+[smhd]$/.test(v.trim()) ? null : "Format: <number><s|m|h|d>",
    });
    if (!every?.trim()) return;

    const agent = await vscode.window.showQuickPick(
      ["claude", "codex", "opencode", "agy"],
      { placeHolder: "Select agent" }
    );
    if (!agent) return;

    const task = await vscode.window.showInputBox({
      prompt: "Task description (injected as agent prompt)",
      placeHolder: "Check all projects for health issues and report",
    });
    if (!task?.trim()) return;

    this.runCli(
      ["cron", "add", title.trim(), "--every", every.trim(), "--agent", agent, "--task", task.trim()],
      `Scheduled job "${title.trim()}" created`
    );
  }

  private async handoffFlow(): Promise<void> {
    const target = await vscode.window.showQuickPick(
      ["claude-kaira", "codex-kaira", "opencode-kaira", "antigravity-kaira"],
      { placeHolder: "Hand off to agent" }
    );
    if (!target) return;

    const status = await vscode.window.showQuickPick(["success", "failed", "blocker"], {
      placeHolder: "Handoff status",
    });
    if (!status) return;

    const msg = await vscode.window.showInputBox({
      prompt: "Context message for receiving agent",
      placeHolder: "Auth module refactored, tests passing, review edge cases",
    });
    if (!msg?.trim()) return;

    this.runCli(
      ["handoff", "--to", target, "--status", status, "--msg", msg.trim()],
      `Handoff delivered to ${target}`
    );
  }

  private currentProjectPath(): string | null {
    return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? null;
  }
}
