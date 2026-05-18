import * as vscode from "vscode";
import { DaemonClient } from "./ipc/DaemonClient";
import { StatusBarProvider } from "./providers/StatusBarProvider";
import { CommandBridge } from "./commands/CommandBridge";
import { DiffInboxProvider } from "./providers/DiffInboxProvider";
import { JumpToCode } from "./bridge/JumpToCode";

let client: DaemonClient;
let statusBar: StatusBarProvider;

export function activate(context: vscode.ExtensionContext): void {
  const config = vscode.workspace.getConfiguration("raios");
  const port = config.get<number>("daemonPort", 42069);
  const pollInterval = config.get<number>("pollInterval", 30);

  client = new DaemonClient(port);
  statusBar = new StatusBarProvider(client, pollInterval);
  const bridge = new CommandBridge(client);

  statusBar.activate(context);
  bridge.register(context);
  const diffInbox = new DiffInboxProvider(client);
  diffInbox.activate(context);
  const jumpToCode = new JumpToCode(client);
  jumpToCode.activate();
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
