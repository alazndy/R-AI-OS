import * as vscode from "vscode";
import * as cp from "child_process";
import { DaemonClient } from "../ipc/DaemonClient";

interface HealthEntry {
  name: string;
  compliance_score?: number;
  compliance_grade: string;
  security_grade?: string;
  git_dirty?: boolean;
}

export class StatusBarProvider implements vscode.Disposable {
  private readonly item: vscode.StatusBarItem;
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

    this.timer = setInterval(
      () => this.refresh(),
      this.pollIntervalSecs * 1000
    );

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

    cp.execFile(
      "raios",
      ["--json", "health", projectName],
      { timeout: 10000 },
      (err, stdout) => {
        if (err || !stdout.trim()) {
          this.item.text = "$(warning) R-AI-OS";
          return;
        }
        try {
          const data = JSON.parse(stdout.trim()) as HealthEntry[];
          const h = data[0];
          if (!h) {
            this.item.text = "$(warning) R-AI-OS";
            return;
          }
          const score = h.compliance_score ?? "?";
          const grade = h.compliance_grade ?? "-";
          const dirty = h.git_dirty === true ? " $(git-commit)" : "";
          const icon =
            grade === "A"
              ? "$(check)"
              : grade === "B"
              ? "$(info)"
              : "$(warning)";
          this.item.text = `${icon} R-AI-OS ${score}/100 (${grade})${dirty}`;
        } catch {
          this.item.text = "$(warning) R-AI-OS";
        }
      }
    );
  }

  private currentProjectName(): string | null {
    const folders = vscode.workspace.workspaceFolders;
    return folders?.[0]?.name ?? null;
  }

  dispose(): void {
    if (this.timer) clearInterval(this.timer);
    this.item.dispose();
  }
}
