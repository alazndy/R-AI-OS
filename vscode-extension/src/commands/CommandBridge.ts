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
      validateInput: (v) => (v.startsWith("http") ? null : "Must start with http:// or https://"),
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

  private currentProjectPath(): string | null {
    return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? null;
  }
}
