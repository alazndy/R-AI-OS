# Handoff lifecycle repair — 2026-10-10

The wrapper previously injected a pending handoff immediately, without offering a
decision. The control-plane reject command changed only `cp_approvals`, leaving
its task/run active. Delivery used four independent writes and could overwrite a
previous rejection or supersession. Successful results submitted outside the
wrapper did not acknowledge the incoming handoff, leaving completed work queued.

## Resulting behavior

An interactive `raios run` shows the pending handoff and asks before starting the
child:

- `a`: accept; deliver context and acknowledge it after a successful spawn.
- `r`: reject; close approval/artifact and cancel the handoff task/run.
- `c`: already completed; acknowledge the handoff without injecting its context.
- `l`, Enter or EOF: later; keep it pending without injecting its context.

Reject, complete and defer still start a normal agent session with the caller's
own arguments. No database transaction remains open while waiting for a choice.
Explicit scripting choices are available before the agent's trailing arguments:

```sh
raios run codex --handoff reject
raios run codex --handoff complete
raios run codex --handoff defer -- "work on something else"
raios run codex --handoff accept
```

The default `--handoff auto` asks only when stdin and stdout are terminals. In
headless execution it preserves automatic delivery and does not read stdin.
Informational invocations continue to leave handoffs alone; failed spawns leave
accepted handoffs pending.

A successful outgoing handoff also acknowledges the sender's incoming pending
handoff for the same project/path. Failed/blocker reports do not do this. Other
projects and unrelated approval workflows remain untouched. Supersession now
cancels only handoff tasks, rather than every awaiting task assigned to that agent.

## Persistence and boundaries

`cp_resolve_handoff` updates approval, artifact, handoff run and delivery task in
the caller's SQLite transaction. It validates ownership and canonical links and
requires pending delivery states. Wrapper acknowledgment is transactional and
audited; the control-plane command uses the same resolver with its existing
authorization, policy, idempotency and atomic audit boundary. An old wrapper
context cannot reopen rejected or superseded records.

These records track delivery/acknowledgment. The separately tracked wrapper
session retains its own execution outcome. No table, wire command, manifest,
dependency or permanent systemd configuration was added.

Changed code: `crates/raios-core/src/db/wf_handoff.rs`,
`crates/raios-runtime/src/agent_runner.rs`,
`crates/raios-runtime/src/control_plane/service.rs`, and CLI run parsing/routing.
Existing uncommitted changes were preserved. The in-memory snapshot test no
longer assumes the host's Factory setting is disabled; default-off configuration
already has dedicated `Config::default` tests.

## Verification

- `cargo test --workspace --locked`: **1,129 passed**, zero failed,
  two existing ignored doc examples.
- Strict all-target/all-feature Clippy: passed.
- Formatting and whitespace checks: passed.
- Release workspace build: passed. Changed core/runtime/CLI security scans
  found zero HIGH/CRITICAL matches; dependency scan found no known Rust CVEs.
- CLI process tests exercise reject/complete/defer with preserved user prompts,
  unchanged other projects and closed linked records.
- A real pseudo-terminal exercise verified all four menu actions, including that
  the child had not started before the human decision.
- Regression tests cover stale-context rejection, transaction rollback,
  receiver binding, audit integrity, successful return closure scoped to the
  workspace, and preservation of unrelated awaiting tasks.

Evidence: `.tmp/codex-handoff-lifecycle-2026-10-10/`.

## Installed and live verification

The validated `raios` and `aiosd` releases were installed atomically; the prior
binaries are retained in the private evidence directory for recovery. The
existing transient systemd user service restarted successfully, with
`ActiveState=active`, PID 170565 at verification and ports 42069/42070/42071 ready.
The four real terminal menu cases also passed against the installed binary.

Installed SHA-256 values match `target/release`:

- `raios`: `e0d0d2643271795f29f24970ee57a0c052196fe18fcc212fea9a2e09594e35cd`
- `aiosd`: `79382694fd375cd2317002b972eae2833669b7f4b707fddad9b0ff660704eefb`

Submitting the native success report as `codex_kaira` closed the previously
completed incoming task `55fb638b-e1ad-42ed-9227-861bf96286cc`: approval approved,
task completed, run succeeded and artifact applied. The old Claude result note
was superseded with its task/run cancelled. Only the new result note remains
pending delivery to Claude; no receiving agent was launched.

New result task: `ac72fbb9-7fad-4e16-8cc4-fb7c122d146e`; approval:
`3ea04e1c-6817-4834-9a8a-64d2f941f37c`. Live audit chain verified 3,574 entries.
This used native workflows only; no raw SQL mutations, record deletion or
age-based expiration were performed.

No commits or changes to `tools/raios-workspace*`, `docs/superpowers/plans`, README
or SIGMAP were performed. No remote CI or cross-platform run was performed.
