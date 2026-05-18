import * as vscode from "vscode";
import { DaemonClient } from "./ipc/DaemonClient";
import { StatusBarProvider } from "./providers/StatusBarProvider";

let client: DaemonClient;
let statusBar: StatusBarProvider;

export function activate(context: vscode.ExtensionContext): void {
  const config = vscode.workspace.getConfiguration("raios");
  const port = config.get<number>("daemonPort", 42069);
  const pollInterval = config.get<number>("pollInterval", 30);

  client = new DaemonClient(port);
  statusBar = new StatusBarProvider(client, pollInterval);

  statusBar.activate(context);
  client.connect();

  context.subscriptions.push({
    dispose: () => {
      client.disconnect();
      statusBar.dispose();
    },
  });

  console.log("[R-AI-OS] Extension activated");
}

export function deactivate(): void {
  client?.disconnect();
  statusBar?.dispose();
}
