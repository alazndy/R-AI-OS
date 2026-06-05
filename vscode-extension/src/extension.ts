import * as vscode from "vscode";
import { DaemonClient } from "./ipc/DaemonClient";
import { StatusBarProvider } from "./providers/StatusBarProvider";
import { CommandBridge } from "./commands/CommandBridge";
import { DiffInboxProvider } from "./providers/DiffInboxProvider";
import { DiagnosticProvider } from "./providers/DiagnosticProvider";
import { SecurityDecorationProvider } from "./providers/SecurityDecorationProvider";
import { RefactorDecorationProvider } from "./providers/RefactorDecorationProvider";
import { RefactorProvider } from "./providers/RefactorProvider";
import { RefactorTreeProvider } from "./providers/RefactorTreeProvider";
import { RefactorStatusItem } from "./providers/RefactorStatusItem";
import { JumpToCode } from "./bridge/JumpToCode";
import { TokenBridge } from "./ipc/TokenBridge";
import { SidebarProvider } from "./providers/SidebarProvider";

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
  statusBar = new StatusBarProvider(client, pollInterval, outputChannel);

  diagnostics = new DiagnosticProvider(outputChannel);
  const tokenBridge = new TokenBridge(context);
  const sidebarProvider = new SidebarProvider(context, tokenBridge, outputChannel);
  const securityDecorations = new SecurityDecorationProvider();
  diagnostics.setDecorationProvider(securityDecorations);

  const refactorDecorations = new RefactorDecorationProvider();
  const refactorTree = new RefactorTreeProvider();
  const refactorStatus = new RefactorStatusItem();
  const refactorProvider = new RefactorProvider(
    outputChannel,
    refactorDecorations,
    refactorTree,
    refactorStatus
  );

  const bridge = new CommandBridge(client, outputChannel, diagnostics);

  statusBar.activate(context);
  bridge.register(context);

  context.subscriptions.push(
    vscode.window.registerFileDecorationProvider(securityDecorations),
    securityDecorations,
    vscode.window.registerFileDecorationProvider(refactorDecorations),
    refactorDecorations,
    vscode.window.registerTreeDataProvider("raiosRefactorView", refactorTree),
    refactorTree,
    refactorStatus,
    vscode.window.registerWebviewViewProvider(SidebarProvider.viewType, sidebarProvider),
    vscode.commands.registerCommand("raios.showRefactorView", () => {
      vscode.commands.executeCommand("raiosRefactorView.focus");
    })
  );

  diagnostics.activate(context);
  refactorProvider.activate(context);

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
      refactorProvider.dispose();
    },
  });

  outputChannel.appendLine("[R-AI-OS] Extension activated");
}

export function deactivate(): void {
  client?.disconnect();
  statusBar?.dispose();
}
