# iOS (Xcode/Swift) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add native iOS/Xcode support (detection, build, test, deps, versioning) to R-AI-OS Core Toolkit.

**Architecture:** Extend `ProjectType` enum, implement `xcodebuild` and `agvtool` wrappers, and add SPM/CocoaPods parsing.

**Tech Stack:** Rust (Command pattern), xcodebuild, agvtool, Swift Package Manager.

---

### Task 1: iOS Project Detection

**Files:**
- Modify: `src/core/build.rs`
- Modify: `src/core/deps.rs`

- [ ] **Step 1: Add `Ios` to `ProjectType` enum**
- [ ] **Step 2: Write failing test for `detect_type`**
- [ ] **Step 3: Implement detection logic (`.xcodeproj`, `.xcworkspace`, `Package.swift`)**
- [ ] **Step 4: Run test to verify it passes**
- [ ] **Step 5: Commit**
```bash
git add src/core/build.rs src/core/deps.rs
git commit -m "feat: add Ios to ProjectType and implement project detection"
```

### Task 2: iOS Build

**Files:**
- Modify: `src/core/build.rs`

- [ ] **Step 1: Write failing test for `build_ios`**
- [ ] **Step 2: Implement `build_ios` using `xcodebuild -scheme <scheme> -sdk iphonesimulator build`**
- [ ] **Step 3: Run test to verify it passes**
- [ ] **Step 4: Commit**
```bash
git add src/core/build.rs
git commit -m "feat: implement xcodebuild wrapper for iOS build"
```

### Task 3: iOS Test

**Files:**
- Modify: `src/core/build.rs`

- [ ] **Step 1: Write failing test for `test_ios`**
- [ ] **Step 2: Implement `test_ios` using `xcodebuild test -destination 'platform=iOS Simulator,name=iPhone 15'`**
- [ ] **Step 3: Run test to verify it passes**
- [ ] **Step 4: Commit**
```bash
git add src/core/build.rs
git commit -m "feat: implement xcodebuild test execution for iOS"
```

### Task 4: iOS Dependencies

**Files:**
- Modify: `src/core/deps.rs`

- [ ] **Step 1: Write failing test for `check_ios`**
- [ ] **Step 2: Implement `check_ios` (Parse `Package.resolved` and `Podfile.lock` for versions)**
- [ ] **Step 3: Run test to verify it passes**
- [ ] **Step 4: Commit**
```bash
git add src/core/deps.rs
git commit -m "feat: implement dependency parsing for SPM and CocoaPods"
```

### Task 5: Smoke Test & Final Review

- [ ] **Step 1: Run full test suite** (`cargo test`)
- [ ] **Step 2: Run `cargo clippy`**
- [ ] **Step 3: Smoke test on a real iOS project** (e.g., `raios build <ios-project> --check`)
- [ ] **Step 4: Commit and Push**
```bash
git commit -am "chore: final review and smoke test for iOS support"
git push origin master
```