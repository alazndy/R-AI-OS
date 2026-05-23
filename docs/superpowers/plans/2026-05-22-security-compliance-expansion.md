# Security & Compliance Expansion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add open-source license tracking and hardcoded secret scanning across all projects.

**Architecture:** Expand `src/security/scanner.rs` with high-entropy regex patterns and dependency license parsing.

**Tech Stack:** Rust (Regex), OWASP guidelines.

---

### Task 1: Secret Scanner (Hardcoded Secrets)

**Files:**
- Modify: `src/security/patterns.rs`
- Modify: `src/security/scanner.rs`

- [ ] **Step 1: Write failing test for detecting AWS keys and generic secrets**
- [ ] **Step 2: Add regex patterns (`AKIA...`, `ghp_...`, `(?i)(password|secret)`)**
- [ ] **Step 3: Run test to verify it passes**
- [ ] **Step 4: Commit**
```bash
git add src/security/
git commit -m "feat: implement hardcoded secret detection patterns"
```

### Task 2: License Compliance Scanner

**Files:**
- Create: `src/security/license.rs`
- Modify: `src/security/mod.rs`

- [ ] **Step 1: Write failing test for license parsing (`Cargo.toml`, `package.json`)**
- [ ] **Step 2: Implement logic to extract and flag copyleft licenses (e.g., GPL)**
- [ ] **Step 3: Run test to verify it passes**
- [ ] **Step 4: Commit**
```bash
git add src/security/
git commit -m "feat: implement license compliance checker"
```

### Task 3: Smoke Test & Final Review

- [ ] **Step 1: Run full test suite** (`cargo test`)
- [ ] **Step 2: Run `cargo clippy`**
- [ ] **Step 3: Smoke test by placing a fake API key in a project and running `raios security`**
- [ ] **Step 4: Commit and Push**
```bash
git commit -am "chore: final review for Security & Compliance expansion"
git push origin master
```