# Plan 1: Build/Test/Deps Columns in Health View

> **For agentic workers:** Use superpowers:subagent-driven-development to execute task-by-task.

**Goal:** `raios health --full` komutu BUILD / TEST / DEPS sonuçlarını gösterir. TUI health tablosuna 3 yeni sütun eklenir.

**Architecture:** `ProjectHealth`'e 5 alan ekle → `check_project_full()` fonksiyonu → DB migration → TUI sütunları → CLI `--full` flag.

**Tech Stack:** Rust, mevcut `core/build.rs` (`build()` + `test()`), `core/deps.rs` (`check()`), `ratatui::Table`

**Mevcut durum:**
- `ProjectHealth` struct: 24 alan (name, path, status, git_dirty, ..., ci_status, ci_url)
- TUI table: 9 sütun, `Constraint`'ler `[2, 10, Min(20), 12, 15, 10, 8, 8, 8]`
- `cmd_health()` satır 889, `Commands::Health { project: Option<String> }` satır 79

---

## File Map

| Dosya | Değişiklik | Satır |
|-------|-----------|-------|
| `src/health.rs` | 5 yeni alan + `check_project_full()` | 26-28 arası (struct) |
| `src/db.rs` | `migrate()`'e 5 idempotent ALTER | ~30 (migrate fn) |
| `src/ui/health.rs` | 3 yeni header + 3 Cell + 3 Constraint | ~180-213 |
| `src/cli.rs` | `--full` flag + `cmd_health_full()` | 79-84 ve 376 ve 889 |

---

## Task 1: `ProjectHealth` struct + `check_project_full()`

**Files:** `src/health.rs`

- [ ] **Step 1.1: Struct'a 5 alan ekle**

`src/health.rs` satır 26-28 arasındaki `// CI/CD` bloğundan sonra ekle:

```rust
    // Build / Test / Deps (populated only by check_project_full)
    pub build_ok: Option<bool>,
    pub test_passed: Option<usize>,
    pub test_failed: Option<usize>,
    pub deps_outdated: Option<usize>,
    pub deps_cve_critical: Option<usize>,
```

- [ ] **Step 1.2: `check_project()` init bloğuna None değerlerini ekle**

`src/health.rs`'deki `ProjectHealth { ... }` literal'inde (satır ~72-95), `ci_url,` satırından sonra:

```rust
        build_ok: None,
        test_passed: None,
        test_failed: None,
        deps_outdated: None,
        deps_cve_critical: None,
```

- [ ] **Step 1.3: `check_project_full()` fonksiyonu ekle**

`check_project()` fonksiyonundan sonra:

```rust
/// Full health check including slow operations: build, test, deps.
/// Use `--full` flag. May take 30–120s depending on project.
pub fn check_project_full(proj: &EntityProject) -> ProjectHealth {
    let mut h = check_project(proj);
    let path = &proj.local_path;

    // Build
    let build = crate::core::build::build(path);
    h.build_ok = Some(build.ok);

    // Test
    let test_result = crate::core::build::test(path);
    h.test_passed = Some(test_result.passed);
    h.test_failed = Some(test_result.failed);

    // Deps
    let deps = crate::core::deps::check(path);
    h.deps_outdated = Some(deps.outdated_count);
    h.deps_cve_critical = Some(deps.cve_critical);

    h
}
```

- [ ] **Step 1.4: cargo check**

```bash
cargo check 2>&1 | head -20
```

Beklenen: hata yok.

- [ ] **Step 1.5: Commit**

```bash
git add src/health.rs
git commit -m "feat(health): build/test/deps fields + check_project_full()"
```

---

## Task 2: DB Migration

**Files:** `src/db.rs`

- [ ] **Step 2.1: `migrate()` fonksiyonuna ekle**

`src/db.rs` satır 24'teki `fn migrate(conn: &Connection)` içinde, mevcut `let _ = conn.execute_batch("ALTER TABLE health_cache ADD COLUMN refactor_high...")` satırından sonra:

```rust
    // Build / Test / Deps columns (idempotent — errors mean column already exists)
    let _ = conn.execute_batch("ALTER TABLE health_cache ADD COLUMN build_ok INTEGER");
    let _ = conn.execute_batch("ALTER TABLE health_cache ADD COLUMN test_passed INTEGER");
    let _ = conn.execute_batch("ALTER TABLE health_cache ADD COLUMN test_failed INTEGER");
    let _ = conn.execute_batch("ALTER TABLE health_cache ADD COLUMN deps_outdated INTEGER");
    let _ = conn.execute_batch("ALTER TABLE health_cache ADD COLUMN deps_cve_critical INTEGER");
```

- [ ] **Step 2.2: cargo check**

```bash
cargo check 2>&1 | head -5
```

- [ ] **Step 2.3: Commit**

```bash
git add src/db.rs
git commit -m "feat(db): health_cache build/test/deps columns migration"
```

---

## Task 3: TUI — 3 yeni sütun

**Files:** `src/ui/health.rs`

- [ ] **Step 3.1: Header'lara 3 sütun ekle**

`src/ui/health.rs` satır ~204'teki `Row::new(vec![...])` header'ına ekle (`"TYPE",` den sonra):

```rust
        // Mevcut:
        "", "GIT", "PROJECT NAME", "COMPLIANCE", "SECURITY", "REFACTOR", "MEM", "SIG", "TYPE",
        // Yeni (TYPE'dan sonra):
        "BUILD", "TEST", "DEPS",
```

- [ ] **Step 3.2: Constraints listesine 3 yeni Constraint ekle**

`Constraint::Length(8),  // Type` satırından sonra:

```rust
        Constraint::Length(7),  // Build
        Constraint::Length(8),  // Test
        Constraint::Length(7),  // Deps
```

- [ ] **Step 3.3: Her satır için 3 yeni Cell ekle**

Row oluşturma bloğunda (her proje için `Row::new(vec![...])` çağrısında), mevcut son cell'den sonra:

```rust
        // Build cell
        let build_cell = match h.build_ok {
            Some(true)  => Cell::from("✓").style(Style::default().fg(Color::Green)),
            Some(false) => Cell::from("✗").style(Style::default().fg(Color::Red)),
            None        => Cell::from("-"),
        };

        // Test cell
        let test_cell = match (h.test_passed, h.test_failed) {
            (_, Some(f)) if f > 0 => Cell::from(format!("{}✗", f))
                                        .style(Style::default().fg(Color::Red)),
            (Some(p), Some(0))    => Cell::from(format!("{}✓", p))
                                        .style(Style::default().fg(Color::Green)),
            _                     => Cell::from("-"),
        };

        // Deps cell
        let deps_cell = match (h.deps_cve_critical, h.deps_outdated) {
            (Some(c), _) if c > 0 => Cell::from(format!("{}!CVE", c))
                                        .style(Style::default().fg(Color::Red)),
            (_, Some(o)) if o > 0 => Cell::from(format!("{}old", o))
                                        .style(Style::default().fg(Color::Yellow)),
            (Some(0), Some(0))    => Cell::from("✓")
                                        .style(Style::default().fg(Color::Green)),
            _                     => Cell::from("-"),
        };
```

Ve `Row::new(vec![...build_cell, test_cell, deps_cell])` ekle.

- [ ] **Step 3.4: cargo check**

```bash
cargo check 2>&1 | head -20
```

- [ ] **Step 3.5: Commit**

```bash
git add src/ui/health.rs
git commit -m "feat(tui): BUILD/TEST/DEPS columns in health view"
```

---

## Task 4: CLI `--full` flag

**Files:** `src/cli.rs`

- [ ] **Step 4.1: `Commands::Health` enum varyantını güncelle**

`src/cli.rs` satır 79-84'teki `Health` varyantını değiştir:

```rust
// ÖNCE:
/// Get health report for a project (dirty, compliance, etc.)
Health {
    /// Project name or path
    project: Option<String>,
},

// SONRA:
/// Get health report for a project (dirty, compliance, etc.)
Health {
    /// Project name or path
    project: Option<String>,
    /// Run full check including build, test, deps (slow — runs compiler)
    #[arg(long)]
    full: bool,
},
```

- [ ] **Step 4.2: Match arm güncelle**

Satır 376'daki match arm'ı değiştir:

```rust
// ÖNCE:
Commands::Health { project } => cmd_health(project, &cfg.dev_ops_path, cli.json),

// SONRA:
Commands::Health { project, full } => {
    if full {
        cmd_health_full(project, &cfg.dev_ops_path, cli.json);
    } else {
        cmd_health(project, &cfg.dev_ops_path, cli.json);
    }
}
```

- [ ] **Step 4.3: `cmd_health_full()` fonksiyonu ekle**

`cmd_health()` fonksiyonundan (satır 889) hemen önce:

```rust
fn cmd_health_full(project: Option<String>, dev_ops: &std::path::Path, json: bool) {
    let projects = crate::entities::load_entities(dev_ops);
    let mut results = Vec::new();

    if let Some(q) = project {
        let query = q.to_lowercase();
        for p in &projects {
            if p.name.to_lowercase().contains(&query)
                || p.local_path.to_string_lossy().to_lowercase().contains(&query)
            {
                eprintln!("Running full check for {}...", p.name);
                results.push(crate::health::check_project_full(p));
            }
        }
    } else {
        let cwd = std::env::current_dir().unwrap_or_default();
        if let Some(p) = projects.iter().find(|p| p.local_path == cwd) {
            eprintln!("Running full check for {}...", p.name);
            results.push(crate::health::check_project_full(p));
        } else {
            eprintln!("No project found for current directory. Try: raios health --full <name>");
            return;
        }
    }

    if json {
        match serde_json::to_string_pretty(&results) {
            Ok(j) => println!("{j}"),
            Err(e) => eprintln!("JSON error: {e}"),
        }
        return;
    }

    for r in &results {
        let dirty = match r.git_dirty {
            Some(true) => "DIRTY", Some(false) => "CLEAN", None => "N/A",
        };
        let build = match r.build_ok {
            Some(true) => "✓ OK", Some(false) => "✗ FAIL", None => "-",
        };
        let test = match (r.test_passed, r.test_failed) {
            (Some(p), Some(0)) => format!("✓ {p}"),
            (_, Some(f)) if f > 0 => format!("✗ {f} fail"),
            _ => "-".into(),
        };
        let deps = match (r.deps_cve_critical, r.deps_outdated) {
            (Some(c), _) if c > 0 => format!("{c} CVE!"),
            (_, Some(o)) if o > 0 => format!("{o} outdated"),
            (Some(0), Some(0)) => "✓".into(),
            _ => "-".into(),
        };
        println!(
            "{:<20} | Git:{:<5} | Comp:{} | Build:{:<7} | Test:{:<10} | Deps:{}",
            r.name, dirty, r.compliance_grade, build, test, deps
        );
    }
}
```

- [ ] **Step 4.4: cargo build**

```bash
cargo build --bin raios 2>&1 | tail -5
```

Beklenen: `Finished` satırı.

- [ ] **Step 4.5: Smoke test**

```bash
cargo run --bin raios -- health --full 2>&1 | head -5
```

Beklenen: `Running full check for <proje>...` çıktısı.

- [ ] **Step 4.6: Tüm testleri çalıştır**

```bash
cargo test --lib 2>&1 | grep "test result"
```

- [ ] **Step 4.7: Final commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): raios health --full — build/test/deps check"
```
