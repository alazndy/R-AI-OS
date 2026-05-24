import * as vscode from "vscode";
import * as cp from "child_process";
import * as path from "path";
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
  // Track which files have diagnostics per scanned directory
  private readonly seenFiles = new Map<string, Set<string>>();
  private readonly outputChannel: vscode.OutputChannel;

  constructor(outputChannel: vscode.OutputChannel) {
    this.collection = vscode.languages.createDiagnosticCollection("raios-security");
    this.outputChannel = outputChannel;
  }

  activate(context: vscode.ExtensionContext): void {
    context.subscriptions.push(this.collection);

    context.subscriptions.push(
      vscode.workspace.onDidSaveTextDocument((doc) => {
        const cfg = vscode.workspace.getConfiguration("raios");
        if (!cfg.get<boolean>("diagnosticsEnabled", true)) return;
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
    const cfg = vscode.workspace.getConfiguration("raios");
    const debounceMs = cfg.get<number>("diagnosticsDebounceMs", 800);

    if (this.debounceTimer) {
      clearTimeout(this.debounceTimer);
      this.debounceTimer = null;
    }
    if (this.pendingProcess) {
      this.pendingProcess.kill();
      this.pendingProcess = null;
    }

    // Scan the parent directory, not the file itself
    const dir = path.dirname(filePath);

    this.debounceTimer = setTimeout(() => {
      this.debounceTimer = null;
      this.scanPath(dir);
    }, debounceMs);
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

    // Show spinner — auto-clears when the process closes
    const scanDone = new Promise<void>((resolve) => child.on("close", () => resolve()));
    vscode.window.setStatusBarMessage("$(sync~spin) R-AI-OS scanning…", scanDone);

    child.stdout.on("data", (chunk: Buffer) => { stdout += chunk.toString(); });
    child.stderr.on("data", (chunk: Buffer) => { stderr += chunk.toString(); });

    child.on("close", (code) => {
      this.pendingProcess = null;
      if (stderr.trim()) {
        this.outputChannel.appendLine(`[raios] stderr: ${stderr.trim()}`);
      }
      if (code === null) return; // killed — newer scan replaced this one
      this.applyDiagnostics(stdout, targetPath);
    });

    child.on("error", (err) => {
      this.pendingProcess = null;
      this.outputChannel.appendLine(`[raios] spawn error: ${err.message}`);
    });
  }

  private applyDiagnostics(raw: string, scannedDir: string): void {
    if (!raw.trim()) return;

    let rows: SecurityRowJson[];
    try {
      rows = JSON.parse(raw) as SecurityRowJson[];
    } catch {
      this.outputChannel.appendLine("[raios] Failed to parse security JSON output");
      return;
    }

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
        const line = Math.max(0, (issue.line ?? 1) - 1);
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

    // Clear stale diagnostics only for files in this scanned directory
    const prevFiles = this.seenFiles.get(scannedDir) ?? new Set<string>();
    for (const fp of prevFiles) {
      if (!byFile.has(fp)) {
        this.collection.delete(vscode.Uri.file(fp));
      }
    }

    // Apply fresh diagnostics
    const newSeen = new Set<string>();
    for (const [fp, diags] of byFile) {
      this.collection.set(vscode.Uri.file(fp), diags);
      newSeen.add(fp);
    }
    this.seenFiles.set(scannedDir, newSeen);

    const total = [...byFile.values()].reduce((s, d) => s + d.length, 0);
    if (total > 0) {
      this.outputChannel.appendLine(`[raios] ${total} issue(s) found in ${scannedDir}`);
    }
  }

  dispose(): void {
    if (this.debounceTimer) clearTimeout(this.debounceTimer);
    if (this.pendingProcess) this.pendingProcess.kill();
    this.collection.dispose();
  }
}
