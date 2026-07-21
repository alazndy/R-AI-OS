# Product Factory CLI Plan

## Goal

Expose the existing Product Factory service through a local R-AI-OS CLI without
creating a new authority path or weakening the TUI/MCP approval model.

## Command Surface

```text
raios factory overview [--json]
raios factory execute --file <factory-command.json> [--json]
```

- `overview` is read-only and returns the canonical Factory snapshot.
- `execute` accepts one serialized `FactoryCommand` envelope only.
- Shell snippets, free-form SQL, credentials, and arbitrary database input are
  never accepted.

## Security Contract

1. Deserialize input into `raios_contracts::FactoryCommand`; reject invalid
   envelopes before opening a mutation transaction.
2. Use `ControlActor::local_session()` and
   `dispatch_factory_command()` so ownership, factory feature-gate,
   idempotency, transactional audit, and secret screening remain canonical.
3. Apply the same allow-list as MCP `factory_execute`.
4. Reject human-only operations with a clear `factory_approval_required`
   response:
   - plan approval;
   - approved-requirement application;
   - cycle cancellation;
   - approved-stage activation/completion;
   - release approval.
5. Never add a remote Factory write endpoint as part of this work.

## Implementation Steps

1. Add `FactoryAction` to CLI action types and `Commands::Factory` to
   argument routing.
2. Add `cli/factory.rs`:
   - render `overview` from `load_work_snapshot(...).factory`;
   - read a bounded local JSON file for `execute`;
   - deserialize and gate the typed command;
   - dispatch through the runtime service and render JSON/human output.
3. Add focused tests:
   - overview is read-only;
   - malformed JSON is rejected;
   - blocked approval variants are rejected;
   - an allowed command reaches the canonical dispatcher with an idempotency key.
4. Document examples and update `memory.md`/generated architecture map.
5. Run formatting, package tests, security/dependency checks, and a temporary
   database CLI smoke test before commit.

## Acceptance Criteria

- `raios factory overview --json` returns a Factory snapshot.
- `raios factory execute --file command.json` can run an allowed typed
  command only when the Factory feature is enabled and the local actor owns the
  resource.
- Human-only commands are denied before state mutation.
- CLI execution produces the same idempotency and audit behavior as MCP/TUI.
