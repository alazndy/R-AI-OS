# Android/Gradle Support Design

**Date:** 2026-05-22
**Test project:** GT Launcher (`c:\Users\turha\Desktop\Dev_Ops_New\05_Mobile_&_Gaming\Apps\GT Launcher`)

---

## Goal

Add Android/Gradle project type to `raios build`, `test`, `deps`, and `version-info`/`version-bump` commands. Currently these commands return "Unknown project type" for Gradle projects.

---

## 1. Detection

**File:** `src/core/build.rs` → `detect_type()`

**Criterion:** `(gradlew OR gradlew.bat) AND (build.gradle OR settings.gradle)` exist in project root.

**Priority order:** Rust → Node → Python → Go → **Android** → Unknown

Also update `src/security/scanner.rs` → `detect_project_type()` (has its own enum) with the same criterion.

---

## 2. Build — 3 Modes

**New public functions in `src/core/build.rs`:**

| Function | Gradle task | When |
|----------|-------------|------|
| `build_android(dir)` | `./gradlew assembleDebug` | default |
| `build_android_release(dir)` | `./gradlew assembleRelease` | `--release` flag |
| `build_android_check(dir)` | `./gradlew compileDebugKotlin` | `--check` flag |

Existing `build(dir)` unchanged — called from TUI health view.

**`src/cli/mod.rs`** — `Build` variant:

```rust
Build {
    project: Option<String>,
    #[arg(long)] release: bool,  // --release → assembleRelease
    #[arg(long)] check: bool,    // --check → compileDebugKotlin
}
```

**`src/cli/dev.rs`** dispatch for Android:

```rust
ProjectType::Android => {
    if opts.check { build_android_check(dir) }
    else if opts.release { build_android_release(dir) }
    else { build_android(dir) }
}
```

**Output parsing:** `BUILD SUCCESSFUL` / `BUILD FAILED` from stdout. Error line count from lines containing `error:`. No JSON diagnostic parsing (Gradle doesn't support it without plugins).

**gradlew resolution:** Try `./gradlew` first, fall back to system `gradle`.

---

## 3. Test — 2 Modes

**New public functions in `src/core/build.rs`:**

| Function | Gradle task | When |
|----------|-------------|------|
| `test_android_unit(dir)` | `./gradlew testDebugUnitTest` | default |
| `test_android_instrumented(dir)` | `./gradlew connectedAndroidTest` | `--instrumented` flag |

**`src/cli/mod.rs`** — `Test` variant adds 1 flag:

```rust
Test {
    project: Option<String>,
    #[arg(long)] all: bool,           // existing — test all projects
    #[arg(long)] instrumented: bool,  // new — Android instrumented tests
}
```

`--instrumented` is silently ignored on non-Android projects.

**Output parsing:** From stdout:
- `tests: N, failures: M` → `passed = N - M`, `failed = M`
- `BUILD SUCCESSFUL` → `ok = true`
- Fallback: count `PASSED` / `FAILED` lines.

---

## 4. Deps — Version Catalog

**New function in `src/core/deps.rs`:** `check_android(dir) -> DepsReport`

**Detection priority:**
1. `gradle/libs.versions.toml` — Gradle Version Catalog (modern projects)
2. `app/build.gradle` regex scan — `implementation "group:artifact:version"` (legacy)

**Version Catalog parse (`[versions]` block):**
```toml
[versions]
kotlin = "2.0.0"
compose = "1.7.8"
```
→ Lists all defined versions. No "outdated" check (would require Maven Central queries — out of scope).

**CVE:** Reported as `tool_missing` — requires OWASP Dependency Check Gradle plugin, which must be configured by the project.

**Output:**
```
── Android ──  lockfile: ✓ (libs.versions.toml)
  📦 23 libraries defined in version catalog
  🔒 CVE scan: not supported (add OWASP Dependency Check plugin)
```

---

## 5. Version — versionName + versionCode

**File:** `src/core/version.rs`

**Reading — `read_android_version(dir)`:**

Parses `app/build.gradle` with regex:
- `versionName '4.2.15'` → semver string
- `versionCode 42` → integer

Returns `VersionInfo { current: "4.2.15", version_file: "app/build.gradle" }`.
Display: `Version: 4.2.15 (code: 42)  (Android)`.

**Writing — `write_android_version()`:**

On `raios version-bump patch`:
1. `versionName '4.2.15'` → `versionName '4.2.16'`
2. `versionCode 42` → `versionCode 43` (always +1, regardless of bump type)
3. `CHANGELOG.md` updated via existing mechanism.

**Priority in `read_version()`:**
Cargo.toml → package.json → pyproject.toml → **app/build.gradle** → None

---

## Files Changed

| File | Change |
|------|--------|
| `src/core/build.rs` | Add `ProjectType::Android`; `build_android()`, `build_android_release()`, `build_android_check()`, `test_android_unit()`, `test_android_instrumented()` |
| `src/core/deps.rs` | Add `ProjectType::Android`; `check_android()` with libs.versions.toml parse |
| `src/core/version.rs` | Add `read_android_version()` and `write_android_version()` in `read_version()` / `write_version()` |
| `src/cli/mod.rs` | `Build`: add `--release`, `--check`; `Test`: add `--instrumented` |
| `src/cli/dev.rs` | Pass new flags through to build/test dispatch |
| `src/security/scanner.rs` | Add Android to `detect_project_type()` |

---

## Out of Scope

- `./gradlew dependencyUpdates` (needs Ben Manes plugin)
- OWASP CVE scan (needs project-level Gradle plugin)
- Emulator management for instrumented tests
- Multi-module projects beyond `app/build.gradle`
- KTS (`build.gradle.kts`) format — only Groovy DSL covered initially
