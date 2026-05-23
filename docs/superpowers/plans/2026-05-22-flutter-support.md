# Flutter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add full Flutter lifecycle support (detection, build, test, deps) to R-AI-OS.

**Architecture:** Detect `pubspec.yaml`, wrap `flutter` CLI for build/test, and parse `pubspec.lock`.

**Tech Stack:** Rust, Flutter SDK, Dart.

---

### Task 1: Flutter Project Detection

**Files:**
- Modify: `src/core/build.rs`
- Modify: `src/core/deps.rs`

- [ ] **Step 1: Add `Flutter` to `ProjectType` enum**
- [ ] **Step 2: Write failing test for `detect_type` (pubspec.yaml)**
- [ ] **Step 3: Implement detection logic (Must prioritize Flutter over generic Android/iOS)**
- [ ] **Step 4: Run test to verify it passes**
- [ ] **Step 5: Commit**
```bash
git add src/core/build.rs src/core/deps.rs
git commit -m "feat: add Flutter project detection via pubspec.yaml"
```

### Task 2: Flutter Build

**Files:**
- Modify: `src/core/build.rs`

- [ ] **Step 1: Write failing test for `build_flutter`**
- [ ] **Step 2: Implement `build_flutter` using `flutter build <target> --release`**
- [ ] **Step 3: Run test to verify it passes**
- [ ] **Step 4: Commit**
```bash
git add src/core/build.rs
git commit -m "feat: implement flutter build wrapper"
```

### Task 3: Flutter Test

**Files:**
- Modify: `src/core/build.rs`

- [ ] **Step 1: Write failing test for `test_flutter`**
- [ ] **Step 2: Implement `test_flutter` using `flutter test --reporter json` and parse results**
- [ ] **Step 3: Run test to verify it passes**
- [ ] **Step 4: Commit**
```bash
git add src/core/build.rs
git commit -m "feat: implement flutter test execution with JSON reporting"
```

### Task 4: Flutter Dependencies

**Files:**
- Modify: `src/core/deps.rs`

- [ ] **Step 1: Write failing test for `check_flutter`**
- [ ] **Step 2: Implement `check_flutter` using `flutter pub outdated --json`**
- [ ] **Step 3: Run test to verify it passes**
- [ ] **Step 4: Commit**
```bash
git add src/core/deps.rs
git commit -m "feat: implement flutter pub outdated integration"
```

### Task 5: Smoke Test & Final Review

- [ ] **Step 1: Run full test suite** (`cargo test`)
- [ ] **Step 2: Run `cargo clippy`**
- [ ] **Step 3: Smoke test on a real Flutter project** (`raios health <flutter-project>`)
- [ ] **Step 4: Commit and Push**
```bash
git commit -am "chore: final review and smoke test for Flutter support"
git push origin master
```