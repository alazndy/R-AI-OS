import * as vscode from "vscode";
import { DaemonClient } from "./ipc/DaemonClient";
import { StatusBarProvider } from "./providers/StatusBarProvider";
import { CommandBridge } from "./commands/CommandBridge";
import { DiffInboxProvider } from "./providers/DiffInboxProvider";
import { DiagnosticProvider } from "./providers/DiagnosticProvider";
import { JumpToCode } from "./bridge/JumpToCode";

let client: DaemonClient;
let statusBar: StatusBarProvider;
let diagnostics: DiagnosticProvider;

export function activate(context: vscode.ExtensionContext): void {
  const config = vscode.workspace.getConfiguration("raios");
  const port = config.get<number>("daemonPort", 42069);
  const pollInterval = config.get<number>("pollInterval", 30);

  const outputChannel = vscode.window.createOutputChannel("R-AI-OS");
  context.subscriptions.push(outputChannel);

  client = new DaemonClient(port);
  statusBar = new StatusBarProvider(client, pollInterval);
  const bridge = new CommandBridge(client, outputChannel);
  diagnostics = new DiagnosticProvider(outputChannel);

  statusBar.activate(context);
  bridge.register(context);
  diagnostics.activate(context);
  const diffInbox = new DiffInboxProvider(client);
  diffInbox.activate(context);
  const jumpToCode = new JumpToCode(client);
  jumpToCode.activate();
  client.connect();

  context.subscriptions.push({
    dispose: () => {
      client.disconnect();
      statusBar.dispose();
      diagnostics.dispose();
    },
  });

  outputChannel.appendLine("[R-AI-OS] Extension activated");
}

export function deactivate(): void {
  client?.disconnect();
  statusBar?.dispose();
}
