# Workspace native, Plan 6 (P4): inspector, handoff composer, consent dialog

Status: **skeleton for review**. No business logic is written until this is approved
(skeleton-first rule). Spec: section 8 of `docs/superpowers/specs/2026-10-05-raios-workspace-native-design.md`.

## 1. Requirement

Three panels over logic that already exists headless in `rwk-core` (`control.rs` + `control/daemon.rs`
+ `control/fake.rs`, `handoff_state.rs`, `consent.rs`). This plan adds only the GPUI layer and the glue.

| Panel | User-visible behaviour | Hard rules from the spec |
|---|---|---|
| **Inspector** | Right-hand dock tab listing the open project's tasks, runs and pending approvals, with a connection banner (connected / degraded / unreachable). | Read-only approvals (C3). Records are shown only on exact `project_path` equality; no path or ambiguous means omitted. Mutations fail closed when unreachable. |
| **Handoff composer** | Dialog: from-agent, to-agent, summary (<= 16,000 bytes). After queueing shows receipt state: Queued, Launching, Pending, Delivered, Consumed, Expired, Rejected, Missing, Unreadable. "Restart receiver" button. | Records a `BLOCKER`, not success. Client-side validation (option injection, obvious secrets) before sending. Restart enabled only when a freshly read receipt is Pending and no owned receiver runs. Only the target wrapper's delivery counts as delivered. |
| **Consent dialog** | Before any mutating command (`launch_agent`, `create_handoff`, `register_project`): ask `command_policy`; on `confirm` show the exact operation text. | Cancel, close, timeout (120 s) and shutdown send nothing. `deny` never shows a dialog, it shows the denial. Operation text is derived from the payload that will be sent. |

Edge cases to settle at review: daemon down while a dialog is open (dialog closes, nothing sent);
two mutations at once (one pending consent at a time, the second waits); project switch while a
receipt is polled (stale result dropped by project generation, as in `FileTreeView`);
symlinked project path for handoff delivery (known product follow-up, see below).

## 2. Investigation (what exists)

- `ControlPlane` trait: `work_snapshot`, `govern_snapshot`, `command_policy`, `register_project`, `launch_agent`, `create_handoff`, `handoff_receipt`. `DaemonClient` is real, `fake.rs` is the test double.
- `EventSource` + `ControlEvent::{SnapshotChanged, ConnectionChanged}`: polling first.
- `handoff_state::next(view, observation)`, `can_start_receiver(view)`: pure, all transitions tested.
- `ConsentRegistry::{begin, decide, wait, shutdown}`: single pending request, 120 s expiry.
- UI so far: `Workbench` shell with `files`, `editor`, terminal dock, `PendingAction` dialogs, palette, toast. No inspector or composer yet. The command catalog has 28 commands; the memory view already has a `Proje hafizasini goster` entry.
- Open product issue: handoff delivery for symlinked project paths (found in PR #44 CI work). Out of scope here, tracked separately.

## 3. Skeleton (types and empty functions only)

New files under `crates/rwk-ui/src/`:

```
inspector/mod.rs        // pub mod view; pub mod model;
inspector/model.rs      // pure: filtering and banner text
inspector/view.rs       // GPUI view
handoff/mod.rs
handoff/composer.rs     // GPUI dialog + validation glue
handoff/validate.rs     // pure client-side prompt validation
consent/mod.rs
consent/dialog.rs       // GPUI dialog bound to ConsentRegistry
workspace_state.rs      // WorkspaceState entity: snapshot, connection, polling task
```

```rust
// inspector/model.rs (pure, headless-testable)
pub struct InspectorModel { pub banner: Banner, pub tasks: Vec<TaskRow>, pub runs: Vec<RunRow>, pub approvals: Vec<ApprovalRow> }
pub enum Banner { None, Degraded(String), Unreachable(String) }
pub fn project_view(work: &WorkSnapshot, govern: &GovernSnapshot, project: &Path, state: &ConnectionState) -> InspectorModel { todo!() }

// handoff/validate.rs (pure)
pub enum PromptProblem { Empty, TooLong { bytes: usize }, OptionInjection, LooksLikeSecret }
pub fn validate_summary(text: &str) -> Result<(), PromptProblem> { todo!() }

// consent/dialog.rs
pub enum ConsentOutcome { Approved, Denied, Expired, ClosedByShutdown }
pub struct ConsentDialog { /* request, deadline */ }
pub fn operation_text(command: &MutatingCommand) -> String { todo!() }

// workspace_state.rs
pub struct WorkspaceState { /* connection, work, govern, project generation */ }
pub enum MutatingCommand { LaunchAgent(LaunchAgentRequest), CreateHandoff(CreateHandoffRequest), RegisterProject(PathBuf) }
```

(`todo!()` appears only in this review skeleton; the code that lands has no placeholders.)

## 4. Tasks (after approval)

1. `workspace_state.rs`: polling `EventSource` task on the background executor, project-generation guard, connection state. Headless test with `fake.rs`.
2. `inspector/model.rs` + tests (exact path match, missing path omitted, ambiguous omitted, banner texts).
3. `inspector/view.rs`: dock tab, banner, three lists, empty states. Headless render test; visual check with `import -window`.
4. `consent/dialog.rs` + `operation_text` tests (text equals payload; Cancel/close/timeout/shutdown send nothing, using `fake.rs` call log).
5. Route the existing `launch_agent` path through consent; `deny` shows the denial.
6. `handoff/validate.rs` + tests (option injection, secrets, size), then `composer.rs` over `handoff_state::next`, receipt polling, Restart receiver rule.
7. Commands in the catalog (`Inspector goster`, `Handoff olustur`) with shortcuts, palette entries, keyboard-only reachability.
8. Gates: clippy `-D warnings`, fmt, `cargo audit`, supply-chain script, UI gates re-run on a quiet machine, report.

Dependencies: tasks 1 -> 2,3; 1 -> 4 -> 5; 4 -> 6 -> 7. Estimated size 8-10 commits, no new crates.

## 5. Decisions needed from the owner

1. Inspector placement: a tab in the right dock next to the terminals (recommended) or a separate panel?
2. Approvals stay read-only in this plan (spec C3). Confirm that approve/reject remains deferred to v1.1.
3. Secret heuristics for the composer: reuse `raios-core`'s `looks_like_secret` through the application crate (injected, because `rwk-core` must not depend on `raios-core`) or a small local pattern list?
4. Should consent also guard `register_project`, or only agent launch and handoff?

## 6. Execution notes (2026-10-13)

Owner approved all four decisions as recommended: inspector as a tab in the right dock; approvals
read-only (v1.1); secret scan injected into `rwk-core` by the application crate; consent guards
`register_project`, `launch_agent` and `create_handoff`.

Done (headless, all in `rwk-core`, tested):
- `ControlPlane::now_snapshot` (+ `DaemonClient`, `FakeControlPlane`). **Finding:** pending approvals
  are in the `now` projection, not in `GovernSnapshot` as the spec and the old trait doc assumed.
- `inspector.rs`: `project_view` (exact path filter, runs by their task's project), `Banner`.
  Moved from the planned `rwk-ui/inspector/model.rs` to `rwk-core` because it is pure.
- `handoff_input.rs`: `validate_summary`, `validate_prompt`, `handoff_operation_text`; injected detector.
- `consent::gate`: policy lookup, then allow / deny / confirm-with-wait; refuses on no, expiry, shutdown.

Remaining (GPUI and wiring): `WorkspaceState` polling entity, inspector dock tab, consent dialog view,
handoff composer view, `DaemonClient` + `TokenSource` in `rwk-app`, catalog commands and shortcuts,
launch path through `gate`, visual check, gates and report.
