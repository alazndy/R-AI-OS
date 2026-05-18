import * as vscode from "vscode";
import { DaemonClient } from "../ipc/DaemonClient";

export class JumpToCode {
  constructor(private readonly client: DaemonClient) {}

  activate(): void {
    this.client.onMessage((msg) => {
      if (msg["event"] !== "OpenFile") return;

      const filePath = msg["path"];
      const line = msg["line"];
      const col = msg["col"];

      if (typeof filePath !== "string" || !filePath) return;

      const lineNum = typeof line === "number" ? Math.max(0, line - 1) : 0;
      const colNum = typeof col === "number" ? Math.max(0, col - 1) : 0;

      const uri = vscode.Uri.file(filePath);
      const pos = new vscode.Position(lineNum, colNum);

      void vscode.window.showTextDocument(uri, {
        selection: new vscode.Range(pos, pos),
        preserveFocus: false,
      });
    });
  }
}
