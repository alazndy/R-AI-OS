# R-AI-OS Product Factory — Visual Control Studio

<p align="center">
  <strong>Interactive Visual Control Studio & Dashboard for R-AI-OS Product Factory (Phases 0–9)</strong>
</p>

---

## 🚀 Overview

`raios-factory-ui` is a modern, high-performance web application providing a full visual control plane for the R-AI-OS Product Factory domain engine. It enables human operators and autonomous swarms to visualize, audit, and orchestrate products across all 10 lifecycle phases:

```
Phase 0: Workspace & Onboarding  ──► Phase 1: Product Intake Q&A   ──► Phase 2: Charter Drafting
Phase 5: Cycle Planning          ◄── Phase 4: Change Control & Impact ◄── Phase 3: Requirements Engine
Phase 6: Stage Task Graph        ──► Phase 7: Quality Profiles    ──► Phase 8: Release Readiness ──► Phase 9: Support & Triage
```

---

## ⚡ Quick Start

### 1. Development Mode

```bash
cd tools/raios-factory-ui
npm run dev
```

Open `http://localhost:5173` in your browser.

### 2. Production Build

```bash
cd tools/raios-factory-ui
npm run build
npm run preview
```

---

## ✨ Features & Architecture

- **01 | 10-Phase Pipeline Flowchart**: Visual 10-node step chain with real-time phase status badges (`completed`, `active`, `warning`, `pending`), invariant checklists, and dispatcher triggers.
- **02 | Intake & Charter Studio**: Interactive discovery questionnaire runner, Markdown Charter editor with versioning and approval sign-off.
- **03 | Change Control & Impact Assessment Matrix**: Network visualizer mapping proposed Change Requests to affected REQ keys, code modules, and AI risk levels (`HIGH`, `MEDIUM`, `LOW`).
- **04 | Execution Cycles & Task Graph DAG**: Cycle status controls (`Pause`, `Resume`, `Cancel`), stage task execution DAG matrix, and SHA-256 content-addressed evidence inspector.
- **05 | Quality Profiles & Release Gate**: Closed-testing checklist (React Native Expo config, TypeScript compilation, device evidence, Rust 42% coverage floor), release blockers counter, and release sign-off workflow.
- **06 | Support & Triage Desk**: User feedback tickets, bug triage, and automatic linking to Change Requests.
- **07 | Control-Plane IPC Terminal**: Collapsible live terminal displaying generated `raios factory` CLI commands and daemon JSON payload contracts in real time.
