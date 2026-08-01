# Obsidian Vault Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give raios a `raios obsidian-sync` command (plus an automatic hook
from `raios new`) that regenerates a `~/Obsidian` vault of per-project notes,
category MOCs, and a root index from the projects raios already tracks.

**Architecture:** A new `raios-runtime::obsidian` module owns rendering
(pure functions → strings) and writing (I/O) for project notes, category
MOCs, and the root atlas. `sync_vault_projects` operates on an explicit
project list (hermetic, testable); `sync_vault` is a thin DB-backed wrapper
around it. Both the new CLI subcommand and `new_project::create`'s vault
step call into this same engine — no duplicated note-writing logic.

**Tech Stack:** Rust (existing `raios-core`/`raios-runtime`/`raios-surface-cli`
crates), `chrono` for timestamps, `clap` for CLI args — all already
workspace dependencies. No new dependencies.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-31-obsidian-vault-sync-design.md`
  (as amended 2026-07-31) is the source of truth for behavior; this plan
  implements it exactly.
- Vault default path: `~/Obsidian` (via `dirs::home_dir()`).
- Category folders match `EntityProject.category` values verbatim: `ai`,
  `web`, `embedded`, `tools`, `core`, `audio`, `mobile`, `archives`.
- Category MOC filename: `<category>-MOC.md` (never bare `_MOC.md` —
  vault-wide Obsidian wikilink uniqueness).
- `status` frontmatter tag uses raios's real DB vocabulary only: `active`,
  `archived`, `beklemede`, `waiting`. No "production" tier.
- Every sync run **fully overwrites** project notes/MOCs/atlas (regenerate,
  not merge) — this is intentional per spec, not a bug to fix.
- No new dependencies; reuse `chrono`, `dirs`, `serde_json`, `clap` already
  present in the relevant `Cargo.toml`s.
- CLI-only in this phase — no MCP tool, no TUI screen.
- Tests must not touch the developer's real `~/.config/raios/workspace.db`
  or real `~/Obsidian` — use `tempfile::tempdir()` throughout, per existing
  convention in `crates/raios-runtime/tests/new_project_integration.rs`.

---

## File Structure

- **Create** `crates/raios-runtime/src/obsidian.rs` — rendering + sync
  engine (`ObsidianSyncReport`, `default_vault_path`, `render_project_note`,
  `render_moc`, `render_atlas`, `sync_vault_projects`, `sync_vault`).
- **Modify** `crates/raios-runtime/src/lib.rs` — register `pub mod obsidian;`.
- **Modify** `crates/raios-runtime/src/new_project.rs` — add `vault_path`
  field to `NewProjectConfig`; replace `update_vault_atlas` (deleted) with
  a call into `obsidian::sync_vault_projects`.
- **Modify** `crates/raios-runtime/tests/new_project_integration.rs` —
  update existing test's struct literal for the new field; add a test
  covering the vault-sync step.
- **Modify** `crates/raios-surface-cli/src/cli/args.rs` — add
  `Commands::ObsidianSync { vault: Option<String>, dry_run: bool }`.
- **Create** `crates/raios-surface-cli/src/cli/obsidian_sync.rs` —
  `cmd_obsidian_sync` (CLI-facing wrapper: path resolution, text/JSON
  output).
- **Modify** `crates/raios-surface-cli/src/cli/mod.rs` — `mod obsidian_sync;`
  + dispatch arm.
- **Modify** `crates/raios-surface-cli/src/cli/new.rs` — pass
  `vault_path: None` in the existing `NewProjectConfig` construction (uses
  the default vault path; keeps `cmd_new`'s public CLI surface unchanged).

---

### Task 1: Rendering functions (pure, unit-tested)

**Files:**
- Create: `crates/raios-runtime/src/obsidian.rs`
- Test: same file, `#[cfg(test)] mod tests` at the bottom

**Interfaces:**
- Consumes: `raios_core::entities::EntityProject` (fields used: `name`,
  `category`, `local_path: PathBuf`, `github: Option<String>`,
  `status: String`, `last_commit: Option<String>`, `version: Option<String>`).
- Produces (used by Task 2 and by the CLI in Task 4):
  - `fn render_project_note(project: &EntityProject, memory_content: Option<&str>, synced_at: &str) -> String`
  - `fn render_moc(category: &str, entries: &[(String, String)]) -> String`
    (`entries` are `(project_name, status)` pairs)
  - `fn render_atlas(projects: &[EntityProject]) -> String`
  - `fn yaml_quote(s: &str) -> String`

- [ ] **Step 1: Write the failing tests**

Create `crates/raios-runtime/src/obsidian.rs` with just the test module and
`use` of not-yet-existing functions (so it fails to compile, which is our
"red" for these pure functions):

```rust
//! Renders and writes an Obsidian-compatible vault of raios project notes.
//!
//! See docs/superpowers/specs/2026-07-31-obsidian-vault-sync-design.md.

use raios_core::entities::EntityProject;
use std::path::PathBuf;

fn yaml_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn render_project_note(
    project: &EntityProject,
    memory_content: Option<&str>,
    synced_at: &str,
) -> String {
    todo!()
}

fn render_moc(category: &str, entries: &[(String, String)]) -> String {
    todo!()
}

fn render_atlas(projects: &[EntityProject]) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_project() -> EntityProject {
        EntityProject {
            name: "sample-project".to_string(),
            category: "ai".to_string(),
            local_path: PathBuf::from("/home/alaz/dev/ai/sample-project"),
            github: Some("https://github.com/alazndy/sample-project".to_string()),
            status: "active".to_string(),
            stars: None,
            last_commit: Some("2026-07-30".to_string()),
            version: Some("1.2.0".to_string()),
            version_nickname: None,
        }
    }

    #[test]
    fn render_project_note_includes_frontmatter_and_memory_content() {
        let project = sample_project();
        let note = render_project_note(
            &project,
            Some("# Project Memory: sample-project\n\nsome content"),
            "2026-07-31T22:00:00",
        );

        assert!(note.starts_with("---\n"));
        assert!(note.contains("tags: [proje, \"kategori/ai\", \"durum/active\"]"));
        assert!(note.contains("category: \"ai\""));
        assert!(note.contains("status: \"active\""));
        assert!(note.contains("local_path: \"/home/alaz/dev/ai/sample-project\""));
        assert!(note.contains("github: \"https://github.com/alazndy/sample-project\""));
        assert!(note.contains("last_commit: \"2026-07-30\""));
        assert!(note.contains("version: \"1.2.0\""));
        assert!(note.contains("synced: \"2026-07-31T22:00:00\""));
        assert!(note.contains("# sample-project"));
        assert!(note.contains("[[ai-MOC|ai projeleri]]"));
        assert!(note.contains("some content"));
    }

    #[test]
    fn render_project_note_handles_missing_memory_md() {
        let project = sample_project();
        let note = render_project_note(&project, None, "2026-07-31T22:00:00");
        assert!(note.contains("_memory.md not found_"));
    }

    #[test]
    fn render_project_note_handles_none_fields() {
        let mut project = sample_project();
        project.github = None;
        project.last_commit = None;
        project.version = None;
        let note = render_project_note(&project, Some("x"), "2026-07-31T22:00:00");
        assert!(note.contains("github: \"\""));
        assert!(note.contains("last_commit: \"\""));
        assert!(note.contains("version: \"\""));
    }

    #[test]
    fn render_moc_lists_entries_sorted_by_name() {
        let entries = vec![
            ("zeta".to_string(), "active".to_string()),
            ("alpha".to_string(), "beklemede".to_string()),
        ];
        let moc = render_moc("ai", &entries);
        assert!(moc.contains("tags: [moc, \"kategori/ai\"]"));
        assert!(moc.contains("# ai projeleri"));
        let alpha_pos = moc.find("[[alpha]] — beklemede").expect("alpha entry");
        let zeta_pos = moc.find("[[zeta]] — active").expect("zeta entry");
        assert!(alpha_pos < zeta_pos, "entries must be sorted by name");
    }

    #[test]
    fn render_atlas_counts_by_category_and_status() {
        let mut p2 = sample_project();
        p2.name = "other-project".to_string();
        p2.category = "web".to_string();
        p2.status = "beklemede".to_string();
        let projects = vec![sample_project(), p2];

        let atlas = render_atlas(&projects);
        assert!(atlas.contains("Toplam: 2 proje"));
        assert!(atlas.contains("[[ai-MOC|ai]] — 1 proje"));
        assert!(atlas.contains("[[web-MOC|web]] — 1 proje"));
        assert!(atlas.contains("active: 1"));
        assert!(atlas.contains("beklemede: 1"));
    }

    #[test]
    fn yaml_quote_escapes_backslash_and_quote() {
        assert_eq!(yaml_quote(r#"a"b\c"#), r#""a\"b\\c""#);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p raios-runtime obsidian:: --lib`
Expected: compile failure or panics from the `todo!()` bodies (the
`yaml_quote` test passes since that one's already implemented — that's
fine, the others must fail).

- [ ] **Step 3: Implement the rendering functions**

Replace the three `todo!()` function bodies:

```rust
fn render_project_note(
    project: &EntityProject,
    memory_content: Option<&str>,
    synced_at: &str,
) -> String {
    let github = project.github.as_deref().unwrap_or("");
    let last_commit = project.last_commit.as_deref().unwrap_or("");
    let version = project.version.as_deref().unwrap_or("");
    let body = memory_content.unwrap_or("_memory.md not found_");

    format!(
        "---\n\
         tags: [proje, \"kategori/{category}\", \"durum/{status}\"]\n\
         category: {category_q}\n\
         status: {status_q}\n\
         local_path: {local_path_q}\n\
         github: {github_q}\n\
         last_commit: {last_commit_q}\n\
         version: {version_q}\n\
         synced: {synced_q}\n\
         ---\n\
         # {name}\n\
         \n\
         ← [[{category}-MOC|{category} projeleri]]\n\
         \n\
         {body}\n",
        category = project.category,
        status = project.status,
        category_q = yaml_quote(&project.category),
        status_q = yaml_quote(&project.status),
        local_path_q = yaml_quote(&project.local_path.to_string_lossy()),
        github_q = yaml_quote(github),
        last_commit_q = yaml_quote(last_commit),
        version_q = yaml_quote(version),
        synced_q = yaml_quote(synced_at),
        name = project.name,
        body = body,
    )
}

fn render_moc(category: &str, entries: &[(String, String)]) -> String {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = format!(
        "---\ntags: [moc, \"kategori/{category}\"]\n---\n# {category} projeleri\n\n"
    );
    for (name, status) in &sorted {
        out.push_str(&format!("- [[{name}]] — {status}\n"));
    }
    out
}

fn render_atlas(projects: &[EntityProject]) -> String {
    use std::collections::BTreeMap;

    let mut by_category: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_status: BTreeMap<&str, usize> = BTreeMap::new();
    for p in projects {
        *by_category.entry(p.category.as_str()).or_insert(0) += 1;
        *by_status.entry(p.status.as_str()).or_insert(0) += 1;
    }

    let mut out = format!("# Proje Atlası\n\nToplam: {} proje\n\n## Kategoriler\n", projects.len());
    for (category, count) in &by_category {
        out.push_str(&format!("- [[{category}-MOC|{category}]] — {count} proje\n"));
    }
    out.push_str("\n## Durum\n");
    for (status, count) in &by_status {
        out.push_str(&format!("- {status}: {count}\n"));
    }
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p raios-runtime obsidian:: --lib`
Expected: all 6 tests PASS.

- [ ] **Step 5: Commit**

```bash
cd /home/alaz/dev/core/R-AI-OS
git add crates/raios-runtime/src/obsidian.rs
git commit -m "feat(obsidian): add pure rendering functions for vault notes/MOC/atlas"
```

---

### Task 2: I/O orchestration — `sync_vault_projects` / `sync_vault`

**Files:**
- Modify: `crates/raios-runtime/src/obsidian.rs`
- Modify: `crates/raios-runtime/src/lib.rs:19` (insert `pub mod obsidian;`
  after `pub mod new_project;`)
- Test: same file, extend `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `render_project_note`, `render_moc`, `render_atlas` from Task 1;
  `raios_core::entities::{EntityProject, load_entities}`.
- Produces (used by Task 3 and Task 4):
  - `pub struct ObsidianSyncReport { pub written: usize, pub errors: Vec<String>, pub paths: Vec<PathBuf> }`
  - `pub fn default_vault_path() -> PathBuf`
  - `pub fn sync_vault_projects(vault: &Path, projects: &[EntityProject], dry_run: bool) -> ObsidianSyncReport`
  - `pub fn sync_vault(dev_ops: &Path, vault: &Path, dry_run: bool) -> ObsidianSyncReport`

- [ ] **Step 1: Write the failing test**

Add to `crates/raios-runtime/src/obsidian.rs`, inside `mod tests`:

```rust
    #[test]
    fn sync_vault_projects_writes_notes_mocs_and_atlas() {
        let dev_ops = tempfile::tempdir().expect("dev_ops tempdir");
        let vault = tempfile::tempdir().expect("vault tempdir");

        let project_dir = dev_ops.path().join("ai").join("sample-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(project_dir.join("memory.md"), "hello from memory.md").unwrap();

        let mut project = sample_project();
        project.local_path = project_dir;

        let report = sync_vault_projects(vault.path(), &[project], false);

        assert_eq!(report.written, 1);
        assert!(report.errors.is_empty());

        let note_path = vault
            .path()
            .join("Projeler")
            .join("ai")
            .join("sample-project.md");
        let note = std::fs::read_to_string(&note_path).expect("note written");
        assert!(note.contains("hello from memory.md"));

        let moc_path = vault.path().join("Projeler").join("ai").join("ai-MOC.md");
        assert!(moc_path.exists());

        let atlas_path = vault.path().join("Proje Atlası.md");
        let atlas = std::fs::read_to_string(&atlas_path).expect("atlas written");
        assert!(atlas.contains("Toplam: 1 proje"));
    }

    #[test]
    fn sync_vault_projects_dry_run_writes_nothing() {
        let vault = tempfile::tempdir().expect("vault tempdir");
        // local_path doesn't exist on disk — fine, dry-run never reads memory.md
        let project = sample_project();

        let report = sync_vault_projects(vault.path(), &[project], true);

        assert_eq!(report.written, 1);
        assert!(!vault.path().join("Projeler").exists());
    }

    #[test]
    fn sync_vault_projects_missing_memory_md_is_not_an_error() {
        let vault = tempfile::tempdir().expect("vault tempdir");
        let mut project = sample_project();
        project.local_path = PathBuf::from("/nonexistent/path/for/this/test");

        let report = sync_vault_projects(vault.path(), &[project], false);

        assert_eq!(report.written, 1);
        assert!(report.errors.is_empty());
        let note = std::fs::read_to_string(
            vault.path().join("Projeler").join("ai").join("sample-project.md"),
        )
        .unwrap();
        assert!(note.contains("_memory.md not found_"));
    }
```

Also add `tempfile = "3"` check — it's already a dev-dependency in this
crate's `Cargo.toml` (confirmed present), no change needed there.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p raios-runtime obsidian:: --lib`
Expected: FAIL — `sync_vault_projects`/`ObsidianSyncReport` not found.

- [ ] **Step 3: Implement the orchestration layer**

Add to `crates/raios-runtime/src/obsidian.rs` (top-level, alongside the
render functions — update the `use` block too):

```rust
use raios_core::entities::{load_entities, EntityProject};
use std::path::{Path, PathBuf};

pub struct ObsidianSyncReport {
    pub written: usize,
    pub errors: Vec<String>,
    pub paths: Vec<PathBuf>,
}

pub fn default_vault_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Obsidian")
}

pub fn sync_vault_projects(
    vault: &Path,
    projects: &[EntityProject],
    dry_run: bool,
) -> ObsidianSyncReport {
    let synced_at = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    let mut report = ObsidianSyncReport {
        written: 0,
        errors: Vec::new(),
        paths: Vec::new(),
    };

    let projeler_dir = vault.join("Projeler");
    let mut by_category: std::collections::BTreeMap<&str, Vec<(String, String)>> =
        std::collections::BTreeMap::new();

    for project in projects {
        let category_dir = projeler_dir.join(&project.category);
        let note_path = category_dir.join(format!("{}.md", project.name));

        if !dry_run {
            let memory_path = project.local_path.join("memory.md");
            let memory_content = std::fs::read_to_string(&memory_path).ok();
            let note = render_project_note(project, memory_content.as_deref(), &synced_at);

            if let Err(e) = std::fs::create_dir_all(&category_dir) {
                report
                    .errors
                    .push(format!("{}: mkdir failed: {}", project.name, e));
                continue;
            }
            if let Err(e) = std::fs::write(&note_path, &note) {
                report
                    .errors
                    .push(format!("{}: write failed: {}", project.name, e));
                continue;
            }
        }

        report.written += 1;
        report.paths.push(note_path);
        by_category
            .entry(project.category.as_str())
            .or_default()
            .push((project.name.clone(), project.status.clone()));
    }

    if !dry_run {
        for (category, entries) in &by_category {
            let moc = render_moc(category, entries);
            let moc_path = projeler_dir.join(category).join(format!("{category}-MOC.md"));
            if let Err(e) = std::fs::write(&moc_path, moc) {
                report
                    .errors
                    .push(format!("{category}: MOC write failed: {e}"));
            }
        }

        let atlas = render_atlas(projects);
        if let Err(e) = std::fs::create_dir_all(vault) {
            report.errors.push(format!("vault mkdir failed: {e}"));
        } else if let Err(e) = std::fs::write(vault.join("Proje Atlası.md"), atlas) {
            report.errors.push(format!("atlas write failed: {e}"));
        }
    }

    report
}

pub fn sync_vault(dev_ops: &Path, vault: &Path, dry_run: bool) -> ObsidianSyncReport {
    let projects = load_entities(dev_ops);
    sync_vault_projects(vault, &projects, dry_run)
}
```

Remove the now-redundant local `use raios_core::entities::EntityProject;`
and `use std::path::PathBuf;` lines from Task 1 (they're superseded by the
combined `use` block above — keep only one `use` block per import).

- [ ] **Step 4: Register the module**

In `crates/raios-runtime/src/lib.rs`, after line 19 (`pub mod new_project;`),
add:

```rust
pub mod obsidian;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p raios-runtime obsidian:: --lib`
Expected: all 9 tests PASS (6 from Task 1 + 3 new).

- [ ] **Step 6: Commit**

```bash
cd /home/alaz/dev/core/R-AI-OS
git add crates/raios-runtime/src/obsidian.rs crates/raios-runtime/src/lib.rs
git commit -m "feat(obsidian): add sync_vault_projects/sync_vault I/O engine"
```

---

### Task 3: Wire `raios new` to the sync engine, delete dead `update_vault_atlas`

**Files:**
- Modify: `crates/raios-runtime/src/new_project.rs`
- Modify: `crates/raios-surface-cli/src/cli/new.rs`
- Modify: `crates/raios-runtime/tests/new_project_integration.rs`

**Interfaces:**
- Consumes: `raios_runtime::obsidian::{sync_vault_projects, default_vault_path}`
  (from Task 2); `raios_core::entities::load_entities` (already imported in
  this file).
- Produces: `NewProjectConfig` gains a `vault_path: Option<&'a Path>` field
  — any other future caller of `NewProjectConfig` must supply it.

- [ ] **Step 1: Write the failing test**

In `crates/raios-runtime/tests/new_project_integration.rs`, update the
existing struct literal (it will fail to compile once the field is added
in Step 3, which is our red signal for this task) and add a new test:

```rust
use raios_runtime::new_project::{create, NewProjectConfig};

#[test]
fn create_scaffolds_a_project_without_fabricating_a_github_url() {
    let dev_ops = tempfile::tempdir().expect("tempdir");

    let cfg = NewProjectConfig {
        name: "sample-project",
        category: "tools",
        dev_ops: dev_ops.path(),
        github: false,
        no_vault: true,
        vault_path: None,
    };

    // ... (rest of existing test body unchanged)
```

Add a new test in the same file:

```rust
#[test]
fn create_writes_a_vault_note_when_vault_is_enabled() {
    let dev_ops = tempfile::tempdir().expect("dev_ops tempdir");
    let vault = tempfile::tempdir().expect("vault tempdir");

    let cfg = NewProjectConfig {
        name: "vaulted-project",
        category: "tools",
        dev_ops: dev_ops.path(),
        github: false,
        no_vault: false,
        vault_path: Some(vault.path()),
    };

    let result = create(&cfg);

    let vault_ok = result
        .steps
        .iter()
        .find(|(desc, _)| desc == "Update Obsidian Vault")
        .map(|(_, ok)| *ok)
        .unwrap_or(false);
    assert!(vault_ok, "vault step should succeed: {:?}", result.steps);

    let note_path = vault
        .path()
        .join("Projeler")
        .join("tools")
        .join("vaulted-project.md");
    assert!(note_path.exists(), "expected vault note at {note_path:?}");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p raios-runtime --test new_project_integration`
Expected: compile error — `vault_path` field doesn't exist on
`NewProjectConfig` yet.

- [ ] **Step 3: Update `NewProjectConfig` and the vault step**

In `crates/raios-runtime/src/new_project.rs`:

Change the struct definition:

```rust
pub struct NewProjectConfig<'a> {
    pub name: &'a str,
    pub category: &'a str,
    pub dev_ops: &'a Path,
    pub github: bool,
    pub no_vault: bool,
    pub vault_path: Option<&'a Path>,
}
```

Replace step 9 (the `if !cfg.no_vault { ... }` block, currently calling
`update_vault_atlas`):

```rust
    // 9. Sync Obsidian vault (unless --no-vault)
    if !cfg.no_vault {
        let owned_default = crate::obsidian::default_vault_path();
        let vault = cfg.vault_path.unwrap_or(owned_default.as_path());
        let projects = raios_core::entities::load_entities(cfg.dev_ops);
        let report = crate::obsidian::sync_vault_projects(vault, &projects, false);
        steps.push((
            "Update Obsidian Vault".into(),
            report.errors.is_empty(),
        ));
    }
```

Delete the entire `update_vault_atlas` function (the block starting
`fn update_vault_atlas(` through its closing `}`, roughly lines 251-298).

In `crates/raios-surface-cli/src/cli/new.rs`, update the `NewProjectConfig`
construction in `cmd_new` to add the new field:

```rust
    let cfg = raios_runtime::new_project::NewProjectConfig {
        name,
        category: effective_category,
        dev_ops,
        github,
        no_vault,
        vault_path: None,
    };
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p raios-runtime --test new_project_integration`
Expected: both tests PASS.

Then run the full runtime test suite to catch any other `NewProjectConfig`
construction sites this plan missed:

Run: `cargo build -p raios-runtime -p raios-surface-cli 2>&1 | grep -i "missing field\|vault_path"`
Expected: no output (clean build). If any other call site shows up, add
`vault_path: None` there too before proceeding.

- [ ] **Step 5: Commit**

```bash
cd /home/alaz/dev/core/R-AI-OS
git add crates/raios-runtime/src/new_project.rs \
        crates/raios-surface-cli/src/cli/new.rs \
        crates/raios-runtime/tests/new_project_integration.rs
git commit -m "feat(new-project): sync Obsidian vault on project creation, drop dead update_vault_atlas"
```

---

### Task 4: `raios obsidian-sync` CLI command

**Files:**
- Modify: `crates/raios-surface-cli/src/cli/args.rs`
- Create: `crates/raios-surface-cli/src/cli/obsidian_sync.rs`
- Modify: `crates/raios-surface-cli/src/cli/mod.rs`

**Interfaces:**
- Consumes: `raios_runtime::obsidian::{sync_vault, default_vault_path, ObsidianSyncReport}`.
- Produces: the `raios obsidian-sync` CLI command (no other task depends on
  this one — it's the final, user-facing entry point).

- [ ] **Step 1: Add the CLI arg**

In `crates/raios-surface-cli/src/cli/args.rs`, add a variant to
`Commands` (place it near `Stats`, e.g. right after the `Stats,` line at
what is currently line 109):

```rust
    /// Sync the Obsidian vault from current raios project data
    ObsidianSync {
        /// Vault path (default: ~/Obsidian)
        #[arg(long)]
        vault: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
```

- [ ] **Step 2: Write `cmd_obsidian_sync`**

Create `crates/raios-surface-cli/src/cli/obsidian_sync.rs`:

```rust
use std::path::{Path, PathBuf};

pub(super) fn cmd_obsidian_sync(
    vault: Option<String>,
    dry_run: bool,
    dev_ops: &Path,
    json: bool,
) {
    let vault_path: PathBuf = vault
        .map(PathBuf::from)
        .unwrap_or_else(raios_runtime::obsidian::default_vault_path);

    let report = raios_runtime::obsidian::sync_vault(dev_ops, &vault_path, dry_run);

    if json {
        let out = serde_json::json!({
            "vault": vault_path.display().to_string(),
            "dry_run": dry_run,
            "written": report.written,
            "errors": report.errors,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return;
    }

    let verb = if dry_run { "would write" } else { "wrote" };
    println!(
        "Obsidian vault sync — {} note(s) {} to {}",
        report.written,
        verb,
        vault_path.display()
    );
    if !report.errors.is_empty() {
        println!("\nErrors:");
        for e in &report.errors {
            println!("  ✗ {e}");
        }
    }
}
```

- [ ] **Step 3: Wire the module and dispatch**

In `crates/raios-surface-cli/src/cli/mod.rs`:

Add `mod obsidian_sync;` to the alphabetically-sorted `mod` list (between
`mod new;` and `mod policy;`).

Add the dispatch arm near the other `&cfg.dev_ops_path` commands (e.g.
right after the `Commands::Stats => ...` line):

```rust
        Commands::ObsidianSync { vault, dry_run } => {
            obsidian_sync::cmd_obsidian_sync(vault, dry_run, &cfg.dev_ops_path, cli.json)
        }
```

- [ ] **Step 4: Build and manually verify against real data**

Run: `cargo build -p raios-surface-cli --release`
Expected: clean build.

Run a dry run first (no filesystem writes):
`./target/release/raios obsidian-sync --dry-run`
Expected: prints `would write` with a count matching `raios projects | wc -l`.

Run for real:
`./target/release/raios obsidian-sync`
Expected: `~/Obsidian/Projeler/<category>/*.md` created for every project,
one `<category>-MOC.md` per populated category, and `~/Obsidian/Proje
Atlası.md` present.

Verify counts match:
`ls ~/Obsidian/Projeler/*/*.md | grep -v MOC | wc -l` should equal
`raios projects | wc -l`.

Open `~/Obsidian/` in Obsidian, confirm a project note's
`[[<category>-MOC|...]]` link resolves to that category's MOC, and the MOC's
`[[project-name]]` links resolve back to project notes.

- [ ] **Step 5: Commit**

```bash
cd /home/alaz/dev/core/R-AI-OS
git add crates/raios-surface-cli/src/cli/args.rs \
        crates/raios-surface-cli/src/cli/obsidian_sync.rs \
        crates/raios-surface-cli/src/cli/mod.rs
git commit -m "feat(cli): add raios obsidian-sync command"
```

---

## Self-Review Notes

- **Spec coverage:** Vault layout (Task 2), frontmatter schema (Task 1),
  `raios obsidian-sync` CLI with `--vault`/`--dry-run`/`--json` (Task 4),
  full-regeneration model (Task 2 always overwrites), missing-`memory.md`
  handling (Task 1 + Task 2 test), `raios new` integration + dead-code
  removal (Task 3), MOC filename uniqueness fix (Task 1/2, `<category>-MOC.md`)
  — all covered.
- **Deferred by spec, correctly absent here:** MCP/TUI exposure, Vault101
  plugin/dashboard porting, git-tracking the vault, production status tier.
- **Type consistency check:** `ObsidianSyncReport` fields (`written`,
  `errors`, `paths`) are the same across Task 2's definition, Task 3's use
  (`report.errors.is_empty()`), and Task 4's use (`report.written`,
  `report.errors`) — no renames introduced.
- **Known limitation carried from spec, not solved here:** project-name
  collisions across categories — last write wins, no dedup logic added
  (YAGNI, no evidence of current collisions).
