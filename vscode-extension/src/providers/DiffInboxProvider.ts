import * as vscode from "vscode";
import * as os from "os";
import * as fs from "fs";
import * as path from "path";
import { DaemonClient } from "../ipc/DaemonClient";

interface PendingDiff {
  id: string;
  project: string;
  file_path: string;
  original: string;
  proposed: string;
  agent: string;
  description: string;
}

export class DiffInboxProvider implements vscode.Disposable {
  private pollTimer: NodeJS.Timeout | null = null;
  private processingIds = new Set<string>();

  constructor(private readonly client: DaemonClient) {}

  activate(context: vscode.ExtensionContext): void {
    this.pollTimer = setInterval(() => this.poll(), 15000);

    this.client.onMessage((msg) => {
      if (msg["event"] === "PendingDiffs") {
        const diffs = msg["diffs"] as PendingDiff[] | undefined;
        if (diffs && diffs.length > 0) {
          void this.processDiffs(diffs);
        }
      }
    });

    context.subscriptions.push(this);
  }

  private poll(): void {
    if (!this.client.isConnected) return;
    this.client.sendRaw({ command: "GetPendingDiffs" });
  }

  private async processDiffs(diffs: PendingDiff[]): Promise<void> {
    for (const diff of diffs) {
      if (this.processingIds.has(diff.id)) continue;
      this.processingIds.add(diff.id);
      await this.showDiffNotification(diff);
    }
  }

  private async showDiffNotification(diff: PendingDiff): Promise<void> {
    const action = await vscode.window.showInformationMessage(
      `R-AI-OS (${diff.agent}): ${diff.description}`,
      "Review Changes",
      "Reject"
    );

    if (action === "Review Changes") {
      await this.openDiffEditor(diff);
    } else if (action === "Reject") {
      this.client.sendRaw({ command: "RejectDiff", id: diff.id });
      this.processingIds.delete(diff.id);
      vscode.window.showInformationMessage("R-AI-OS: Diff rejected");
    } else {
      this.processingIds.delete(diff.id);
    }
  }

  private async openDiffEditor(diff: PendingDiff): Promise<void> {
    let original: string;
    let proposed: string;
    try {
      original = Buffer.from(diff.original, "base64").toString("utf-8");
      proposed = Buffer.from(diff.proposed, "base64").toString("utf-8");
      if (!original && !proposed) {
        throw new Error("decoded content is empty");
      }
    } catch (err) {
      this.processingIds.delete(diff.id);
      vscode.window.showErrorMessage(
        `R-AI-OS: Failed to decode diff content — ${err instanceof Error ? err.message : String(err)}`
      );
      return;
    }

    const tmpDir = os.tmpdir();
    const rnd = Math.random().toString(36).slice(2, 9);
    const origPath = path.join(tmpDir, `raios-orig-${diff.id}-${rnd}.tmp`);
    const propPath = path.join(tmpDir, `raios-prop-${diff.id}-${rnd}.tmp`);

    try {
      fs.writeFileSync(origPath, original, "utf-8");
      fs.writeFileSync(propPath, proposed, "utf-8");

      await vscode.commands.executeCommand(
        "vscode.diff",
        vscode.Uri.file(origPath),
        vscode.Uri.file(propPath),
        `R-AI-OS Diff: ${path.basename(diff.file_path)} (${diff.agent})`
      );

      const decision = await vscode.window.showInformationMessage(
        `Accept changes from ${diff.agent} to ${path.basename(diff.file_path)}?`,
        "Accept",
        "Reject"
      );

      if (decision === "Accept") {
        this.client.sendRaw({ command: "ApproveDiff", id: diff.id });
        vscode.window.showInformationMessage(
          `R-AI-OS: Changes applied to ${path.basename(diff.file_path)}`
        );
      } else {
        this.client.sendRaw({ command: "RejectDiff", id: diff.id });
      }
    } finally {
      this.processingIds.delete(diff.id);
      try { fs.unlinkSync(origPath); } catch { /* ignore */ }
      try { fs.unlinkSync(propPath); } catch { /* ignore */ }
    }
  }

  dispose(): void {
    if (this.pollTimer) clearInterval(this.pollTimer);
  }
}
