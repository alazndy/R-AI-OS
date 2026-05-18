import * as vscode from "vscode";
import * as cp from "child_process";
import { DaemonClient } from "../ipc/DaemonClient";

export class CommandBridge {
  constructor(private readonly client: DaemonClient) {}

  register(context: vscode.ExtensionContext): void {
    context.subscriptions.push(
      vscode.commands.registerCommand("raios.healthCheck", () =>
        this.runCli(["health"], "Health check complete")
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
      vscode.commands.registerCommand("raios.securityScan", () =>
        this.runCli(["security", "."], "Security scan complete")
      )
    );
  }

  private runCli(args: string[], successMsg: string): void {
    const projectPath = this.currentProjectPath();

    vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: `R-AI-OS: ${args[0]}…`,
        cancellable: false,
      },
      () =>
        new Promise<void>((resolve) => {
          cp.execFile(
            "raios",
            args,
            { cwd: projectPath ?? undefined, timeout: 60000 },
            (err, _stdout, stderr) => {
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

    if (msg === undefined) return;

    const args = msg.trim()
      ? ["commit", "--message", msg, "--push"]
      : ["commit", "--push"];

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
    terminal.sendText(`raios task "${task.trim()}"`);
  }

  private currentProjectPath(): string | null {
    const folders = vscode.workspace.workspaceFolders;
    return folders?.[0]?.uri.fsPath ?? null;
  }
}
