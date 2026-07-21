# Product Factory: Cycle Pause, Resume, and Cancel

## Local commands

```text
/factory pause-cycle <cycle_id>
/factory resume-cycle <cycle_id>
/factory cancel-cycle <cycle_id>
```

## Rules

- Only the product owner can control a cycle.
- `pause` is valid from `planned` or `active`; it prevents stage activation and
  evidence-backed completion without changing existing task, approval, or run records.
- `resume` returns a paused cycle to `active` only when an active stage was
  preserved; otherwise it returns to `planned`, so normal approval and
  activation checks still apply.
- `cancel` is terminal for the cycle. Pending and active stage rows become
  `cancelled`; completed rows, evidence, approvals, and run history are kept.

No command starts a shell, agent, build, or external submission. These controls
are the product-policy gate that later scheduler claims must honor.
