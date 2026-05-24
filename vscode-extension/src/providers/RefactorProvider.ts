import * as vscode from "vscode";
import * as cp from "child_process";
import { RefactorDecorationProvider } from "./RefactorDecorationProvider";

interface RefactorFileIssue {
  schema_version: number;
  file: string;
  lines: number;
  severity: "HIGH" | "MEDIUM" | "LOW";
  reasons: string[];
}

export class RefactorProvider implements vscode.Disposable {
  private readonly disposables: vscode.Disposable[] = [];
  private readonly outputChannel: vscode.OutputChannel;
  private readonly decorationProvider: RefactorDecorationProvider;
  private scanTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(
    outputChannel: vscode.OutputChannel,
    decorationProvider: RefactorDecorationProvider
  ) {
    this.outputChannel = outputChannel;
    this.decorationProvider = decorationProvider;
  }

  activate(context: vscode.ExtensionContext): void {
    this.disposables.push(
      vscode.workspace.onDidSaveTextDocument(() => {
        if (this.scanTimer) clearTimeout(this.scanTimer);
        this.scanTimer = setTimeout(() => this.scanWorkspace(), 3000);
      })
    );
    context.subscriptions.push(...this.disposables);
    this.scanWorkspace();
  }

  private scanWorkspace(): void {
    const folders = vscode.workspace.workspaceFolders;
    if (!folders || folders.length === 0) return;

    const allFiles = new Map<string, "HIGH" | "MEDIUM">();
    let pending = folders.length;

    for (const folder of folders) {
      this.runScan(folder.uri.fsPath, (issues) => {
        for (const issue of issues) {
          if (issue.severity === "HIGH" || issue.severity === "MEDIUM") {
            allFiles.set(issue.file, issue.severity);
          }
        }
        pending--;
        if (pending === 0) {
          this.decorationProvider.update(allFiles);
        }
      });
    }
  }

  private runScan(
    path: string,
    callback: (issues: RefactorFileIssue[]) => void
  ): void {
    cp.execFile(
      "raios",
      ["--json", "refactor", path],
      { timeout: 30_000 },
      (err, stdout, stderr) => {
        if (err) {
          this.outputChannel.appendLine(
            `[raios] refactor scan error: ${err.message}`
          );
          callback([]);
          return;
        }
        if (stderr) {
          this.outputChannel.appendLine(`[raios] refactor stderr: ${stderr}`);
        }
        try {
          const issues = JSON.parse(stdout) as RefactorFileIssue[];
          callback(issues);
        } catch {
          this.outputChannel.appendLine(
            `[raios] refactor parse error: ${stdout.slice(0, 200)}`
          );
          callback([]);
        }
      }
    );
  }

  dispose(): void {
    if (this.scanTimer) clearTimeout(this.scanTimer);
    for (const d of this.disposables) d.dispose();
  }
}
