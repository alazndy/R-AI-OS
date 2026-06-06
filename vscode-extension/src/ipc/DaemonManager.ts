import * as fs from "fs";
import * as cp from "child_process";
import * as vscode from "vscode";
import { resolveAiosdBinary, tokenFilePath } from "../utils/raiosBinary";

const TOKEN_MAX_AGE_MS = 8 * 60 * 60 * 1000; // 8 hours — matches Rust auth.rs
const POLL_INTERVAL_MS = 600;
const POLL_TIMEOUT_MS = 15_000;

export class DaemonManager {
  private daemonProcess: cp.ChildProcess | null = null;

  constructor(
    private readonly outputChannel: vscode.OutputChannel,
    private readonly onDaemonReady: () => void
  ) {}

  /** Returns true if a fresh token file exists (daemon is likely running). */
  public isTokenFresh(): boolean {
    const tokenPath = tokenFilePath();
    try {
      const stat = fs.statSync(tokenPath);
      return Date.now() - stat.mtimeMs < TOKEN_MAX_AGE_MS;
    } catch {
      return false;
    }
  }

  /**
   * Ensures the daemon is running. If the token is stale/missing, spawns aiosd
   * and waits for the token file to appear (up to POLL_TIMEOUT_MS).
   */
  public async ensureRunning(): Promise<boolean> {
    if (this.isTokenFresh()) {
      return true;
    }
    return this.spawn();
  }

  /** Spawns aiosd detached. Returns true once the token file appears. */
  public async spawn(): Promise<boolean> {
    const bin = resolveAiosdBinary();
    if (!bin) {
      this.outputChannel.appendLine(
        "[DaemonManager] aiosd binary not found. Build with: cargo build"
      );
      vscode.window.showWarningMessage(
        "R-AI-OS: daemon binary (aiosd) not found. Run `cargo build` first.",
        "OK"
      );
      return false;
    }

    this.outputChannel.appendLine(`[DaemonManager] Spawning daemon: ${bin}`);

    this.daemonProcess = cp.spawn(bin, [], {
      detached: true,
      stdio: "ignore",
      windowsHide: true,
    });
    this.daemonProcess.unref();

    return this.waitForToken();
  }

  /** Polls for the token file until it appears or timeout. */
  private waitForToken(): Promise<boolean> {
    return new Promise((resolve) => {
      const deadline = Date.now() + POLL_TIMEOUT_MS;
      const interval = setInterval(() => {
        if (this.isTokenFresh()) {
          clearInterval(interval);
          this.outputChannel.appendLine("[DaemonManager] Daemon ready — token found.");
          this.onDaemonReady();
          resolve(true);
          return;
        }
        if (Date.now() > deadline) {
          clearInterval(interval);
          this.outputChannel.appendLine("[DaemonManager] Timeout waiting for daemon.");
          resolve(false);
        }
      }, POLL_INTERVAL_MS);
    });
  }

  public dispose(): void {
    // Do not kill the daemon on extension deactivation — it should keep running.
    this.daemonProcess = null;
  }
}
