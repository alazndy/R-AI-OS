import * as vscode from "vscode";

export class RefactorStatusItem implements vscode.Disposable {
  private readonly item: vscode.StatusBarItem;

  constructor() {
    this.item = vscode.window.createStatusBarItem(
      vscode.StatusBarAlignment.Left,
      90
    );
    this.item.command = "raios.showRefactorView";
    this.item.tooltip = "R-AI-OS: Refactor scan results — click to open panel";
    this.setClean();
    this.item.show();
  }

  update(high: number, medium: number): void {
    const total = high + medium;
    if (total === 0) {
      this.setClean();
      return;
    }
    this.item.text = `$(warning) ${total} refactor`;
    this.item.tooltip = `R-AI-OS: ${high} HIGH, ${medium} MEDIUM files need refactoring`;
    this.item.backgroundColor =
      high > 0
        ? new vscode.ThemeColor("statusBarItem.warningBackground")
        : undefined;
  }

  private setClean(): void {
    this.item.text = "$(check) R";
    this.item.tooltip = "R-AI-OS: No refactor issues";
    this.item.backgroundColor = undefined;
  }

  dispose(): void {
    this.item.dispose();
  }
}
