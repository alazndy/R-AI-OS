import * as vscode from "vscode";
import { DaemonClient } from "./ipc/DaemonClient";

let client: DaemonClient;

export function activate(context: vscode.ExtensionContext): void {
  const config = vscode.workspace.getConfiguration("raios");
  const port = config.get<number>("daemonPort", 42069);

  client = new DaemonClient(port);
  client.connect();

  context.subscriptions.push({ dispose: () => client.disconnect() });

  console.log("[R-AI-OS] Extension activated");
}

export function deactivate(): void {
  client?.disconnect();
}
