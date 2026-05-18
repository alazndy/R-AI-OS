# Plan 1: Build/Test/Deps Columns in Health View

> **Subagent-Driven Development** ile execute et.

**Goal:** `raios health` TUI ve CLI çıktısına BUILD / TEST / DEPS sütunları ekle. Yavaş işlemler opt-in (`--full` flag).

**Architecture:** `ProjectHealth` struct'a 5 yeni alan. `check_project_full()` build+test+deps çalıştırır. TUI yeni sütunları gösterir.

**Tech Stack:** Rust, mevcut `core/build.rs` + `core/deps.rs`, `ratatui::Table`

---

## File Map

| Dosya | Değişiklik |
|-------|-----------|
| `src/health.rs` | `ProjectHealth`'e 5 alan + `check_project_full()` |
| `src/ui/health.rs` | TUI tablosuna BUILD/TEST/DEPS sütunları |
| `src/cli.rs` | `Health { project, --full }` flag |
| `src/db.rs` | `health_cache` tablosuna yeni sütunlar (idempotent ALTER) |

---

## Task 1: `ProjectHealth` struct genişletme + `check_project_full()`

**Files:** `src/health.rs`

- [ ] `ProjectHealth` struct'a ekle:
```rust
pub build_ok: Option<bool>,
pub test_passed: Option<usize>,
pub test_failed: Option<usize>,
pub deps_outdated: Option<usize>,
pub deps_cve_critical: Option<usize>,
```
Mevcut `check_project()` init bloğunda tüm yeni alanlar `None` ile başlar.

- [ ] `check_project_full(proj: &EntityProject) -> ProjectHealth` ekle:
```rust
pub fn check_project_full(proj: &EntityProject) -> ProjectHealth {
    let mut h = check_project(proj);
    let path = &proj.local_path;

    let build = crate::core::build::build(path);
    h.build_ok = Some(build.ok);

    let test = crate::core::build::test(path);
    h.test_passed = Some(test.passed);
    h.test_failed = Some(test.failed);

    let deps = crate::core::deps::check(path);
    h.deps_outdated = Some(deps.outdated_count);
    h.deps_cve_critical = Some(deps.cve_critical);

    h
}
```

- [ ] `cargo check` → temiz
- [ ] Commit: `feat(health): build/test/deps fields + check_project_full()`

---

## Task 2: DB Migration

**Files:** `src/db.rs`

- [ ] `migrate()`'e idempotent ALTER'lar:
```rust
let _ = conn.execute_batch("ALTER TABLE health_cache ADD COLUMN build_ok INTEGER");
let _ = conn.execute_batch("ALTER TABLE health_cache ADD COLUMN test_passed INTEGER");
let _ = conn.execute_batch("ALTER TABLE health_cache ADD COLUMN test_failed INTEGER");
let _ = conn.execute_batch("ALTER TABLE health_cache ADD COLUMN deps_outdated INTEGER");
let _ = conn.execute_batch("ALTER TABLE health_cache ADD COLUMN deps_cve_critical INTEGER");
```

- [ ] `cargo check` → temiz
- [ ] Commit: `feat(db): health_cache build/test/deps columns`

---

## Task 3: TUI — BUILD/TEST/DEPS sütunları

**Files:** `src/ui/health.rs`

- [ ] Header'lara `"BUILD"`, `"TEST"`, `"DEPS"` ekle

- [ ] Her satır için:
```rust
let build_cell = match h.build_ok {
    Some(true)  => Cell::from("✓").style(Style::default().fg(Color::Green)),
    Some(false) => Cell::from("✗").style(Style::default().fg(Color::Red)),
    None        => Cell::from("-"),
};

let test_cell = match (h.test_passed, h.test_failed) {
    (_, Some(f)) if f > 0 => Cell::from(format!("{}✗", f)).style(Style::default().fg(Color::Red)),
    (Some(p), Some(0))    => Cell::from(format!("{}✓", p)).style(Style::default().fg(Color::Green)),
    _                     => Cell::from("-"),
};

let deps_cell = match (h.deps_cve_critical, h.deps_outdated) {
    (Some(c), _) if c > 0 => Cell::from(format!("{}CVE", c)).style(Style::default().fg(Color::Red)),
    (_, Some(o)) if o > 0  => Cell::from(format!("{}old", o)).style(Style::default().fg(Color::Yellow)),
    (Some(0), Some(0))     => Cell::from("✓").style(Style::default().fg(Color::Green)),
    _                      => Cell::from("-"),
};
```

- [ ] `Constraint::Length(7)` × 3 ekle
- [ ] `cargo check` → temiz
- [ ] Commit: `feat(tui): BUILD/TEST/DEPS columns in health view`

---

## Task 4: CLI `--full` flag

**Files:** `src/cli.rs`

- [ ] `Commands::Health`'e ekle:
```rust
/// Run full check including build, test and deps (slow — runs compiler)
#[arg(long)]
full: bool,
```

- [ ] Match arm güncelle — `full` true ise `check_project_full()` çağır

- [ ] Smoke test: `raios health --full`
- [ ] Commit: `feat(cli): raios health --full — build/test/deps check`
