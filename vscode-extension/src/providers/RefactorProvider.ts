import * as vscode from "vscode";
import * as cp from "child_process";
import { RefactorDecorationProvider } from "./RefactorDecorationProvider";
import { RefactorTreeProvider, RefactorFileData } from "./RefactorTreeProvider";
import { RefactorStatusItem } from "./RefactorStatusItem";

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
  private readonly treeProvider: RefactorTreeProvider;
  private readonly statusItem: RefactorStatusItem;
  private scanTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(
    outputChannel: vscode.OutputChannel,
    decorationProvider: RefactorDecorationProvider,
    treeProvider: RefactorTreeProvider,
    statusItem: RefactorStatusItem
  ) {
    this.outputChannel = outputChannel;
    this.decorationProvider = decorationProvider;
    this.treeProvider = treeProvider;
    this.statusItem = statusItem;
  }

  activate(context: vscode.ExtensionContext): void {
    this.disposables.push(
      vscode.workspace.onDidSaveTextDocument(() => {
        if (this.scanTimer) clearTimeout(this.scanTimer);
        this.scanTimer = setTimeout(() => this.scanWorkspace(), 3000);
      }),
      vscode.workspace.onDidChangeConfiguration((e) => {
        if (e.affectsConfiguration("raios.refactor")) {
          this.scanWorkspace();
        }
      })
    );
    context.subscriptions.push(...this.disposables);
    this.scanWorkspace();
  }

  private scanWorkspace(): void {
    const folders = vscode.workspace.workspaceFolders;
    if (!folders || folders.length === 0) return;

    const allDecorations = new Map<string, "HIGH" | "MEDIUM">();
    const allFileData: RefactorFileData[] = [];
    let pending = folders.length;

    for (const folder of folders) {
      this.runScan(folder.uri.fsPath, (issues) => {
        for (const issue of issues) {
          if (issue.severity === "HIGH" || issue.severity === "MEDIUM") {
            allDecorations.set(issue.file, issue.severity);
            allFileData.push({
              file: issue.file,
              severity: issue.severity,
              lines: issue.lines,
              reasons: issue.reasons,
            });
          }
        }
        pending--;
        if (pending === 0) {
          this.decorationProvider.update(allDecorations);
          this.treeProvider.update(allFileData);
          const high = allFileData.filter((f) => f.severity === "HIGH").length;
          const med = allFileData.filter((f) => f.severity === "MEDIUM").length;
          this.statusItem.update(high, med);
        }
      });
    }
  }

  private getThresholdArgs(): string[] {
    const cfg = vscode.workspace.getConfiguration("raios.refactor");
    const args = [
      "--high-lines",
      String(cfg.get<number>("highLineThreshold", 500)),
      "--medium-lines",
      String(cfg.get<number>("mediumLineThreshold", 300)),
      "--high-unwrap",
      String(cfg.get<number>("highUnwrapThreshold", 10)),
      "--medium-unwrap",
      String(cfg.get<number>("mediumUnwrapThreshold", 5)),
    ];

    const extRaw = cfg.get<Record<string, unknown>>("extensions", {});
    if (Object.keys(extRaw).length > 0) {
      const extCli: Record<string, Record<string, number>> = {};
      for (const [ext, overrides] of Object.entries(extRaw)) {
        if (typeof overrides === "object" && overrides !== null) {
          const o = overrides as Record<string, number>;
          const mapped: Record<string, number> = {};
          if (o.highLines !== undefined) mapped.high_lines = o.highLines;
          if (o.mediumLines !== undefined) mapped.medium_lines = o.mediumLines;
          if (o.highUnwrap !== undefined) mapped.high_unwrap = o.highUnwrap;
          if (o.mediumUnwrap !== undefined) mapped.medium_unwrap = o.mediumUnwrap;
          if (Object.keys(mapped).length > 0) extCli[ext] = mapped;
        }
      }
      if (Object.keys(extCli).length > 0) {
        args.push("--ext-config", JSON.stringify(extCli));
      }
    }

    return args;
  }

  private runScan(
    path: string,
    callback: (issues: RefactorFileIssue[]) => void
  ): void {
    cp.execFile(
      "raios",
      ["--json", "refactor", path, ...this.getThresholdArgs()],
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
