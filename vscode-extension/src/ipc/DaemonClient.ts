import * as net from "net";
import * as fs from "fs";
import * as path from "path";
import { raiosConfigDir } from "../utils/raiosBinary";

type MessageHandler = (msg: Record<string, unknown>) => void;

export class DaemonClient {
  private socket: net.Socket | null = null;
  private buffer = "";
  private handlers: MessageHandler[] = [];
  private connected = false;
  private connecting = false;
  private reconnectTimer: NodeJS.Timeout | null = null;

  constructor(
    private readonly port: number = 42069,
    private readonly host: string = "127.0.0.1"
  ) {}

  get isConnected(): boolean {
    return this.connected;
  }

  onMessage(handler: MessageHandler): void {
    this.handlers.push(handler);
  }

  connect(): void {
    if (this.connected || this.connecting) return;
    this.connecting = true;

    const token = this.readToken();
    if (!token) {
      console.error("[R-AI-OS] IPC token not found — is aiosd running?");
      this.connecting = false;
      this.scheduleReconnect();
      return;
    }

    this.socket = new net.Socket();

    this.socket.connect(this.port, this.host, () => {
      this.connecting = false;
      this.connected = true;
      const sock = this.socket;
      if (sock) {
        sock.write(`AUTH ${token}\n`);
      }
      console.log("[R-AI-OS] Connected to aiosd");
    });

    this.socket.on("data", (data: Buffer) => {
      this.buffer += data.toString();
      const lines = this.buffer.split("\n");
      this.buffer = lines.pop() ?? "";
      for (const line of lines) {
        if (!line.trim()) continue;
        try {
          const msg = JSON.parse(line) as Record<string, unknown>;
          this.handlers.forEach((h) => h(msg));
        } catch {
          console.warn("[R-AI-OS] Received malformed JSON from daemon:", line.slice(0, 100));
        }
      }
    });

    this.socket.on("error", (err) => {
      console.error("[R-AI-OS] IPC error:", err.message);
      this.connecting = false;
      this.socket?.destroy();
      this.socket = null;
    });

    this.socket.on("close", () => {
      this.connected = false;
      this.connecting = false;
      this.socket = null;
      this.scheduleReconnect();
    });
  }

  send(method: string, params: Record<string, unknown> = {}): void {
    if (!this.socket || !this.connected) return;
    const msg = JSON.stringify({ jsonrpc: "2.0", id: Date.now(), method, params });
    this.socket.write(msg + "\n");
  }

  sendRaw(payload: Record<string, unknown>): void {
    if (!this.socket || !this.connected) return;
    this.socket.write(JSON.stringify(payload) + "\n");
  }

  disconnect(): void {
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.connecting = false;
    this.socket?.destroy();
    this.connected = false;
  }

  private readToken(): string | null {
    const tokenPath = path.join(raiosConfigDir(), ".ipc_token");
    try {
      return fs.readFileSync(tokenPath, "utf-8").trim();
    } catch {
      return null;
    }
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimer) return;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, 5000);
  }
}
