import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import * as http from "http";
import * as vscode from "vscode";

export class TokenBridge {
  private cachedToken: string | null = null;

  constructor(private readonly context: vscode.ExtensionContext) {}

  /**
   * Reads the bootstrap session token from disk.
   * Checks both .session_token and fallback .ipc_token.
   */
  public readToken(): string | null {
    const configDir = path.join(os.homedir(), ".config", "raios");
    
    // Windows path detection (AppData/Roaming/raios)
    let appDataDir = configDir;
    if (process.platform === "win32") {
      const appData = process.env.APPDATA || path.join(os.homedir(), "AppData", "Roaming");
      appDataDir = path.join(appData, "raios");
    }

    const tokenPaths = [
      path.join(appDataDir, ".session_token"),
      path.join(appDataDir, ".ipc_token"),
      path.join(configDir, ".session_token"),
      path.join(configDir, ".ipc_token")
    ];

    for (const tokenPath of tokenPaths) {
      try {
        if (fs.existsSync(tokenPath)) {
          const token = fs.readFileSync(tokenPath, "utf-8").trim();
          if (token) {
            this.cachedToken = token;
            // Also cache it in VS Code Secrets Storage as secure backup
            this.context.secrets.store("raios.session_token", token);
            return token;
          }
        }
      } catch (err) {
        // Silent catch for individual path failures
      }
    }

    return null;
  }

  /**
   * Retrieves the token, prioritizing memory cache, disk, then Secrets Storage.
   */
  public async getToken(): Promise<string | null> {
    if (this.cachedToken) {
      return this.cachedToken;
    }

    const diskToken = this.readToken();
    if (diskToken) {
      return diskToken;
    }

    // Try Secrets Storage fallback
    const secretToken = await this.context.secrets.get("raios.session_token");
    if (secretToken) {
      this.cachedToken = secretToken;
      return secretToken;
    }

    return null;
  }

  /**
   * Performs an HTTP request to the local Raios daemon with the Bearer token.
   */
  public async request(endpoint: string, method: string = "GET", body?: any): Promise<any> {
    const token = await this.getToken();
    if (!token) {
      throw new Error("No session token available. Please ensure R-AI-OS daemon is running.");
    }

    // Read HTTP port from configurations, default to 42071 (per plan)
    const config = vscode.workspace.getConfiguration("raios");
    const port = config.get<number>("httpPort", 42071);

    const headers: Record<string, string> = {
      "Authorization": `Bearer ${token}`,
      "Host": `localhost:${port}`,
      "Content-Type": "application/json"
    };

    const postData = body ? JSON.stringify(body) : undefined;
    if (postData) {
      headers["Content-Length"] = Buffer.byteLength(postData).toString();
    }

    return new Promise((resolve, reject) => {
      const req = http.request(
        {
          hostname: "127.0.0.1",
          port,
          path: endpoint,
          method: method.toUpperCase(),
          headers,
          timeout: 5000 // 5-second timeout per plan
        },
        (res) => {
          let data = "";
          res.on("data", (chunk) => {
            data += chunk;
          });
          res.on("end", () => {
            if (res.statusCode && res.statusCode >= 200 && res.statusCode < 300) {
              try {
                resolve(JSON.parse(data));
              } catch {
                resolve({ raw: data });
              }
            } else {
              // Clear cached token if unauthorized (might be expired)
              if (res.statusCode === 401) {
                this.cachedToken = null;
                this.context.secrets.delete("raios.session_token");
              }
              reject(new Error(`Daemon API returned status code ${res.statusCode}: ${data}`));
            }
          });
        }
      );

      req.on("error", (err) => {
        reject(err);
      });

      req.on("timeout", () => {
        req.destroy();
        reject(new Error("Request to daemon timed out"));
      });

      if (postData) {
        req.write(postData);
      }
      req.end();
    });
  }

  /**
   * Router to handle message bridge calls from WebView
   */
  public async handleMessage(message: any, webview: vscode.Webview): Promise<void> {
    if (message.type === "fetch") {
      const { requestId, endpoint, method, body } = message;
      try {
        const response = await this.request(endpoint, method, body);
        webview.postMessage({
          type: "fetchResponse",
          requestId,
          success: true,
          data: response
        });
      } catch (err: any) {
        webview.postMessage({
          type: "fetchResponse",
          requestId,
          success: false,
          error: err.message || "Unknown error"
        });
      }
    }
  }
}
