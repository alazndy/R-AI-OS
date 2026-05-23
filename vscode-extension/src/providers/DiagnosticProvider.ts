import * as vscode from "vscode";
import * as cp from "child_process";
import { resolveRaiosBinary } from "../utils/raiosBinary";

// Schema v1 — matches raios security --json output
interface SecurityIssueJson {
  owasp: string;
  severity: "CRITICAL" | "HIGH" | "MEDIUM" | "LOW" | "INFO";
  title: string;
  file: string | null;
  line: number | null;
  snippet: string | null;
}

interface SecurityRowJson {
  schema_version: number;
  name: string;
  path: string;
  score: number;
  grade: string;
  critical_count: number;
  high_count: number;
  issues: SecurityIssueJson[];
}

const SUPPORTED_SCHEMA_VERSION = 1;

function toVscodeSeverity(s: SecurityIssueJson["severity"]): vscode.DiagnosticSeverity {
  switch (s) {
    case "CRITICAL":
    case "HIGH":
      return vscode.DiagnosticSeverity.Error;
    case "MEDIUM":
      return vscode.DiagnosticSeverity.Warning;
    default:
      return vscode.DiagnosticSeverity.Information;
  }
}

export class DiagnosticProvider implements vscode.Disposable {
  private readonly collection: vscode.DiagnosticCollection;
  private debounceTimer: NodeJS.Timeout | null = null;
  private pendingProcess: cp.ChildProcess | null = null;
  private readonly outputChannel: vscode.OutputChannel;

  constructor(outputChannel: vscode.OutputChannel) {
    this.collection = vscode.languages.createDiagnosticCollection("raios-security");
    this.outputChannel = outputChannel;
  }

  activate(context: vscode.ExtensionContext): void {
    context.subscriptions.push(this.collection);

    context.subscriptions.push(
      vscode.workspace.onDidSaveTextDocument((doc) => {
        this.onSave(doc.uri.fsPath);
      })
    );

    // Scan workspace on activation
    const projectPath = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    if (projectPath) {
      this.scanPath(projectPath);
    }
  }

  private onSave(filePath: string): void {
    if (this.debounceTimer) {
      clearTimeout(this.debounceTimer);
      this.debounceTimer = null;
    }
    // Kill any in-flight scan — avoids process accumulation on rapid saves
    if (this.pendingProcess) {
      this.pendingProcess.kill();
      this.pendingProcess = null;
    }

    this.debounceTimer = setTimeout(() => {
      this.debounceTimer = null;
      this.scanPath(filePath);
    }, 800);
  }

  scanPath(targetPath: string): void {
    const binary = resolveRaiosBinary();
    this.outputChannel.appendLine(`[raios] security scan: ${targetPath}`);

    let stdout = "";
    let stderr = "";

    const child = cp.spawn(binary, ["--json", "security", targetPath], {
      timeout: 30000,
    });

    this.pendingProcess = child;

    child.stdout.on("data", (chunk: Buffer) => { stdout += chunk.toString(); });
    child.stderr.on("data", (chunk: Buffer) => { stderr += chunk.toString(); });

    child.on("close", (code) => {
      this.pendingProcess = null;
      if (stderr.trim()) {
        this.outputChannel.appendLine(`[raios] stderr: ${stderr.trim()}`);
      }
      if (code === null) {
        // Process was killed (new scan replaced this one)
        return;
      }
      this.applyDiagnostics(stdout);
    });

    child.on("error", (err) => {
      this.pendingProcess = null;
      this.outputChannel.appendLine(`[raios] spawn error: ${err.message}`);
    });
  }

  private applyDiagnostics(raw: string): void {
    if (!raw.trim()) return;

    let rows: SecurityRowJson[];
    try {
      rows = JSON.parse(raw) as SecurityRowJson[];
    } catch {
      this.outputChannel.appendLine("[raios] Failed to parse security JSON output");
      return;
    }

    // Guard against future schema changes
    if (rows.length > 0 && rows[0].schema_version !== SUPPORTED_SCHEMA_VERSION) {
      this.outputChannel.appendLine(
        `[raios] Unsupported schema_version ${rows[0].schema_version} (expected ${SUPPORTED_SCHEMA_VERSION}) — update extension`
      );
      return;
    }

    // Group issues by file
    const byFile = new Map<string, vscode.Diagnostic[]>();

    for (const row of rows) {
      for (const issue of row.issues) {
        if (!issue.file) continue;

        const line = Math.max(0, (issue.line ?? 1) - 1); // VS Code is 0-indexed
        const range = new vscode.Range(line, 0, line, Number.MAX_SAFE_INTEGER);
        const message = `[${issue.owasp}] ${issue.title}`;
        const diagnostic = new vscode.Diagnostic(range, message, toVscodeSeverity(issue.severity));
        diagnostic.source = "raios";
        diagnostic.code = issue.owasp;

        const existing = byFile.get(issue.file) ?? [];
        existing.push(diagnostic);
        byFile.set(issue.file, existing);
      }
    }

    // Clear stale diagnostics, then apply fresh ones
    this.collection.clear();
    for (const [filePath, diagnostics] of byFile) {
      this.collection.set(vscode.Uri.file(filePath), diagnostics);
    }

    const total = [...byFile.values()].reduce((s, d) => s + d.length, 0);
    if (total > 0) {
      this.outputChannel.appendLine(`[raios] ${total} issue(s) found`);
    }
  }

  dispose(): void {
    if (this.debounceTimer) clearTimeout(this.debounceTimer);
    if (this.pendingProcess) this.pendingProcess.kill();
    this.collection.dispose();
  }
}
