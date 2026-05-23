# DevOps & IaC Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Terraform, Docker, and Kubernetes analysis capabilities to R-AI-OS.

**Architecture:** Extend `ProjectType` enum for IaC. Add `core/iac.rs` to run `terraform plan`, use `trivy` (if installed) for Docker scanning, and `kubeval` for K8s manifest checks.

**Tech Stack:** Rust, Terraform, Trivy, Docker.

---

### Task 1: IaC Project Detection

**Files:**
- Modify: `src/core/build.rs`
- Modify: `src/core/deps.rs`

- [ ] **Step 1: Add `IaC` (or Terraform, Docker, K8s) to `ProjectType` enum**
- [ ] **Step 2: Write failing test for `detect_type`**
- [ ] **Step 3: Implement detection logic (`*.tf`, `Dockerfile`, `docker-compose.yml`, `*.yaml` for K8s)**
- [ ] **Step 4: Run test to verify it passes**
- [ ] **Step 5: Commit**
```bash
git add src/core/build.rs src/core/deps.rs
git commit -m "feat: add IaC to ProjectType and implement project detection"
```

### Task 2: Terraform Analysis

**Files:**
- Create: `src/core/iac.rs`
- Modify: `src/core/mod.rs`

- [ ] **Step 1: Write failing test for `run_terraform_plan`**
- [ ] **Step 2: Implement `run_terraform_plan` parsing output for changes**
- [ ] **Step 3: Run test to verify it passes**
- [ ] **Step 4: Commit**
```bash
git add src/core/
git commit -m "feat: implement terraform plan wrapper and parser"
```

### Task 3: Docker & K8s Security

**Files:**
- Modify: `src/core/iac.rs`

- [ ] **Step 1: Write failing tests for Docker/K8s security checks**
- [ ] **Step 2: Implement `check_dockerfile` and `run_trivy` if available**
- [ ] **Step 3: Implement K8s manifest validation**
- [ ] **Step 4: Run test to verify it passes**
- [ ] **Step 5: Commit**
```bash
git add src/core/iac.rs
git commit -m "feat: implement docker and k8s security validation"
```

### Task 4: Smoke Test & Final Review

- [ ] **Step 1: Run full test suite** (`cargo test`)
- [ ] **Step 2: Run `cargo clippy`**
- [ ] **Step 3: Smoke test on a real IaC project**
- [ ] **Step 4: Commit and Push**
```bash
git commit -am "chore: final review and smoke test for DevOps & IaC support"
git push origin master
```