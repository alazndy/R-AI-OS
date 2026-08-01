export const INITIAL_FACTORY_DATA = {
  overview: {
    enabled: true,
    product_count: 4,
    active_cycle_count: 2,
    pending_change_request_count: 3,
    open_support_items: 2,
    blocking_quality_profiles: 1,
    release_drafts: 1,
    completed_verify_stages: 14,
    approved_closed_testing_releases: 2,
    mode: 'governed', // 'quick' | 'governed'
    latest_product: {
      id: 'prod-01',
      title: 'R-AI-OS Control Center',
      status: 'active',
      mode: 'governed',
      project_path: '/home/alaz/dev/core/R-AI-OS',
      source_remote: 'git@github.com:alazndy/R-AI-OS.git',
      source_revision: 'd8fd8ad4919',
      stack: 'rust_tauri_react',
      scaffold_state: 'attached_existing',
      quality_blockers: 0,
      release_blockers: 1,
    }
  },

  phases: [
    { id: 0, key: 'setup', name: 'Workspace & Onboarding', status: 'completed', badge: 'Phase 0', desc: 'Attach local Git worktree or scaffold structure.' },
    { id: 1, key: 'intake', name: 'Product Intake Q&A', status: 'completed', badge: 'Phase 1', desc: 'Interactive discovery questionnaire.' },
    { id: 2, key: 'charter', name: 'Charter Drafting', status: 'completed', badge: 'Phase 2', desc: 'Human-approved project vision & scope boundary.' },
    { id: 3, key: 'requirements', name: 'Requirements Engine', status: 'active', badge: 'Phase 3', desc: 'Stable REQ-xxx key drafting & evidence linking.' },
    { id: 4, key: 'change_control', name: 'Change Control & Impact', status: 'active', badge: 'Phase 4', desc: 'Automated AI impact analysis & delta approval.' },
    { id: 5, key: 'planning', name: 'Cycle Planning', status: 'active', badge: 'Phase 5', desc: 'Materialize planned cycles, pause/resume/cancel.' },
    { id: 6, key: 'stage_graph', name: 'Stage Task Graph', status: 'active', badge: 'Phase 6', desc: 'Task graph DAG, approval gates & execution.' },
    { id: 7, key: 'quality', name: 'Quality Profiles', status: 'warning', badge: 'Phase 7', desc: 'Closed-testing profiles & device evidence gates.' },
    { id: 8, key: 'release', name: 'Release Readiness', status: 'pending', badge: 'Phase 8', desc: 'SHA-256 release drafts & closed-testing approval.' },
    { id: 9, key: 'support', name: 'Support & Triage', status: 'active', badge: 'Phase 9', desc: 'User feedback triage & CR linking.' },
  ],

  products: [
    {
      id: 'prod-01',
      title: 'R-AI-OS Control Center',
      status: 'active',
      mode: 'governed',
      project_path: '/home/alaz/dev/core/R-AI-OS',
      source_remote: 'git@github.com:alazndy/R-AI-OS.git',
      source_revision: 'd8fd8ad4919',
      stack: 'rust_tauri_react',
      scaffold_state: 'attached_existing',
      quality_blockers: 0,
      release_blockers: 1,
      created_at: '2026-07-21T14:30:00Z',
    },
    {
      id: 'prod-02',
      title: 'raios-tray Desktop App',
      status: 'active',
      mode: 'governed',
      project_path: '/home/alaz/dev/tools/raios-tray',
      source_remote: 'git@github.com:alazndy/raios-tray.git',
      source_revision: '943f919a01b',
      stack: 'python_pyside6',
      scaffold_state: 'attached_existing',
      quality_blockers: 0,
      release_blockers: 0,
      created_at: '2026-07-24T10:15:00Z',
    },
    {
      id: 'prod-03',
      title: 'Expo Mobile Control Client',
      status: 'intake',
      mode: 'quick',
      project_path: '/home/alaz/dev/mobile/raios-mobile',
      source_remote: 'git@github.com:alazndy/raios-mobile.git',
      source_revision: '71f1eb5c92d',
      stack: 'react_native_expo',
      scaffold_state: 'scaffolded',
      quality_blockers: 1,
      release_blockers: 2,
      created_at: '2026-07-28T09:00:00Z',
    },
    {
      id: 'prod-04',
      title: 'ANKA Transcript Archive Engine',
      status: 'standby',
      mode: 'governed',
      project_path: '/home/alaz/dev/ai/anka-engine',
      source_remote: 'git@github.com:alazndy/anka-engine.git',
      source_revision: '5a7d35e11f8',
      stack: 'rust_sqlite',
      scaffold_state: 'attached_existing',
      quality_blockers: 0,
      release_blockers: 0,
      created_at: '2026-07-20T16:45:00Z',
    }
  ],

  intakeSession: {
    sessionId: 'intake-sess-8841',
    productId: 'prod-01',
    mode: 'governed',
    status: 'in_progress',
    answers: [
      { key: 'target_audience', label: 'Who is the target audience?', response: 'Autonomous agent swarms (Claude, Codex, AGY) and system operators.' },
      { key: 'security_posture', label: 'What security model is required?', response: 'Strict Landlock/UMAI sandbox, owner-bound signatures, non-bypassable approvals.' },
      { key: 'data_retention', label: 'How should transcript evidence be handled?', response: 'Content-addressed SHA-256 artifact store kept out of SQLite DB.' },
      { key: 'target_platforms', label: 'Which platforms are supported?', response: 'Linux (systemd), macOS (launchd), Windows (Scheduled Tasks).' },
    ],
    pendingQuestions: [
      { key: 'offline_mode', label: 'Should local Cortex BM25 fallback function entirely offline?', type: 'boolean', hint: 'Ensures Cortex operates without WAN access.' },
      { key: 'telemetry_level', label: 'Select anonymized telemetry emission tier:', type: 'select', options: ['None (Strict)', 'Opt-in Error Logs', 'Full Audit Metrics'] }
    ]
  },

  charter: {
    id: 'charter-rev-04',
    productId: 'prod-01',
    revision: 4,
    status: 'approved',
    approvedBy: 'alaz (Owner)',
    approvedAt: '2026-07-21T18:00:00Z',
    content: `# Product Charter: R-AI-OS Control Center

## 1. Vision & Core Mission
R-AI-OS Control Center is the **hardened operating kernel** for multi-agent autonomous engineering swarms. It enforces strict policy barriers, audits every file mutation, and provides a unified 10-phase Product Factory lifecycle layer.

## 2. Scope & Boundaries
- **In-Scope:** Typed Control-Plane IPC, TUI & Tray UI, VS Code extension, Product Factory domain engine, ANKA transcript indexing.
- **Out-of-Scope:** Direct un-audited agent shell access without wrapper isolation.

## 3. Human Approval Invariants
1. All file mutations outside workspace bounds require explicit human approval.
2. Release readiness checks cannot be bypassed by Quick mode.
3. Every evidence record must be content-addressed via SHA-256.
`
  },

  requirements: [
    { id: 'req-101', key: 'REQ-001', title: 'Typed Control Plane IPC Serialization', status: 'approved', priority: 'High', evidenceCount: 3, owner: 'Security Team' },
    { id: 'req-102', key: 'REQ-002', title: 'Content-Addressed SHA-256 Artifact Store', status: 'approved', priority: 'High', evidenceCount: 5, owner: 'Core Kernel' },
    { id: 'req-103', key: 'REQ-003', title: 'React Native Closed-Testing Quality Profile', status: 'approved', priority: 'Medium', evidenceCount: 2, owner: 'Mobile Squad' },
    { id: 'req-104', key: 'REQ-004', title: 'ANKA Transcript Search with Redaction', status: 'draft', priority: 'Medium', evidenceCount: 1, owner: 'AI Intelligence' },
    { id: 'req-105', key: 'REQ-005', title: 'Real-time Tray Systemd Service Monitoring', status: 'approved', priority: 'High', evidenceCount: 4, owner: 'Desktop Tools' },
  ],

  changeRequests: [
    {
      id: 'cr-401',
      productId: 'prod-01',
      summary: 'Add Native Biometric Auth to Mobile Control Client',
      submittedBy: 'claude_kaira',
      submittedAt: '2026-07-29T11:20:00Z',
      status: 'assessed',
      impactAssessment: {
        id: 'impact-8902',
        riskLevel: 'HIGH',
        affectedRequirements: ['REQ-003', 'REQ-001'],
        affectedModules: ['raios-contracts/src/dto.rs', 'tools/raios-tray/raios-tray.py'],
        estimatedEffort: '3 days',
        securityReviewNeeded: true,
        approved: false,
        summary: 'Widens authentication boundaries. Requires explicit human approval and updated security policy rules.'
      }
    },
    {
      id: 'cr-402',
      productId: 'prod-01',
      summary: 'Upgrade brace-expansion transitive dependency to 5.0.8',
      submittedBy: 'codex_kaira',
      submittedAt: '2026-07-30T14:10:00Z',
      status: 'approved',
      impactAssessment: {
        id: 'impact-8903',
        riskLevel: 'LOW',
        affectedRequirements: ['REQ-001'],
        affectedModules: ['vscode-extension/pnpm-workspace.yaml'],
        estimatedEffort: '15 mins',
        securityReviewNeeded: false,
        approved: true,
        summary: 'Fixes GHSA-mh99-v99m-4gvg DoS vulnerability. Zero breaking contract changes.'
      }
    }
  ],

  executionCycles: [
    {
      id: 'cycle-2026-07-b',
      productId: 'prod-01',
      title: 'v3.7.2 Stabilization & Factory Visualizer Cycle',
      status: 'active', // 'planned' | 'active' | 'paused' | 'completed' | 'cancelled'
      currentStage: 'stage_3_verification',
      startedAt: '2026-07-29T08:00:00Z',
      progressPercent: 75,
      stages: [
        { key: 'stage_0_intake', name: 'Intake & Alignment', status: 'completed', completedAt: '2026-07-29T09:30:00Z', evidenceRef: 'sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855' },
        { key: 'stage_1_architecture', name: 'Architecture & Contracts', status: 'completed', completedAt: '2026-07-29T14:00:00Z', evidenceRef: 'sha256:8f4e3c2b1a0d9e8f7c6b5a4d3e2f1a0b9c8d7e6f5a4b3c2d1e0f9a8b7c6d5e4' },
        { key: 'stage_2_implementation', name: 'Implementation & Audit', status: 'completed', completedAt: '2026-07-30T17:00:00Z', evidenceRef: 'sha256:1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2' },
        { key: 'stage_3_verification', name: 'Verification & Quality Gates', status: 'active', startedAt: '2026-07-30T17:30:00Z', evidenceRef: null },
        { key: 'stage_4_release', name: 'Release Readiness & Signoff', status: 'planned', evidenceRef: null },
      ]
    }
  ],

  qualityProfiles: [
    {
      id: 'qp-react-native-01',
      name: 'React Native / Expo Closed-Testing Quality Profile',
      required: true,
      checks: [
        { name: 'TypeScript Static Compilation', passed: true, evidence: 'tsc --noEmit clean (0 errors)' },
        { name: 'Expo App Config Validation', passed: true, evidence: 'npx expo config --type public valid' },
        { name: 'Web Export Build Verification', passed: true, evidence: 'npx expo export --platform web succeeded' },
        { name: 'Dependency Audit (pnpm audit)', passed: true, evidence: '0 high/critical vulnerabilities' },
        { name: 'Android Device Evidence Snapshot', passed: false, evidence: 'Awaiting emulator test run' },
        { name: 'iOS Device Evidence Snapshot', passed: true, evidence: 'Simulator test evidence linked' },
      ]
    },
    {
      id: 'qp-rust-kernel-02',
      name: 'Rust Kernel Safety & Coverage Profile',
      required: true,
      checks: [
        { name: 'Line Coverage Floor (≥ 42%)', passed: true, evidence: 'tarpaulin report: 42.8% line coverage' },
        { name: 'Zero Panic Vectors in Refactored Modules', passed: true, evidence: 'Full manual audit of 79 refactor-flagged files' },
        { name: 'Cargo Clippy (-D warnings)', passed: true, evidence: 'cargo clippy --workspace clean' },
        { name: 'Rustdoc Missing Docs Floor', passed: true, evidence: 'raios-contracts & raios-surface-tui 100% documented' },
      ]
    }
  ],

  releaseDrafts: [
    {
      id: 'rel-v3.7.2-rc1',
      productId: 'prod-01',
      version: 'v3.7.2-rc1',
      buildRef: 'sha256:d8fd8ad49193740263f4581293847',
      status: 'pending_approval',
      createdAt: '2026-07-30T18:30:00Z',
      blockers: [
        'Android Device Evidence Snapshot in React Native Quality Profile is incomplete'
      ]
    }
  ],

  supportItems: [
    {
      id: 'supp-901',
      productId: 'prod-01',
      sourceKind: 'user_feedback',
      summary: 'Request visual graph dashboard for Product Factory cycles',
      status: 'triaged',
      linkedChangeRequestId: 'cr-401',
      createdAt: '2026-07-30T19:00:00Z',
    },
    {
      id: 'supp-902',
      productId: 'prod-02',
      sourceKind: 'bug',
      summary: 'raios-tray systemd service path drift under custom venv',
      status: 'resolved',
      resolutionRef: 'commit 943f919a01b',
      createdAt: '2026-07-29T12:00:00Z',
    }
  ],

  commandLogs: [
    { timestamp: '2026-07-30T20:45:10Z', cmd: 'raios factory overview', status: '200 OK', latency: '12ms' },
    { timestamp: '2026-07-30T20:46:02Z', cmd: 'raios factory execute AssessChangeRequest --id cr-401', status: '200 OK', latency: '45ms' },
    { timestamp: '2026-07-30T20:48:30Z', cmd: 'raios factory execute InspectReleaseReadiness --product prod-01', status: '200 OK', latency: '18ms' }
  ]
};
