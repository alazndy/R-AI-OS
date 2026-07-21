# Product Factory Phase 4: Local TUI Intake Transport

## Scope

Phase 4 connects the existing TUI command palette to the isolated `FactoryCommand` contract through the existing daemon IPC channel. It adds no HTTP Factory write endpoint and does not change remote authorization.

## Local Commands

When `factory.enabled = true` in trusted local configuration, the TUI accepts:

```text
/factory workspace <name>
/factory product <workspace_id> | <title>
/factory intake <product_id>
/factory answer <session_id> | <question_key> | <response>
/factory charter <product_id> | <content>
```

The UI treats values as opaque command input: it does not log the typed payload. The daemon independently validates all bounds and rejects likely secrets before persistence.

## Trust Boundary

The daemon constructs `ControlActor::local_session()` only for loopback TCP connections. Non-loopback connections receive a remote actor and the Product Factory service rejects mutation. The daemon independently loads trusted Factory configuration and passes only its boolean gate to the dispatcher.

## Failure Behavior

The command palette reports a generic status while an IPC response is pending. The typed event stream reports server-side problems into the existing TUI error state. A disabled configuration, remote client, ownership mismatch, invalid payload, or idempotency collision cannot create a Factory row.

## Verification

- The TUI parser test covers product and answer contracts plus malformed input rejection.
- Runtime Product Factory tests retain configuration, actor, ownership, idempotency, and secret-screening coverage.
- Targeted TUI and runtime test/lint commands are run before promotion.
