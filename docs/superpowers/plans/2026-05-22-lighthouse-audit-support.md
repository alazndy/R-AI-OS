# Lighthouse Web Audit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Integrate Google Lighthouse for web performance, SEO, and accessibility auditing.

**Architecture:** Create a `core/audit.rs` module to wrap `npx lighthouse`, store results, and display scores in the TUI.

**Tech Stack:** Rust, Node.js (npx lighthouse).

---

### Task 1: Audit Core Module & Detection

**Files:**
- Modify: `src/core/build.rs`
- Create: `src/core/audit.rs`
- Modify: `src/core/mod.rs`

- [ ] **Step 1: Add `Web` to `ProjectType` enum**
- [ ] **Step 2: Write failing test for `run_lighthouse`**
- [ ] **Step 3: Implement `run_lighthouse(url: &str)` executing `npx lighthouse <url> --output=json`**
- [ ] **Step 4: Run test to verify it passes**
- [ ] **Step 5: Commit**
```bash
git add src/core/ src/core/audit.rs
git commit -m "feat: add Lighthouse wrapper module for web audits"
```

### Task 2: CLI Command `raios audit`

**Files:**
- Create: `src/cli/audit.rs`
- Modify: `src/cli/mod.rs`

- [ ] **Step 1: Write failing test for CLI arg parsing**
- [ ] **Step 2: Add `raios audit [--url <url>] [--json]` command**
- [ ] **Step 3: Run test to verify it passes**
- [ ] **Step 4: Commit**
```bash
git add src/cli/
git commit -m "feat: implement 'raios audit' CLI command"
```

### Task 3: TUI Integration

**Files:**
- Create: `src/ui/panels/audit.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Create a "Score Ring" UI component for Performance, Accessibility, SEO**
- [ ] **Step 2: Integrate into the main dashboard/health view**
- [ ] **Step 3: Commit**
```bash
git add src/ui/
git commit -m "feat: add Lighthouse visual scores to TUI"
```

### Task 4: Smoke Test & Final Review

- [ ] **Step 1: Run full test suite** (`cargo test`)
- [ ] **Step 2: Run `cargo clippy`**
- [ ] **Step 3: Smoke test on a local/remote web URL** (`raios audit --url https://example.com`)
- [ ] **Step 4: Commit and Push**
```bash
git commit -am "chore: final review and smoke test for Lighthouse support"
git push origin master
```