import * as vscode from "vscode";

export class RefactorDecorationProvider
  implements vscode.FileDecorationProvider, vscode.Disposable
{
  private readonly emitter = new vscode.EventEmitter<vscode.Uri[]>();
  readonly onDidChangeFileDecorations = this.emitter.event;
  private readonly fileData = new Map<string, "HIGH" | "MEDIUM">();

  provideFileDecoration(uri: vscode.Uri): vscode.FileDecoration | undefined {
    const sev = this.fileData.get(uri.fsPath);
    if (!sev) return undefined;
    if (sev === "HIGH") {
      return {
        badge: "R!",
        tooltip: "R-AI-OS: Refactor needed (HIGH)",
        color: new vscode.ThemeColor("editorError.foreground"),
        propagate: true,
      };
    }
    return {
      badge: "R",
      tooltip: "R-AI-OS: Refactor suggested (MEDIUM)",
      color: new vscode.ThemeColor("editorWarning.foreground"),
      propagate: true,
    };
  }

  update(fileMap: Map<string, "HIGH" | "MEDIUM">): void {
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
