import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import * as vscode from "vscode";

/**
 * Resolve the full path to the raios executable.
 * VS Code processes on Windows don't always inherit the shell PATH,
 * so we check known installation locations explicitly.
 */
export function resolveRaiosBinary(): string {
  const config = vscode.workspace.getConfiguration("raios");
  const configPath = config.get<string>("pathToBinary");
  if (configPath && configPath !== "raios") {
    try {
      if (fs.existsSync(configPath)) {
        return configPath;
      }
    } catch {
      // continue
    }
  }

  const home = os.homedir();
  const isWindows = process.platform === "win32";
  const exe = isWindows ? "raios.exe" : "raios";

  const candidates = [
    // cargo install default location
    path.join(home, ".cargo", "bin", exe),
    // .aios install location (used by R-AI-OS bootstrap)
    path.join(home, ".aios", exe),
    // /usr/local/bin (Linux/Mac)
    path.join("/usr/local/bin", exe),
    // /usr/bin (Linux)
    path.join("/usr/bin", exe),
  ];

  for (const candidate of candidates) {
    try {
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    } catch {
      // continue
    }
  }

  // Fallback: rely on PATH (may work if VS Code was launched from terminal)
  return "raios";
}
