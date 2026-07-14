# Constitution Editor & Creator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the R-AI-OS TUI's hardcoded "System Rules" panel with a real, editable Constitution panel that outlines/edits `AGENT_CONSTITUTION.md` and per-project `CLAUDE.md`/`AGENTS.md`/`GEMINI.md` files, plus a guided creator flow for new project-specific constitution files.

**Architecture:** A new pure-logic `raios-runtime::constitution` module parses `##`/`###` markdown headers into a section tree and discovers per-project constitution files; a new `save_constitution_file()` in `raios-runtime::filebrowser` adds timestamped backups (keeping the last 5) before any write. On the TUI side, a new `ConstitutionState` drives three interaction surfaces inside the existing menu-index-1 slot: an **outline** (navigate/inline-edit/add/delete rows, reusing the flattened section tree), **raw edit** (100% reuse of the existing generic `FileEdit`/`Editor` machinery — no new text editor), and **creator** (guided new-file wizard). A save-confirmation modal, modeled directly on the existing `handover_modal` pattern, gates every write through a diff preview.

**Tech Stack:** Rust 2021, ratatui/crossterm TUI (`raios-surface-tui` crate), pure logic in `raios-runtime` crate, existing `Config`/`FileEntry`/`Editor`/`simple_diff` primitives — no new external crates.

## Global Constraints

- Repo root: `/home/alaz/dev/core/R-AI-OS`. Workspace crates referenced: `raios-runtime` (package name `raios-runtime`), `raios-surface-tui` (package name `raios-surface-tui`).
- Do not modify `get_master_rule_files()`, `get_policy_files()`, `get_agent_config_files()`, or anything in `raios-surface-cli` — those back the unrelated `raios rules` CLI command and other panels, and are out of scope.
- Every write to a constitution file (global or project) must go through `save_constitution_file()` — never call `save_file_content()` directly for a constitution target.
- Follow existing repo test convention: pure-logic modules get `#[cfg(test)] mod tests { use super::*; ... }` at the bottom of the same file (see `crates/raios-runtime/src/compressor/diff.rs` for the exact style already in this repo). Ratatui render functions and keyboard-dispatch wiring are not unit-tested anywhere in this codebase (`ui/panels/*.rs` has zero tests) — for those tasks, verification is `cargo check`/`cargo clippy` plus a manual run, matching existing practice; do not invent fake UI tests.
- Run `cargo test -p raios-runtime` and/or `cargo test -p raios-surface-tui` (as relevant to the task) before every commit in this plan. Both must stay green with 0 new clippy warnings (`cargo clippy -p raios-runtime -p raios-surface-tui -- -D warnings`).
- Commit messages: English, short, imperative (e.g. `feat: add constitution section parser`), matching this repo's existing log.

---

### Task 1: Constitution parser & project-file discovery

**Files:**
- Create: `crates/raios-runtime/src/constitution.rs`
- Modify: `crates/raios-runtime/src/lib.rs` (add `pub mod constitution;` after the existing `pub mod filebrowser;` line)

**Interfaces:**
- Produces: `raios_runtime::constitution::ConstitutionSection { level: u8, title: String, line_start: usize, line_end: usize, items: Vec<String>, children: Vec<ConstitutionSection> }`
- Produces: `raios_runtime::constitution::parse_sections(content: &str) -> Vec<ConstitutionSection>`
- Produces: `raios_runtime::constitution::ProjectFileKind` (enum: `ClaudeMd`, `AgentsMd`, `GeminiMd`), with `.filename(&self) -> &'static str`
- Produces: `raios_runtime::constitution::discover_project_constitution_files(project_root: &Path) -> Vec<(ProjectFileKind, PathBuf)>`
- Produces: `raios_runtime::constitution::is_include_only(content: &str) -> bool`

- [ ] **Step 1: Write the failing tests**

Create `crates/raios-runtime/src/constitution.rs` with just the test module first:

```rust
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_top_level_sections_and_items() {
        let content = "\
## 1. Identity\n\
* System Name: k-ai-ra\n\
* Role: Partner\n\
\n\
## 2. Standard\n\
Every task follows this loop.\n\
1. Requirement\n\
2. Investigation\n";
        let sections = parse_sections(content);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].title, "1. Identity");
        assert_eq!(sections[0].items, vec!["System Name: k-ai-ra", "Role: Partner"]);
        assert_eq!(sections[1].title, "2. Standard");
        assert_eq!(
            sections[1].items,
            vec!["Every task follows this loop.", "Requirement", "Investigation"]
        );
    }

    #[test]
    fn parses_nested_subsections_as_children() {
        let content = "\
## 4. Engineering Standards\n\
### AgentShield: Absolute OWASP Rules\n\
1. **Broken Access Control:** Enforce least privilege.\n\
2. **Cryptographic Failures:** No custom crypto.\n\
\n\
## 5. Communication\n\
Turkish in chat.\n";
        let sections = parse_sections(content);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].title, "4. Engineering Standards");
        assert!(sections[0].items.is_empty());
        assert_eq!(sections[0].children.len(), 1);
        assert_eq!(sections[0].children[0].title, "AgentShield: Absolute OWASP Rules");
        assert_eq!(
            sections[0].children[0].items,
            vec![
                "**Broken Access Control:** Enforce least privilege.",
                "**Cryptographic Failures:** No custom crypto.",
            ]
        );
        assert_eq!(sections[1].title, "5. Communication");
    }

    #[test]
    fn line_ranges_cover_header_through_last_body_line() {
        let content = "## A\nfoo\nbar\n## B\nbaz\n";
        let sections = parse_sections(content);
        assert_eq!(sections[0].line_start, 0);
        assert_eq!(sections[0].line_end, 2);
        assert_eq!(sections[1].line_start, 3);
        assert_eq!(sections[1].line_end, 4);
    }

    #[test]
    fn no_headers_yields_empty_sections() {
        assert!(parse_sections("just some text\nno headers here\n").is_empty());
        assert!(parse_sections("").is_empty());
    }

    #[test]
    fn include_only_file_detected() {
        assert!(is_include_only("@/home/alaz/AGENT_CONSTITUTION.md\n"));
        assert!(is_include_only("\n@/home/alaz/AGENT_CONSTITUTION.md\n\n"));
        assert!(!is_include_only("## 1. Identity\nSome real content\n"));
        assert!(!is_include_only(""));
    }

    #[test]
    fn discover_project_files_finds_existing_ones_only() {
        let dir = std::env::temp_dir().join(format!(
            "raios-constitution-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("CLAUDE.md"), "@/home/alaz/AGENT_CONSTITUTION.md\n").unwrap();
        // AGENTS.md and GEMINI.md deliberately absent

        let found = discover_project_constitution_files(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, ProjectFileKind::ClaudeMd);
        assert_eq!(found[0].1, dir.join("CLAUDE.md"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
```

- [ ] **Step 2: Run tests to verify they fail to compile**

Run: `cargo test -p raios-runtime --lib constitution:: 2>&1 | tail -30`
Expected: compile errors — `parse_sections`, `is_include_only`, `discover_project_constitution_files`, `ProjectFileKind` not found.

- [ ] **Step 3: Implement the parser and discovery functions**

Add above the `#[cfg(test)]` block in `crates/raios-runtime/src/constitution.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ConstitutionSection {
    pub level: u8,
    pub title: String,
    pub line_start: usize,
    pub line_end: usize,
    pub items: Vec<String>,
    pub children: Vec<ConstitutionSection>,
}

pub fn parse_sections(content: &str) -> Vec<ConstitutionSection> {
    let lines: Vec<&str> = content.lines().collect();
    let mut sections = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some(title) = lines[i].strip_prefix("## ") {
            let start = i;
            i += 1;
            let (items, children, end) = parse_body(&lines, &mut i, start);
            sections.push(ConstitutionSection {
                level: 1,
                title: title.trim().to_string(),
                line_start: start,
                line_end: end,
                items,
                children,
            });
        } else {
            i += 1;
        }
    }
    sections
}

/// Consumes lines starting at `*i` until the next `## ` header (exclusive), collecting
/// plain body lines as `items` and `### ` headers as `children`. Returns the last
/// non-empty line index consumed (falls back to `fallback_line` if nothing was consumed).
fn parse_body(
    lines: &[&str],
    i: &mut usize,
    fallback_line: usize,
) -> (Vec<String>, Vec<ConstitutionSection>, usize) {
    let mut items = Vec::new();
    let mut children = Vec::new();
    let mut last = fallback_line;
    while *i < lines.len() {
        let line = lines[*i];
        if line.starts_with("## ") {
            break;
        }
        if let Some(title) = line.strip_prefix("### ") {
            let start = *i;
            *i += 1;
            let (child_items, _grandchildren, end) = parse_body(lines, i, start);
            children.push(ConstitutionSection {
                level: 2,
                title: title.trim().to_string(),
                line_start: start,
                line_end: end,
                items: child_items,
                children: Vec::new(),
            });
            last = end;
            continue;
        }
        if !line.trim().is_empty() {
            items.push(strip_list_marker(line));
            last = *i;
        }
        *i += 1;
    }
    (items, children, last)
}

fn strip_list_marker(line: &str) -> String {
    let t = line.trim();
    if let Some(rest) = t.strip_prefix("* ") {
        return rest.to_string();
    }
    if let Some(rest) = t.strip_prefix("- ") {
        return rest.to_string();
    }
    if let Some(dot) = t.find(". ") {
        if !t[..dot].is_empty() && t[..dot].chars().all(|c| c.is_ascii_digit()) {
            return t[dot + 2..].to_string();
        }
    }
    t.to_string()
}

pub fn is_include_only(content: &str) -> bool {
    let meaningful: Vec<&str> = content.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    !meaningful.is_empty() && meaningful.iter().all(|l| l.starts_with('@'))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectFileKind {
    ClaudeMd,
    AgentsMd,
    GeminiMd,
}

impl ProjectFileKind {
    pub fn filename(&self) -> &'static str {
        match self {
            ProjectFileKind::ClaudeMd => "CLAUDE.md",
            ProjectFileKind::AgentsMd => "AGENTS.md",
            ProjectFileKind::GeminiMd => "GEMINI.md",
        }
    }
}

pub fn discover_project_constitution_files(project_root: &Path) -> Vec<(ProjectFileKind, PathBuf)> {
    [ProjectFileKind::ClaudeMd, ProjectFileKind::AgentsMd, ProjectFileKind::GeminiMd]
        .into_iter()
        .filter_map(|kind| {
            let p = project_root.join(kind.filename());
            p.exists().then_some((kind, p))
        })
        .collect()
}
```

- [ ] **Step 4: Register the module**

In `crates/raios-runtime/src/lib.rs`, add this line directly after `pub mod filebrowser;`:

```rust
pub mod constitution;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p raios-runtime --lib constitution:: 2>&1 | tail -30`
Expected: `test result: ok. 6 passed; 0 failed`

- [ ] **Step 6: Commit**

```bash
git add crates/raios-runtime/src/constitution.rs crates/raios-runtime/src/lib.rs
git commit -m "feat: add constitution section parser and project-file discovery"
```

---

### Task 2: Save-safety helper (backup + prune)

**Files:**
- Modify: `crates/raios-runtime/src/filebrowser/files.rs`
- Modify: `crates/raios-runtime/src/filebrowser/mod.rs` (export the new function)

**Interfaces:**
- Consumes: `raios_runtime::filebrowser::save_file_content(path: &Path, content: &str) -> std::io::Result<()>` (already exists, `files.rs:117-119`)
- Produces: `raios_runtime::filebrowser::save_constitution_file(path: &Path, new_content: &str) -> std::io::Result<()>`

- [ ] **Step 1: Write the failing tests**

Add to the bottom of `crates/raios-runtime/src/filebrowser/files.rs`:

```rust
#[cfg(test)]
mod constitution_save_tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "raios-save-test-{}-{}",
            std::process::id(),
            name
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("CONSTITUTION.md")
    }

    #[test]
    fn first_save_with_no_prior_file_creates_no_backup() {
        let path = temp_file("first");
        save_constitution_file(&path, "## 1. Hello\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "## 1. Hello\n");
        let dir = path.parent().unwrap();
        let backups: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".bak."))
            .collect();
        assert!(backups.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn second_save_backs_up_the_previous_content() {
        let path = temp_file("second");
        save_constitution_file(&path, "version one\n").unwrap();
        save_constitution_file(&path, "version two\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "version two\n");

        let dir = path.parent().unwrap();
        let backups: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".bak."))
            .collect();
        assert_eq!(backups.len(), 1);
        let backup_content = std::fs::read_to_string(backups[0].path()).unwrap();
        assert_eq!(backup_content, "version one\n");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn keeps_only_five_most_recent_backups() {
        let path = temp_file("prune");
        for i in 0..7 {
            save_constitution_file(&path, &format!("version {}\n", i)).unwrap();
            // Force distinct backup timestamps even on fast filesystems/CI.
            std::thread::sleep(std::time::Duration::from_millis(1100));
        }
        let dir = path.parent().unwrap();
        let backups: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".bak."))
            .collect();
        assert_eq!(backups.len(), 5, "expected exactly 5 backups, found {}", backups.len());
        std::fs::remove_dir_all(dir).ok();
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p raios-runtime --lib constitution_save_tests:: 2>&1 | tail -30`
Expected: compile error — `save_constitution_file` not found.

- [ ] **Step 3: Implement `save_constitution_file`**

Add to `crates/raios-runtime/src/filebrowser/files.rs`, directly after the existing `save_file_content` function (currently ending at line 119):

```rust
/// Writes `new_content` to `path`, first backing up any existing content to
/// `<path>.bak.<unix_timestamp>` and pruning backups beyond the 5 most recent.
/// Used exclusively for constitution files (global + per-project) so a bad
/// edit to the single file every agent reads is always recoverable.
pub fn save_constitution_file(path: &Path, new_content: &str) -> std::io::Result<()> {
    if path.exists() {
        let existing = fs::read_to_string(path).unwrap_or_default();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let backup_path = PathBuf::from(format!("{}.bak.{}", path.display(), ts));
        fs::write(&backup_path, existing)?;
        prune_old_backups(path)?;
    }
    save_file_content(path, new_content)
}

fn prune_old_backups(path: &Path) -> std::io::Result<()> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let prefix = format!("{}.bak.", file_name);

    let mut backups: Vec<(u64, PathBuf)> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let ts_str = name.strip_prefix(&prefix)?;
            let ts: u64 = ts_str.parse().ok()?;
            Some((ts, e.path()))
        })
        .collect();

    backups.sort_by_key(|(ts, _)| std::cmp::Reverse(*ts));
    for (_, old_path) in backups.into_iter().skip(5) {
        let _ = fs::remove_file(old_path);
    }
    Ok(())
}
```

- [ ] **Step 4: Export it from the filebrowser module**

In `crates/raios-runtime/src/filebrowser/mod.rs`, change:

```rust
pub use files::{
    discover_memory_files, find_file_by_name, get_agent_config_files, get_master_rule_files,
    get_mempalace_files, get_policy_files, load_file_content, save_file_content,
};
```

to:

```rust
pub use files::{
    discover_memory_files, find_file_by_name, get_agent_config_files, get_master_rule_files,
    get_mempalace_files, get_policy_files, load_file_content, save_constitution_file,
    save_file_content,
};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p raios-runtime --lib constitution_save_tests:: 2>&1 | tail -30`
Expected: `test result: ok. 3 passed; 0 failed` (the prune test takes ~8 seconds due to the sleeps — that's expected).

- [ ] **Step 6: Commit**

```bash
git add crates/raios-runtime/src/filebrowser/files.rs crates/raios-runtime/src/filebrowser/mod.rs
git commit -m "feat: add backup-and-prune safe save for constitution files"
```

---

### Task 3: TUI state types for the Constitution panel

**Files:**
- Modify: `crates/raios-surface-tui/src/app/state.rs`
- Modify: `crates/raios-surface-tui/src/app/mod.rs`

**Interfaces:**
- Consumes: `raios_runtime::constitution::{ConstitutionSection, ProjectFileKind}` (Task 1)
- Produces: `ConstitutionTarget` (enum: `Global { path: PathBuf }`, `ProjectFile { path: PathBuf, kind: ProjectFileKind }`), with `.path(&self) -> &Path` and `.label(&self) -> String`
- Produces: `OutlineRow` (enum: `Section { idx: usize }`, `Child { idx: usize, child_idx: usize }`, `Item { idx: usize, child_idx: Option<usize>, item_idx: usize }`)
- Produces: `flatten_sections(sections: &[ConstitutionSection]) -> Vec<OutlineRow>`
- Produces: `CreatorStep` (enum: `ChooseTarget`, `Notes`, `Preview`, `Default` = `ChooseTarget`)
- Produces: `CreatorState { active: bool, target_is_global: bool, step: CreatorStep, notes_input: String }` (all `Default`-derived)
- Produces: `PendingConstitutionSave { path: PathBuf, new_content: String, diff_lines: Vec<String>, added: usize, removed: usize }`
- Produces: `ConstitutionState { tabs: Vec<ConstitutionTarget>, active_tab: usize, sections: Vec<ConstitutionSection>, rows: Vec<OutlineRow>, outline_cursor: usize, item_editing: bool, item_input: String, pending_save: Option<PendingConstitutionSave>, creator: CreatorState }` (Default-derived). Outline-vs-Creator display is driven entirely by `creator.active` — no separate mode enum, to avoid a second source of truth.
- Produces: `App.constitution: ConstitutionState` field
- Removes: `RuleCategory`, `system_rules()`, `InventoryState.system_rules`

- [ ] **Step 1: Add the new types to `state.rs`**

Read the surrounding code first: `RuleCategory` is defined at `crates/raios-surface-tui/src/app/state.rs:283-286`, and `system_rules()` at the bottom of the file (`crates/raios-surface-tui/src/app/state.rs:455-490`, per current layout). Delete both, and delete the `system_rules: Vec<RuleCategory>` field from `InventoryState` (`state.rs:393`).

Replace the `RuleCategory` struct definition with:

```rust
// ─── Constitution State ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum ConstitutionTarget {
    Global { path: PathBuf },
    ProjectFile {
        path: PathBuf,
        kind: raios_runtime::constitution::ProjectFileKind,
    },
}

impl ConstitutionTarget {
    pub fn path(&self) -> &std::path::Path {
        match self {
            ConstitutionTarget::Global { path } => path,
            ConstitutionTarget::ProjectFile { path, .. } => path,
        }
    }

    pub fn label(&self) -> String {
        match self {
            ConstitutionTarget::Global { .. } => "Global Constitution".to_string(),
            ConstitutionTarget::ProjectFile { kind, .. } => kind.filename().to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineRow {
    Section { idx: usize },
    Child { idx: usize, child_idx: usize },
    Item {
        idx: usize,
        child_idx: Option<usize>,
        item_idx: usize,
    },
}

pub fn flatten_sections(
    sections: &[raios_runtime::constitution::ConstitutionSection],
) -> Vec<OutlineRow> {
    let mut rows = Vec::new();
    for (idx, sec) in sections.iter().enumerate() {
        rows.push(OutlineRow::Section { idx });
        for item_idx in 0..sec.items.len() {
            rows.push(OutlineRow::Item { idx, child_idx: None, item_idx });
        }
        for (child_idx, child) in sec.children.iter().enumerate() {
            rows.push(OutlineRow::Child { idx, child_idx });
            for item_idx in 0..child.items.len() {
                rows.push(OutlineRow::Item {
                    idx,
                    child_idx: Some(child_idx),
                    item_idx,
                });
            }
        }
    }
    rows
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreatorStep {
    ChooseTarget,
    Notes,
    Preview,
}

impl Default for CreatorStep {
    fn default() -> Self {
        CreatorStep::ChooseTarget
    }
}

#[derive(Debug, Default, Clone)]
pub struct CreatorState {
    pub active: bool,
    pub target_is_global: bool,
    pub step: CreatorStep,
    pub notes_input: String,
}

#[derive(Debug, Clone)]
pub struct PendingConstitutionSave {
    pub path: PathBuf,
    pub new_content: String,
    pub diff_lines: Vec<String>,
    pub added: usize,
    pub removed: usize,
}

#[derive(Debug, Default)]
pub struct ConstitutionState {
    pub tabs: Vec<ConstitutionTarget>,
    pub active_tab: usize,
    pub sections: Vec<raios_runtime::constitution::ConstitutionSection>,
    pub rows: Vec<OutlineRow>,
    pub outline_cursor: usize,
    pub item_editing: bool,
    pub item_input: String,
    pub pending_save: Option<PendingConstitutionSave>,
    pub creator: CreatorState,
}
```

Remove the `system_rules: Vec<RuleCategory>` field from `InventoryState`.

- [ ] **Step 2: Wire `constitution: ConstitutionState` into `App`**

In `crates/raios-surface-tui/src/app/mod.rs`:

1. Add a field to the `App` struct (next to the existing `pub wizard: WizardState,` field, around line 210):

```rust
    // Constitution Editor
    pub constitution: ConstitutionState,
```

2. In both `Self { ... }` constructors (`new()` around line 274 and `new_remote()` around line 334), remove `system_rules: raios_surface_tui::app::state::system_rules(),` from the `InventoryState { ... }` literal (leaving `InventoryState { ..Default::default() }` if it becomes the only override, or dropping the whole override if `InventoryState` needs no fields set — check what else is set there; currently nothing else is, so replace:

```rust
            inventory: InventoryState {
                system_rules: raios_surface_tui::app::state::system_rules(),
                ..Default::default()
            },
```

with:

```rust
            inventory: InventoryState::default(),
```

in both places, and add `constitution: ConstitutionState::default(),` as a new line in both `Self { ... }` literals.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p raios-surface-tui 2>&1 | tail -60`
Expected: errors only about `render_rules` / `rules.rs` still referencing the now-deleted `system_rules`/`RuleCategory` (that panel is replaced in Task 4) — no other new errors. If any other file references `system_rules()` or `RuleCategory`, note the file path; it will be handled in Task 4.

- [ ] **Step 4: Commit**

```bash
git add crates/raios-surface-tui/src/app/state.rs crates/raios-surface-tui/src/app/mod.rs
git commit -m "feat: add ConstitutionState types and wire into App"
```
(This commit is expected to leave `cargo check` red on `rules.rs` until Task 4 — that's fine, it's an intermediate step in the same feature branch, not a point where the branch needs to build cleanly.)

---

### Task 4: Outline panel rendering + menu wiring

**Files:**
- Delete: `crates/raios-surface-tui/src/ui/panels/rules.rs`
- Create: `crates/raios-surface-tui/src/ui/panels/constitution.rs`
- Modify: `crates/raios-surface-tui/src/ui/panels/mod.rs`
- Modify: `crates/raios-surface-tui/src/ui/panels/content.rs`
- Modify: `crates/raios-surface-tui/src/app/mod.rs` (loading logic + `current_menu_files`)

**Interfaces:**
- Consumes: `ConstitutionState`, `OutlineRow`, `ConstitutionTarget`, `flatten_sections` (Task 3); `raios_runtime::constitution::{parse_sections, discover_project_constitution_files, is_include_only}`, `raios_core::safe_io`-backed `load_file_content` (existing)
- Produces: `pub fn render_constitution(frame: &mut Frame, area: Rect, app: &App)`
- Produces: `App::load_constitution_tab(&mut self, idx: usize)` — loads content for `self.constitution.tabs[idx]` into `sections`/`rows`, resets `outline_cursor` to 0
- Produces: `App::refresh_constitution_tabs(&mut self)` — rebuilds `self.constitution.tabs` from `self.config.master_md_path` + `self.projects.active` and calls `load_constitution_tab(0)`

- [ ] **Step 1: Delete the old panel and remove its exports**

```bash
git rm crates/raios-surface-tui/src/ui/panels/rules.rs
```

In `crates/raios-surface-tui/src/ui/panels/mod.rs`, remove `pub mod rules;` and `pub use rules::*;`.

- [ ] **Step 2: Create the new panel renderer**

Create `crates/raios-surface-tui/src/ui/panels/constitution.rs`:

```rust
use raios_surface_tui::app::state::OutlineRow;
use raios_surface_tui::app::App;
use raios_surface_tui::ui::*;
use ratatui::{
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span, Text},
    widgets::Paragraph,
    Frame,
};

pub fn render_constitution(frame: &mut Frame, area: Rect, app: &App) {
    if app.constitution.creator.active {
        render_creator(frame, area, app);
        return;
    }

    let mut lines = vec![render_tab_bar(app), Line::from("")];

    if app.constitution.sections.is_empty() {
        let target_path = app
            .constitution
            .tabs
            .get(app.constitution.active_tab)
            .map(|t| t.path().to_path_buf())
            .unwrap_or_default();
        let content = raios_runtime::filebrowser::load_file_content(&target_path);
        if raios_runtime::constitution::is_include_only(&content) {
            lines.push(Line::from(vec![Span::styled(
                " ↳ includes: AGENT_CONSTITUTION.md — press [1] to edit the real content",
                Style::new().fg(DIM),
            )]));
        } else {
            lines.push(Line::from(Span::styled(
                " (empty or unparsed — press [r] to raw-edit)",
                Style::new().fg(DIM),
            )));
        }
        frame.render_widget(Paragraph::new(Text::from(lines)), area);
        return;
    }

    for (row_idx, row) in app.constitution.rows.iter().enumerate() {
        let selected = row_idx == app.constitution.outline_cursor;
        lines.push(render_row(app, row, selected));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn render_tab_bar(app: &App) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, tab) in app.constitution.tabs.iter().enumerate() {
        let marker = if i == app.constitution.active_tab { GREEN } else { DIM };
        spans.push(Span::styled(
            format!(" [{}] {} ", i + 1, tab.label()),
            Style::new().fg(marker).bold(),
        ));
    }
    Line::from(spans)
}

fn render_row(app: &App, row: &OutlineRow, selected: bool) -> Line<'static> {
    let prefix = if selected { "▶ " } else { "  " };
    let base_style = if selected {
        Style::new().fg(GREEN).bold()
    } else {
        Style::new().fg(MID)
    };
    match *row {
        OutlineRow::Section { idx } => {
            let title = app.constitution.sections[idx].title.clone();
            Line::from(Span::styled(format!("{}◈ {}", prefix, title), base_style))
        }
        OutlineRow::Child { idx, child_idx } => {
            let title = app.constitution.sections[idx].children[child_idx].title.clone();
            Line::from(Span::styled(format!("{}  ◦ {}", prefix, title), base_style))
        }
        OutlineRow::Item { idx, child_idx, item_idx } => {
            let text = if let Some(c) = child_idx {
                if selected && app.constitution.item_editing {
                    app.constitution.item_input.clone()
                } else {
                    app.constitution.sections[idx].children[c].items[item_idx].clone()
                }
            } else if selected && app.constitution.item_editing {
                app.constitution.item_input.clone()
            } else {
                app.constitution.sections[idx].items[item_idx].clone()
            };
            let indent = if child_idx.is_some() { "      " } else { "    " };
            Line::from(Span::styled(format!("{}{}• {}", prefix, indent, text), base_style))
        }
    }
}

fn render_creator(frame: &mut Frame, area: Rect, app: &App) {
    use raios_surface_tui::app::state::CreatorStep;
    let c = &app.constitution.creator;
    let mut lines = vec![
        Line::from(Span::styled(" CONSTITUTION CREATOR", Style::new().fg(MID).bold())),
        Line::from(""),
    ];
    match c.step {
        CreatorStep::ChooseTarget => {
            lines.push(Line::from(" [p] Project-specific file   [g] Global (from scratch, requires confirm)"));
        }
        CreatorStep::Notes => {
            lines.push(Line::from(" Project-specific notes (appended as \"## Project-Specific Rules\"):"));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(format!(" {}█", c.notes_input), Style::new().fg(GREEN))));
        }
        CreatorStep::Preview => {
            lines.push(Line::from(" Preview — [Enter] to save, [Esc] to cancel:"));
            lines.push(Line::from(""));
            if c.target_is_global {
                lines.push(Line::from(Span::styled(" ⚠ Overwriting the GLOBAL constitution file.", Style::new().fg(AMBER).bold())));
            } else {
                lines.push(Line::from("@/home/alaz/AGENT_CONSTITUTION.md"));
                lines.push(Line::from(""));
                lines.push(Line::from("## Project-Specific Rules"));
                for line in c.notes_input.lines() {
                    lines.push(Line::from(line.to_string()));
                }
            }
        }
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}
```

- [ ] **Step 3: Wire the export and content dispatch**

In `crates/raios-surface-tui/src/ui/panels/mod.rs`, add `pub mod constitution;` and `pub use constitution::*;` (alphabetically, before `pub mod content;`/`pub use content::*;`).

In `crates/raios-surface-tui/src/ui/panels/content.rs`, change:

```rust
        1 => render_rules(frame, inner, app),
```

to:

```rust
        1 => render_constitution(frame, inner, app),
```

- [ ] **Step 4: Add tab-loading logic to `App`**

In `crates/raios-surface-tui/src/app/mod.rs`, add these methods in the `impl App` block (near `current_menu_files`, around line 429):

```rust
    pub fn refresh_constitution_tabs(&mut self) {
        let mut tabs = vec![raios_surface_tui::app::state::ConstitutionTarget::Global {
            path: self.config.master_md_path.clone(),
        }];
        if let Some(ref proj) = self.projects.active {
            for (kind, path) in raios_runtime::constitution::discover_project_constitution_files(&proj.local_path) {
                tabs.push(raios_surface_tui::app::state::ConstitutionTarget::ProjectFile { path, kind });
            }
        }
        self.constitution.tabs = tabs;
        self.constitution.active_tab = 0;
        self.load_constitution_tab(0);
    }

    pub fn load_constitution_tab(&mut self, idx: usize) {
        let Some(target) = self.constitution.tabs.get(idx) else { return };
        let content = raios_runtime::filebrowser::load_file_content(target.path());
        self.constitution.sections = raios_runtime::constitution::parse_sections(&content);
        self.constitution.rows = raios_surface_tui::app::state::flatten_sections(&self.constitution.sections);
        self.constitution.outline_cursor = 0;
        self.constitution.active_tab = idx;
    }
```

Also update `current_menu_files()` (currently `1 => self.inventory.master_files.clone(),`) to stop showing the generic file list for the Constitution panel, since tabs now own that role:

```rust
            1 => vec![],
```

- [ ] **Step 5: Trigger a refresh when the Constitution menu item is selected**

In `crates/raios-surface-tui/src/app/events/keyboard/dashboard.rs`, inside the `KeyCode::Down | KeyCode::Char('j')` arm's `else if self.ui.menu_cursor < MENU_ITEMS.len() - 1` branch (around line 271-282, right after the existing extensions lazy-load block), add:

```rust
                    if self.ui.menu_cursor == 1 && self.constitution.tabs.is_empty() {
                        self.refresh_constitution_tabs();
                    }
```

And symmetrically in the `KeyCode::Up | KeyCode::Char('k')` arm's `else if self.ui.menu_cursor > 0` branch (around line 222-229):

```rust
                    if self.ui.menu_cursor == 1 && self.constitution.tabs.is_empty() {
                        self.refresh_constitution_tabs();
                    }
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo check -p raios-surface-tui 2>&1 | tail -60`
Expected: clean compile, 0 errors. Fix any remaining reference to the deleted `system_rules`/`RuleCategory`/`render_rules` symbols surfaced here.

- [ ] **Step 7: Manual verification**

Run: `cargo run -p raios-surface-tui --bin raios-tui 2>&1 | head -5` is not interactive-friendly for automated checking — instead confirm via `cargo build -p raios-surface-tui` succeeding, then launch the real binary manually in a `screen`/`tmux` session, navigate to the "System Rules" menu item (now Constitution), and confirm it shows a tab bar plus either an outline or the include-only notice instead of the old static Turkish text. Record what you observe.

- [ ] **Step 8: Commit**

```bash
git add -A crates/raios-surface-tui/src/ui/panels/ crates/raios-surface-tui/src/app/mod.rs crates/raios-surface-tui/src/app/events/keyboard/dashboard.rs
git commit -m "feat: replace hardcoded Rules panel with real Constitution outline"
```

---

### Task 5: Outline navigation + footer hints

**Files:**
- Modify: `crates/raios-surface-tui/src/app/events/keyboard/dashboard.rs`
- Modify: `crates/raios-surface-tui/src/ui/components.rs`

**Interfaces:**
- Consumes: `ConstitutionState.{outline_cursor, rows, tabs, active_tab}` (Task 3/4), `App::load_constitution_tab` (Task 4)

- [ ] **Step 1: Add outline up/down/tab-switch key handling**

In `crates/raios-surface-tui/src/app/events/keyboard/dashboard.rs`, add new match arms directly after the "End Extensions" comment (line 181) and before the `KeyCode::Char('/') | KeyCode::Tab` arm (line 182):

```rust
            // ── Constitution panel keyboard (menu_cursor == 1) ───────────────
            KeyCode::Char(n @ '1'..='9')
                if self.ui.menu_cursor == 1
                    && self.ui.right_panel_focus
                    && !self.constitution.item_editing
                    && !self.constitution.creator.active =>
            {
                let idx = (n as usize) - ('1' as usize);
                if idx < self.constitution.tabs.len() {
                    self.load_constitution_tab(idx);
                }
            }
            KeyCode::Up | KeyCode::Char('k')
                if self.ui.menu_cursor == 1
                    && self.ui.right_panel_focus
                    && !self.constitution.item_editing
                    && !self.constitution.creator.active =>
            {
                if self.constitution.outline_cursor > 0 {
                    self.constitution.outline_cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j')
                if self.ui.menu_cursor == 1
                    && self.ui.right_panel_focus
                    && !self.constitution.item_editing
                    && !self.constitution.creator.active =>
            {
                let max = self.constitution.rows.len().saturating_sub(1);
                if self.constitution.outline_cursor < max {
                    self.constitution.outline_cursor += 1;
                }
            }
            // ── End Constitution ──────────────────────────────────────────────
```

Note: this block must come *before* the generic `KeyCode::Up | KeyCode::Char('k')` arm at line 192 and `KeyCode::Down | KeyCode::Char('j')` arm at line 231 — Rust tries match arms top-to-bottom, so placing the Constitution-specific arms earlier means they win when `menu_cursor == 1`, and the generic arms still handle every other panel unchanged.

- [ ] **Step 2: Allow `→` to focus the Constitution panel**

In the same file, update the `KeyCode::Right | KeyCode::Char('l')` arm's `can_focus` computation (around line 284-289):

```rust
            KeyCode::Right | KeyCode::Char('l') => {
                let can_focus = !self.current_menu_files().is_empty()
                    || (self.ui.menu_cursor == 0 && !self.tasks.list.is_empty())
                    || (self.ui.menu_cursor == 1 && !self.constitution.tabs.is_empty())
                    || (self.ui.menu_cursor == 6 && !self.search.results.is_empty())
                    || (self.ui.menu_cursor == 7 && !self.projects.list.is_empty())
                    || (self.ui.menu_cursor == 15 && !self.ext.extensions.is_empty());
```

- [ ] **Step 3: Add footer hints**

In `crates/raios-surface-tui/src/ui/components.rs`, in the `right_panel_focus` hint `match` (currently starting `let hint = match app.ui.menu_cursor { 0 => ..., 6 => ..., 7 => ..., 15 => ..., _ => ... }`), add:

```rust
            1 => " [1-9] tab  [↑↓] navigate  [Enter/r] raw edit  [i] edit item  [n] new item  [d] delete  [c] creator  [←/Esc] menu",
```

And in the non-focused footer hint chain (the `else if app.ui.menu_cursor == N && ...` sequence a few lines below), add a branch before the final `else { "" }`:

```rust
        } else if app.ui.menu_cursor == 1 && !app.constitution.tabs.is_empty() {
            "  [→] outline"
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p raios-surface-tui 2>&1 | tail -40`
Expected: clean compile, 0 errors.

- [ ] **Step 5: Manual verification**

Launch the TUI manually, navigate to Constitution, press `→` to focus, confirm `↑`/`↓`/`j`/`k` move the `▶` cursor through outline rows, and `1`/`2` switch tabs when a project with a `CLAUDE.md` is open.

- [ ] **Step 6: Commit**

```bash
git add crates/raios-surface-tui/src/app/events/keyboard/dashboard.rs crates/raios-surface-tui/src/ui/components.rs
git commit -m "feat: add outline navigation and tab switching to Constitution panel"
```

---

### Task 6: Raw-edit jump + save-confirmation modal

**Files:**
- Modify: `crates/raios-surface-tui/src/app/events/actions.rs`
- Modify: `crates/raios-surface-tui/src/app/events/keyboard/dashboard.rs`
- Modify: `crates/raios-surface-tui/src/app/events/keyboard/mod.rs`
- Modify: `crates/raios-surface-tui/src/ui/mod.rs`
- Modify: `crates/raios-surface-tui/src/ui/components.rs`

**Interfaces:**
- Consumes: `raios_surface_tui::app::editor::simple_diff` (existing, `app/editor.rs:4`), `raios_runtime::filebrowser::save_constitution_file` (Task 2), `App.editor.active_file`/`App.save_file` (existing, `actions.rs:57-71`)
- Produces: `App::jump_to_constitution_raw_edit(&mut self)` — opens `AppState::FileEdit` on the active tab's file, cursor at the selected outline row's line
- Produces: `App::is_constitution_path(&self, path: &Path) -> bool`
- Produces: `App::request_constitution_save(&mut self, path: PathBuf, new_content: String)` — computes the diff, populates `self.constitution.pending_save`, does *not* write yet
- Produces: `App::confirm_constitution_save(&mut self)` / `App::cancel_constitution_save(&mut self)`
- Modifies: `App::save_file()` to route through `request_constitution_save` when the active file is a constitution path, instead of writing directly

- [ ] **Step 1: Add the raw-edit jump and path-check helpers**

In `crates/raios-surface-tui/src/app/events/actions.rs`, add (near `open_file_edit`, after line 36):

```rust
    pub(crate) fn is_constitution_path(&self, path: &Path) -> bool {
        self.constitution.tabs.iter().any(|t| t.path() == path)
    }

    pub(crate) fn jump_to_constitution_raw_edit(&mut self) {
        let Some(target) = self.constitution.tabs.get(self.constitution.active_tab).cloned() else { return };
        let line = self.outline_cursor_line();
        let entry = FileEntry::new(target.label(), target.path().to_path_buf());
        self.open_file_edit(entry);
        if let Some(l) = line {
            self.editor.editor.cursor_row = l.min(self.editor.editor.lines.len().saturating_sub(1));
            self.editor.editor.cursor_col = 0;
        }
    }

    fn outline_cursor_line(&self) -> Option<usize> {
        use raios_surface_tui::app::state::OutlineRow;
        let row = self.constitution.rows.get(self.constitution.outline_cursor)?;
        let sections = &self.constitution.sections;
        Some(match *row {
            OutlineRow::Section { idx } => sections[idx].line_start,
            OutlineRow::Child { idx, child_idx } => sections[idx].children[child_idx].line_start,
            OutlineRow::Item { idx, child_idx, .. } => match child_idx {
                Some(c) => sections[idx].children[c].line_start,
                None => sections[idx].line_start,
            },
        })
    }
```

Add `use raios_runtime::filebrowser::FileEntry` is already imported at the top of this file (line 7) — reuse it.

- [ ] **Step 2: Add save-confirmation request/confirm/cancel**

In the same file, add:

```rust
    pub(crate) fn request_constitution_save(&mut self, path: PathBuf, new_content: String) {
        let old_content = load_file_content(&path);
        let diff_lines = raios_surface_tui::app::editor::simple_diff(&old_content, &new_content);
        let added = diff_lines.iter().filter(|l| l.starts_with('+')).count();
        let removed = diff_lines.iter().filter(|l| l.starts_with('-')).count();
        self.constitution.pending_save = Some(raios_surface_tui::app::state::PendingConstitutionSave {
            path,
            new_content,
            diff_lines,
            added,
            removed,
        });
    }

    pub(crate) fn confirm_constitution_save(&mut self) {
        if let Some(pending) = self.constitution.pending_save.take() {
            match raios_runtime::filebrowser::save_constitution_file(&pending.path, &pending.new_content) {
                Ok(()) => {
                    self.editor.save_msg = Some("Saved!".into());
                    self.state = AppState::FileView;
                    let idx = self.constitution.active_tab;
                    self.load_constitution_tab(idx);
                }
                Err(e) => {
                    self.editor.save_msg = Some(format!("Error: {}", e));
                }
            }
        }
    }

    pub(crate) fn cancel_constitution_save(&mut self) {
        self.constitution.pending_save = None;
    }
```

- [ ] **Step 3: Route `save_file()` through the confirmation flow for constitution paths**

Replace the existing `save_file` method body (`actions.rs:57-71`):

```rust
    pub(crate) fn save_file(&mut self) {
        if let Some(ref file) = self.editor.active_file.clone() {
            let content = self.editor.editor.content();
            if self.is_constitution_path(&file.path) {
                self.request_constitution_save(file.path.clone(), content);
                return;
            }
            match save_file_content(&file.path, &content) {
                Ok(()) => {
                    self.editor.lines = content.lines().map(str::to_owned).collect();
                    self.editor.save_msg = Some("Saved!".into());
                    self.state = AppState::FileView;
                }
                Err(e) => {
                    self.editor.save_msg = Some(format!("Error: {}", e));
                }
            }
        }
    }
```

- [ ] **Step 4: Wire the `r`/`Enter` raw-edit-jump key and the modal dispatch**

In `crates/raios-surface-tui/src/app/events/keyboard/dashboard.rs`, inside the "Constitution panel keyboard" block added in Task 5, add (before the `// ── End Constitution ──` comment):

```rust
            KeyCode::Char('r') | KeyCode::Enter
                if self.ui.menu_cursor == 1
                    && self.ui.right_panel_focus
                    && !self.constitution.item_editing
                    && !self.constitution.creator.active
                    && !self.constitution.sections.is_empty() =>
            {
                self.jump_to_constitution_raw_edit();
            }
```

In `crates/raios-surface-tui/src/app/events/keyboard/mod.rs`, add a new priority check mirroring `handover_modal` (insert right after the `handover_modal` block, before the launcher-overlay block at line 40):

```rust
        // Constitution save-confirmation modal takes priority over all other input
        if self.constitution.pending_save.is_some() {
            match key.code {
                KeyCode::Enter => self.confirm_constitution_save(),
                KeyCode::Esc => self.cancel_constitution_save(),
                _ => {}
            }
            return Ok(());
        }
```

- [ ] **Step 5: Render the modal**

In `crates/raios-surface-tui/src/ui/components.rs`, add a new function near `render_handover_modal`:

```rust
pub fn render_constitution_save_modal(frame: &mut Frame, app: &App) {
    if let Some(ref pending) = app.constitution.pending_save {
        let area = center_rect(70, 60, frame.area());
        frame.render_widget(Clear, area);

        let block = Block::default()
            .title(" Save Constitution File? ")
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(AMBER))
            .style(Style::default().bg(Color::Rgb(8, 12, 16)));

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                format!(
                    " {} lines added, {} lines removed — {}",
                    pending.added,
                    pending.removed,
                    pending.path.display()
                ),
                Style::default().fg(AMBER).bold(),
            )]),
            Line::from(""),
        ];
        for diff_line in pending.diff_lines.iter().take(20) {
            let color = if diff_line.starts_with('+') {
                GREEN
            } else if diff_line.starts_with('-') {
                RED
            } else {
                DIM
            };
            lines.push(Line::from(Span::styled(diff_line.clone(), Style::default().fg(color))));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " [Enter] Save (backs up previous version)   [Esc] Cancel",
            Style::default().fg(MID),
        )));

        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(Paragraph::new(lines), inner);
    }
}
```

In `crates/raios-surface-tui/src/ui/mod.rs`, add to the overlay chain in `render()` (after the `handover_modal` check):

```rust
    if app.constitution.pending_save.is_some() {
        render_constitution_save_modal(frame, app);
    }
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo check -p raios-surface-tui 2>&1 | tail -60`
Expected: clean compile, 0 errors.

- [ ] **Step 7: Manual verification**

Launch the TUI, open Constitution, `→` to focus, `r` on a section to jump into raw edit at the right line, make an edit, `Ctrl+S` — confirm the diff modal appears with correct added/removed counts, `Enter` confirms and a `.bak.<timestamp>` file appears next to the edited file, `Esc` cancels and nothing is written.

- [ ] **Step 8: Commit**

```bash
git add crates/raios-surface-tui/src/app/events/actions.rs crates/raios-surface-tui/src/app/events/keyboard/dashboard.rs crates/raios-surface-tui/src/app/events/keyboard/mod.rs crates/raios-surface-tui/src/ui/mod.rs crates/raios-surface-tui/src/ui/components.rs
git commit -m "feat: raw-edit line jump and backup+diff save confirmation for constitution files"
```

---

### Task 7: Inline item edit / add / delete

**Files:**
- Modify: `crates/raios-surface-tui/src/app/events/actions.rs`
- Modify: `crates/raios-surface-tui/src/app/events/keyboard/dashboard.rs`

**Interfaces:**
- Consumes: `App::request_constitution_save`, `App::outline_cursor_line` (private, Task 6), `ConstitutionState.{item_editing, item_input}` (Task 3)
- Produces: `App::begin_item_edit(&mut self)`, `App::begin_item_insert(&mut self)`, `App::commit_item_edit(&mut self)`, `App::delete_item_at_cursor(&mut self)`

- [ ] **Step 1: Implement the item edit/insert/delete/commit actions**

Add to `crates/raios-surface-tui/src/app/events/actions.rs`:

```rust
    pub(crate) fn begin_item_edit(&mut self) {
        use raios_surface_tui::app::state::OutlineRow;
        let Some(&row) = self.constitution.rows.get(self.constitution.outline_cursor) else { return };
        if let OutlineRow::Item { idx, child_idx, item_idx } = row {
            let current = match child_idx {
                Some(c) => self.constitution.sections[idx].children[c].items[item_idx].clone(),
                None => self.constitution.sections[idx].items[item_idx].clone(),
            };
            self.constitution.item_input = current;
            self.constitution.item_editing = true;
        }
    }

    pub(crate) fn begin_item_insert(&mut self) {
        self.constitution.item_input = String::new();
        self.constitution.item_editing = true;
    }

    pub(crate) fn commit_item_edit(&mut self) {
        let Some(target) = self.constitution.tabs.get(self.constitution.active_tab).cloned() else {
            self.constitution.item_editing = false;
            return;
        };
        let Some(line) = self.outline_cursor_line() else {
            self.constitution.item_editing = false;
            return;
        };
        let content = load_file_content(target.path());
        let mut lines: Vec<String> = content.lines().map(str::to_owned).collect();
        let new_text = self.constitution.item_input.clone();
        if line < lines.len() {
            lines[line] = new_text;
        } else {
            lines.push(new_text);
        }
        let new_content = lines.join("\n") + "\n";
        self.constitution.item_editing = false;
        self.request_constitution_save(target.path().to_path_buf(), new_content);
    }

    pub(crate) fn delete_item_at_cursor(&mut self) {
        use raios_surface_tui::app::state::OutlineRow;
        let Some(&row) = self.constitution.rows.get(self.constitution.outline_cursor) else { return };
        if !matches!(row, OutlineRow::Item { .. }) {
            return;
        }
        let Some(target) = self.constitution.tabs.get(self.constitution.active_tab).cloned() else { return };
        let Some(line) = self.outline_cursor_line() else { return };
        let content = load_file_content(target.path());
        let mut lines: Vec<String> = content.lines().map(str::to_owned).collect();
        if line < lines.len() {
            lines.remove(line);
        }
        let new_content = lines.join("\n") + "\n";
        self.request_constitution_save(target.path().to_path_buf(), new_content);
    }
```

Note: `begin_item_insert` reuses `outline_cursor_line()`'s section/child line as the insertion point when committed — for a first pass this appends the new item text as a single new line at that position, which is correct for adding directly under the currently-selected header/item and matches the "n under a section" behavior from the design.

- [ ] **Step 2: Wire the `i`/`n`/`d` keys and item-input typing**

In `crates/raios-surface-tui/src/app/events/keyboard/dashboard.rs`, add inside the "Constitution panel keyboard" block (after the `r`/`Enter` arm from Task 6, before `// ── End Constitution ──`):

```rust
            KeyCode::Char('i')
                if self.ui.menu_cursor == 1
                    && self.ui.right_panel_focus
                    && !self.constitution.item_editing
                    && !self.constitution.creator.active =>
            {
                self.begin_item_edit();
            }
            KeyCode::Char('n')
                if self.ui.menu_cursor == 1
                    && self.ui.right_panel_focus
                    && !self.constitution.item_editing
                    && !self.constitution.creator.active =>
            {
                self.begin_item_insert();
            }
            KeyCode::Char('d')
                if self.ui.menu_cursor == 1
                    && self.ui.right_panel_focus
                    && !self.constitution.item_editing
                    && !self.constitution.creator.active =>
            {
                self.delete_item_at_cursor();
            }
            KeyCode::Enter if self.ui.menu_cursor == 1 && self.constitution.item_editing => {
                self.commit_item_edit();
            }
            KeyCode::Esc if self.ui.menu_cursor == 1 && self.constitution.item_editing => {
                self.constitution.item_editing = false;
                self.constitution.item_input.clear();
            }
            KeyCode::Char(c) if self.ui.menu_cursor == 1 && self.constitution.item_editing => {
                self.constitution.item_input.push(c);
            }
            KeyCode::Backspace if self.ui.menu_cursor == 1 && self.constitution.item_editing => {
                self.constitution.item_input.pop();
            }
```

These arms must be placed before the `r`/`Enter` raw-edit-jump arm from Task 6 is *not* required since that arm's guard already excludes `item_editing` — but this new `Enter if item_editing` arm must appear before it in source order since both match `KeyCode::Enter`. Place this whole block immediately after the Task 6 `r`/`Enter` arm to guarantee correct ordering (Rust match tries arms top-to-bottom; since the Task 6 arm's guard requires `!self.constitution.item_editing`, ordering between the two is actually safe either way — but keeping them adjacent keeps the code readable).

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p raios-surface-tui 2>&1 | tail -60`
Expected: clean compile, 0 errors.

- [ ] **Step 4: Manual verification**

Launch the TUI, focus Constitution, move to an item row, press `i`, edit the text, `Enter` — confirm the save-confirmation modal (Task 6) appears with the right diff, `Enter` again commits it and the outline reloads showing the new text. Repeat for `n` (new item appended) and `d` (item removed).

- [ ] **Step 5: Commit**

```bash
git add crates/raios-surface-tui/src/app/events/actions.rs crates/raios-surface-tui/src/app/events/keyboard/dashboard.rs
git commit -m "feat: inline item edit/insert/delete in Constitution outline"
```

---

### Task 8: Creator mode

**Files:**
- Modify: `crates/raios-surface-tui/src/app/events/actions.rs`
- Modify: `crates/raios-surface-tui/src/app/events/keyboard/dashboard.rs`

**Interfaces:**
- Consumes: `CreatorState`, `CreatorStep` (Task 3), `App::request_constitution_save` (Task 6)
- Produces: `App::open_creator(&mut self)`, `App::creator_choose_target(&mut self, is_global: bool)`, `App::creator_confirm_save(&mut self)`, `App::close_creator(&mut self)`

- [ ] **Step 1: Implement creator actions**

Add to `crates/raios-surface-tui/src/app/events/actions.rs`:

```rust
    pub(crate) fn open_creator(&mut self) {
        self.constitution.creator = raios_surface_tui::app::state::CreatorState {
            active: true,
            ..Default::default()
        };
    }

    pub(crate) fn close_creator(&mut self) {
        self.constitution.creator = raios_surface_tui::app::state::CreatorState::default();
    }

    pub(crate) fn creator_choose_target(&mut self, is_global: bool) {
        use raios_surface_tui::app::state::CreatorStep;
        self.constitution.creator.target_is_global = is_global;
        self.constitution.creator.step = CreatorStep::Notes;
    }

    pub(crate) fn creator_confirm_save(&mut self) {
        let creator = self.constitution.creator.clone();
        if creator.target_is_global {
            let notes = format!("## Project-Specific Rules\n{}\n", creator.notes_input);
            self.request_constitution_save(self.config.master_md_path.clone(), notes);
        } else if let Some(ref proj) = self.projects.active.clone() {
            let path = proj.local_path.join("CLAUDE.md");
            let content = format!(
                "@/home/alaz/AGENT_CONSTITUTION.md\n\n## Project-Specific Rules\n{}\n",
                creator.notes_input
            );
            self.request_constitution_save(path, content);
        }
        self.close_creator();
    }
```

`CreatorState` needs `Clone` (already derived in Task 3) for the `.clone()` call above.

- [ ] **Step 2: Wire the `c` entry key and the creator's own step keys**

In `crates/raios-surface-tui/src/app/events/keyboard/dashboard.rs`, add inside the "Constitution panel keyboard" block, after the `d` (delete) arm from Task 7:

```rust
            KeyCode::Char('c')
                if self.ui.menu_cursor == 1
                    && self.ui.right_panel_focus
                    && !self.constitution.item_editing
                    && !self.constitution.creator.active =>
            {
                self.open_creator();
            }
            KeyCode::Esc if self.ui.menu_cursor == 1 && self.constitution.creator.active => {
                self.close_creator();
            }
            KeyCode::Char('p')
                if self.ui.menu_cursor == 1
                    && self.constitution.creator.active
                    && self.constitution.creator.step
                        == raios_surface_tui::app::state::CreatorStep::ChooseTarget =>
            {
                self.creator_choose_target(false);
            }
            KeyCode::Char('g')
                if self.ui.menu_cursor == 1
                    && self.constitution.creator.active
                    && self.constitution.creator.step
                        == raios_surface_tui::app::state::CreatorStep::ChooseTarget =>
            {
                self.creator_choose_target(true);
            }
            KeyCode::Enter
                if self.ui.menu_cursor == 1
                    && self.constitution.creator.active
                    && self.constitution.creator.step
                        == raios_surface_tui::app::state::CreatorStep::Notes =>
            {
                self.constitution.creator.step = raios_surface_tui::app::state::CreatorStep::Preview;
            }
            KeyCode::Char(ch)
                if self.ui.menu_cursor == 1
                    && self.constitution.creator.active
                    && self.constitution.creator.step
                        == raios_surface_tui::app::state::CreatorStep::Notes =>
            {
                self.constitution.creator.notes_input.push(ch);
            }
            KeyCode::Backspace
                if self.ui.menu_cursor == 1
                    && self.constitution.creator.active
                    && self.constitution.creator.step
                        == raios_surface_tui::app::state::CreatorStep::Notes =>
            {
                self.constitution.creator.notes_input.pop();
            }
            KeyCode::Enter
                if self.ui.menu_cursor == 1
                    && self.constitution.creator.active
                    && self.constitution.creator.step
                        == raios_surface_tui::app::state::CreatorStep::Preview =>
            {
                self.creator_confirm_save();
            }
```

These arms must be placed before the generic Task-5/6/7 `Enter`/`Esc` arms guarded only by `menu_cursor == 1` (without the `creator.active` check) so creator-mode input doesn't leak into outline-mode handling — since all of these carry the additional `self.constitution.creator.active` guard, and the outline arms carry `!self.constitution.creator.active`, the two sets are mutually exclusive regardless of order, but keep them grouped together for readability.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p raios-surface-tui 2>&1 | tail -60`
Expected: clean compile, 0 errors.

- [ ] **Step 4: Manual verification**

Launch the TUI, focus Constitution, `c` to open creator, `p` for project-specific (with a project open), type some notes, `Enter` to preview, confirm the preview shows the `@include` line plus your notes, `Enter` again to save through the existing diff-confirm modal, confirm `<project>/CLAUDE.md` is created with the expected content. Then repeat choosing `g` (global) and confirm the amber overwrite warning appears in the preview before saving.

- [ ] **Step 5: Commit**

```bash
git add crates/raios-surface-tui/src/app/events/actions.rs crates/raios-surface-tui/src/app/events/keyboard/dashboard.rs
git commit -m "feat: add Constitution creator mode for new project/global files"
```

---

### Task 9: Full regression pass and cleanup

**Files:**
- No new files — verification and cleanup only.

- [ ] **Step 1: Run the full test suite**

Run: `cargo test --workspace 2>&1 | tail -60`
Expected: all tests green, including the new ones from Tasks 1-2.

- [ ] **Step 2: Run clippy across the workspace**

Run: `cargo clippy --workspace -- -D warnings 2>&1 | tail -80`
Expected: 0 warnings. Fix anything the new code introduced (e.g. needless clones, redundant `.clone()` on `Copy` types) without changing behavior.

- [ ] **Step 3: Grep for any leftover references to deleted symbols**

Run: `grep -rn "system_rules\|RuleCategory\|render_rules" --include="*.rs" crates/ | grep -v target`
Expected: no output. If anything remains, remove it.

- [ ] **Step 4: Manual end-to-end walkthrough**

Launch the real TUI binary in a detached `screen`/`tmux` session (this repo's own convention per its 2026-06-25 change log entry for verifying the Inbox panel the same way) and walk the full feature:
1. Navigate to Constitution (menu index 1), confirm the outline renders from the real `AGENT_CONSTITUTION.md`.
2. Focus it, jump into raw edit on a section, make a trivial edit, save, confirm a `.bak.<timestamp>` appears and the outline reflects the change on return.
3. Edit/add/delete an item inline without leaving the outline.
4. Open a project with an existing `CLAUDE.md`, confirm tab `[2]` appears and shows the include-only notice.
5. Run creator mode for a project without a `CLAUDE.md` yet, confirm the file is created correctly.
6. Confirm `Esc` at the diff-confirm modal correctly aborts without writing anything.

- [ ] **Step 5: Regenerate SIGMAP.md per this repo's own commit convention**

Run: `sigmap` (from the repo root) — this repo's `AGENT_CONSTITUTION.md` §6/§7 requires `SIGMAP.md` to be regenerated before every commit that changes architecture. If `sigmap` isn't installed in the environment running this plan, note that explicitly instead of skipping silently.

- [ ] **Step 6: Final commit**

```bash
git add -A
git commit -m "chore: regenerate SIGMAP.md after Constitution editor feature"
```

---

## Self-Review Notes

- **Spec coverage:** Outline mode (Task 4/5), raw edit mode reusing existing `Editor`/`FileEdit` (Task 6), Creator mode with project + global-from-scratch paths (Task 8), multi-file tabs with include-only detection (Task 4), backup+diff-confirm save safety (Task 2/6), inline item edit/add/delete (Task 7), menu-slot replacement of the old Rules panel (Task 4) — all covered.
- **Placeholder scan:** no TBD/TODO; every step has complete code.
- **Type consistency:** `ConstitutionTarget`, `OutlineRow`, `ConstitutionState`, `PendingConstitutionSave`, `CreatorState`/`CreatorStep` are defined once in Task 3 and referenced identically (same field names/types) in Tasks 4-8; `save_constitution_file` signature from Task 2 matches its call sites in Task 6.
- **Scope:** single cohesive feature, no unrelated subsystem touched (CLI `raios rules`, policy files, and the generic file browser for non-constitution files are explicitly left untouched per Global Constraints).
