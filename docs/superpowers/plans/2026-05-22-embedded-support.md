# Embedded (ESP/Arduino/STM) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add build, test, and resource analysis for Embedded projects.

**Architecture:** Use `arduino-cli`, `idf.py` (ESP-IDF), and `pio` (PlatformIO) as backends. Add a "Resource Analyzer" to track Flash/RAM usage.

**Tech Stack:** Rust, arduino-cli, ESP-IDF, PlatformIO.

---

### Task 1: Embedded Project Detection

**Files:**
- Modify: `src/core/build.rs`
- Modify: `src/core/deps.rs`

- [ ] **Step 1: Add `Embedded` to `ProjectType` enum**
- [ ] **Step 2: Write failing test for `detect_type`**
- [ ] **Step 3: Implement detection logic (`sdkconfig`, `*.ino`, `platformio.ini`)**
- [ ] **Step 4: Run test to verify it passes**
- [ ] **Step 5: Commit**
```bash
git add src/core/build.rs src/core/deps.rs
git commit -m "feat: add Embedded to ProjectType and implement project detection"
```

### Task 2: Embedded Build & Flash

**Files:**
- Modify: `src/core/build.rs`

- [ ] **Step 1: Write failing test for `build_embedded`**
- [ ] **Step 2: Implement `build_embedded` (detect toolchain and compile)**
- [ ] **Step 3: Run test to verify it passes**
- [ ] **Step 4: Commit**
```bash
git add src/core/build.rs
git commit -m "feat: implement build wrapper for embedded toolchains"
```

### Task 3: Embedded Testing & Resource Analysis

**Files:**
- Modify: `src/core/build.rs`

- [ ] **Step 1: Write failing test for `test_embedded` and memory analysis**
- [ ] **Step 2: Implement `test_embedded` (e.g., `pio test` or `idf.py size`)**
- [ ] **Step 3: Run test to verify it passes**
- [ ] **Step 4: Commit**
```bash
git add src/core/build.rs
git commit -m "feat: implement test and memory size analysis for embedded projects"
```

### Task 4: Smoke Test & Final Review

- [ ] **Step 1: Run full test suite** (`cargo test`)
- [ ] **Step 2: Run `cargo clippy`**
- [ ] **Step 3: Smoke test on a real ESP/Arduino project** (`raios build <embedded-project>`)
- [ ] **Step 4: Commit and Push**
```bash
git commit -am "chore: final review and smoke test for Embedded support"
git push origin master
```