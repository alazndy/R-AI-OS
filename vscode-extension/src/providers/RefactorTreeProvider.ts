import * as vscode from "vscode";
import * as path from "path";

export interface RefactorFileData {
  file: string;
  severity: "HIGH" | "MEDIUM";
  lines: number;
  reasons: string[];
}

type NodeKind = "category" | "file" | "reason" | "settings" | "setting-item";

class RefactorNode extends vscode.TreeItem {
  constructor(
    public readonly kind: NodeKind,
    label: string,
    collapsible: vscode.TreeItemCollapsibleState,
    public readonly fileData?: RefactorFileData
  ) {
    super(label, collapsible);
  }
}

export class RefactorTreeProvider
  implements vscode.TreeDataProvider<RefactorNode>, vscode.Disposable
{
  private readonly emitter = new vscode.EventEmitter<
    RefactorNode | undefined
  >();
  readonly onDidChangeTreeData = this.emitter.event;

  private highFiles: RefactorFileData[] = [];
  private medFiles: RefactorFileData[] = [];

  update(files: RefactorFileData[]): void {
    this.highFiles = files.filter((f) => f.severity === "HIGH");
    this.medFiles = files.filter((f) => f.severity === "MEDIUM");
    this.emitter.fire(undefined);
  }

  getTreeItem(el: RefactorNode): vscode.TreeItem {
    return el;
  }

  getChildren(el?: RefactorNode): RefactorNode[] {
    if (!el) return this.roots();
    if (el.kind === "category") {
      const files = el.label?.toString().startsWith("HIGH")
        ? this.highFiles
        : this.medFiles;
      return files.map((f) => this.fileNode(f));
    }
    if (el.kind === "file" && el.fileData) {
      return el.fileData.reasons.map((r) => {
        const n = new RefactorNode(
          "reason",
          r,
          vscode.TreeItemCollapsibleState.None
        );
        n.iconPath = new vscode.ThemeIcon("info");
        return n;
      });
    }
    if (el.kind === "settings") return this.settingNodes();
    return [];
  }

  private roots(): RefactorNode[] {
    const high = new RefactorNode(
      "category",
      `HIGH  (${this.highFiles.length})`,
      this.highFiles.length > 0
        ? vscode.TreeItemCollapsibleState.Expanded
        : vscode.TreeItemCollapsibleState.None
    );
    high.iconPath = new vscode.ThemeIcon(
      "error",
      new vscode.ThemeColor("editorError.foreground")
    );

    const med = new RefactorNode(
      "category",
      `MEDIUM  (${this.medFiles.length})`,
      this.medFiles.length > 0
        ? vscode.TreeItemCollapsibleState.Collapsed
        : vscode.TreeItemCollapsibleState.None
    );
    med.iconPath = new vscode.ThemeIcon(
      "warning",
      new vscode.ThemeColor("editorWarning.foreground")
    );

    const cfg = new RefactorNode(
      "settings",
      "Settings",
      vscode.TreeItemCollapsibleState.Collapsed
    );
    cfg.iconPath = new vscode.ThemeIcon("settings-gear");

    return [high, med, cfg];
  }

  private fileNode(f: RefactorFileData): RefactorNode {
    const n = new RefactorNode(
      "file",
      path.basename(f.file),
      vscode.TreeItemCollapsibleState.Collapsed,
      f
    );
    n.description = `${f.lines} ln`;
    n.tooltip = f.file;
    n.resourceUri = vscode.Uri.file(f.file);
    n.command = {
      command: "vscode.open",
      title: "Open File",
      arguments: [vscode.Uri.file(f.file)],
    };
    n.iconPath =
      f.severity === "HIGH"
        ? new vscode.ThemeIcon(
            "error",
            new vscode.ThemeColor("editorError.foreground")
          )
        : new vscode.ThemeIcon(
            "warning",
            new vscode.ThemeColor("editorWarning.foreground")
          );
    return n;
  }

  private settingNodes(): RefactorNode[] {
    const cfg = vscode.workspace.getConfiguration("raios.refactor");

    const makeItem = (label: string, value: string): RefactorNode => {
      const n = new RefactorNode(
        "setting-item",
        label,
        vscode.TreeItemCollapsibleState.None
      );
      n.description = value;
      n.iconPath = new vscode.ThemeIcon("symbol-constant");
      return n;
    };

    const items: RefactorNode[] = [
      makeItem("High line limit", String(cfg.get("highLineThreshold", 500))),
      makeItem("Medium line limit", String(cfg.get("mediumLineThreshold", 300))),
      makeItem("High unwrap limit", String(cfg.get("highUnwrapThreshold", 10))),
      makeItem("Medium unwrap limit", String(cfg.get("mediumUnwrapThreshold", 5))),
      makeItem("High nesting depth", String(cfg.get("highNestingThreshold", 10))),
      makeItem("Medium nesting depth", String(cfg.get("mediumNestingThreshold", 8))),
    ];

    const extRaw = cfg.get<Record<string, unknown>>("extensions", {});
    for (const [ext, overrides] of Object.entries(extRaw)) {
      items.push(makeItem(`.${ext}`, JSON.stringify(overrides)));
    }

    const open = new RefactorNode(
      "setting-item",
      "Open Refactor Settings…",
      vscode.TreeItemCollapsibleState.None
    );
    open.iconPath = new vscode.ThemeIcon("settings");
    open.command = {
      command: "workbench.action.openSettings",
      title: "Open Settings",
      arguments: ["raios.refactor"],
    };
    items.push(open);

    return items;
  }

  dispose(): void {
    this.emitter.dispose();
  }
}
