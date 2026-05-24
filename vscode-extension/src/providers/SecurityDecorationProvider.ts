import * as vscode from "vscode";

export class SecurityDecorationProvider implements vscode.FileDecorationProvider, vscode.Disposable {
  private readonly emitter = new vscode.EventEmitter<vscode.Uri[]>();
  readonly onDidChangeFileDecorations = this.emitter.event;

  private readonly fileData = new Map<string, vscode.DiagnosticSeverity>();

  provideFileDecoration(uri: vscode.Uri): vscode.FileDecoration | undefined {
    const sev = this.fileData.get(uri.fsPath);
    if (sev === undefined) return undefined;

    if (sev === vscode.DiagnosticSeverity.Error) {
      return {
        badge: "!",
        tooltip: "R-AI-OS: Security issue (CRITICAL/HIGH)",
        color: new vscode.ThemeColor("errorForeground"),
        propagate: true,
      };
    }
    if (sev === vscode.DiagnosticSeverity.Warning) {
      return {
        badge: "~",
        tooltip: "R-AI-OS: Security warning (MEDIUM)",
        color: new vscode.ThemeColor("editorWarning.foreground"),
        propagate: true,
      };
    }
    return undefined;
  }

  update(fileMap: Map<string, vscode.DiagnosticSeverity>): void {
    const changed: vscode.Uri[] = [];

    for (const fp of this.fileData.keys()) {
      if (!fileMap.has(fp)) changed.push(vscode.Uri.file(fp));
    }
    for (const fp of fileMap.keys()) {
      changed.push(vscode.Uri.file(fp));
    }

    this.fileData.clear();
    for (const [fp, sev] of fileMap) {
      this.fileData.set(fp, sev);
    }

    if (changed.length > 0) this.emitter.fire(changed);
  }

  dispose(): void {
    this.emitter.dispose();
  }
}
