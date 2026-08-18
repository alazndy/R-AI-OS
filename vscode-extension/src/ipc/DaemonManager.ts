import * as fs from "fs";
import * as cp from "child_process";
import * as net from "net";
import * as vscode from "vscode";
import { resolveAiosdBinary, tokenFilePath } from "../utils/raiosBinary";

const TOKEN_MAX_AGE_MS = 8 * 60 * 60 * 1000; // 8 hours — matches Rust auth.rs
const POLL_INTERVAL_MS = 600;
const POLL_TIMEOUT_MS = 15_000;
const DAEMON_PROBE_TIMEOUT_MS = 500;
const DAEMON_HOST = "127.0.0.1";
const DAEMON_PORT = 42069;

export class DaemonManager {
  private daemonProcess: cp.ChildProcess | null = null;

  constructor(
    private readonly outputChannel: vscode.OutputChannel,
    private readonly onDaemonReady: () => void
  ) {}

  /** Returns true if a fresh token file exists (daemon is likely running). */
  public isTokenFresh(): boolean {
    return this.readFreshToken() !== null;
  }

  private readFreshToken(): string | null {
    const tokenPath = tokenFilePath();
    try {
      const stat = fs.statSync(tokenPath);
      if (Date.now() - stat.mtimeMs >= TOKEN_MAX_AGE_MS) {
        return null;
      }
      const token = fs.readFileSync(tokenPath, "utf8").trim();
      return token || null;
    } catch {
      return null;
    }
  }

  /** Proves that the listener accepts the current token, not merely that the port is open. */
  private probeAuthenticatedDaemon(): Promise<boolean> {
    const token = this.readFreshToken();
    if (!token) {
      return Promise.resolve(false);
    }

    return new Promise((resolve) => {
      let settled = false;
      let response = "";
      const socket = net.createConnection({ host: DAEMON_HOST, port: DAEMON_PORT });
      const finish = (ready: boolean): void => {
        if (settled) return;
        settled = true;
        socket.destroy();
        resolve(ready);
      };

      socket.setTimeout(DAEMON_PROBE_TIMEOUT_MS);
      socket.once("connect", () => socket.write(`AUTH ${token}\n`));
      socket.on("data", (chunk: Buffer) => {
        response += chunk.toString("utf8");
        if (response.includes('"event":"SessionStarted"')) {
          finish(true);
        } else if (response.includes('"event":"Error"')) {
          finish(false);
        }
      });
      socket.once("timeout", () => finish(false));
      socket.once("error", () => finish(false));
      socket.once("close", () => finish(false));
    });
  }

  /** Let the Linux user service own daemon lifecycle; multiple callers remain idempotent. */
  private startViaSystemd(): Promise<boolean> {
    if (process.platform !== "linux") {
      return Promise.resolve(false);
    }

    return new Promise((resolve) => {
      cp.execFile(
        "systemctl",
        ["--user", "start", "--no-block", "aiosd.service"],
        { timeout: 2_000 },
        (error) => resolve(!error)
      );
    });
  }

  /**
   * Ensures the daemon is running. Linux delegates lifecycle to systemd;
   * platforms without that user service fall back to a detached child.
   */
  public async ensureRunning(): Promise<boolean> {
    if (await this.probeAuthenticatedDaemon()) {
      return true;
    }

    if (await this.startViaSystemd()) {
      this.outputChannel.appendLine(
        "[DaemonManager] Requested aiosd.service from the systemd user manager."
      );
      return this.waitForDaemon();
    }

    return this.spawn();
  }

  /** Spawns aiosd detached. Returns true once the token file appears. */
  public async spawn(): Promise<boolean> {
    if (await this.probeAuthenticatedDaemon()) {
      return true;
    }

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

    return this.waitForDaemon();
  }

  /** Polls until the current token completes an authenticated daemon handshake. */
  private async waitForDaemon(): Promise<boolean> {
    const deadline = Date.now() + POLL_TIMEOUT_MS;
    while (Date.now() <= deadline) {
      if (await this.probeAuthenticatedDaemon()) {
        this.outputChannel.appendLine(
          "[DaemonManager] Daemon ready — authenticated handshake succeeded."
        );
        this.onDaemonReady();
        return true;
      }
      await new Promise<void>((resolve) => setTimeout(resolve, POLL_INTERVAL_MS));
    }

    this.outputChannel.appendLine("[DaemonManager] Timeout waiting for daemon.");
    return false;
  }

  public dispose(): void {
    // Do not kill the daemon on extension deactivation — it should keep running.
    this.daemonProcess = null;
  }
}
