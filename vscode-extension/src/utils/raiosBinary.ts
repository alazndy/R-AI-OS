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

/**
 * Resolve the full path to the aiosd daemon binary.
 * Checks the same locations as raios, plus sibling of the raios binary.
 */
export function resolveAiosdBinary(): string | null {
  const home = os.homedir();
  const isWindows = process.platform === "win32";
  const exe = isWindows ? "aiosd.exe" : "aiosd";

  // Check sibling of the resolved raios binary first
  const raiosBin = resolveRaiosBinary();
  const siblingDir = path.dirname(raiosBin);
  const sibling = path.join(siblingDir, exe);
  try {
    if (fs.existsSync(sibling)) {
      return sibling;
    }
  } catch { /* continue */ }

  const candidates = [
    path.join(home, ".cargo", "bin", exe),
    path.join(home, ".aios", exe),
    path.join("/usr/local/bin", exe),
    path.join("/usr/bin", exe),
  ];

  for (const candidate of candidates) {
    try {
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    } catch { /* continue */ }
  }

  return null;
}

/**
 * Returns the platform-specific R-AI-OS config directory, where the daemon
 * writes its session/IPC token files.
 */
export function raiosConfigDir(): string {
  const isWindows = process.platform === "win32";
  const configBase = isWindows
    ? process.env.APPDATA || path.join(os.homedir(), "AppData", "Roaming")
    : path.join(os.homedir(), ".config");
  return path.join(configBase, "raios");
}

/**
 * Returns the path where the daemon writes its session token.
 */
export function tokenFilePath(): string {
  return path.join(raiosConfigDir(), ".session_token");
}
