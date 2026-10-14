# Handoff management

The local owner can list recorded handoffs, explicitly confirm completed work, and remove handoffs from the queue and default listing.

```sh
raios handoffs list
raios handoffs list --project /home/alaz/dev/core/R-AI-OS
raios handoffs list --all --json
raios handoffs done <approval-id>
raios handoffs remove <approval-id>
```

Use the full approval ID shown in the list. Delivery status and user completion status are separate: an approved delivery does not imply that the user confirmed the work. Completing a pending handoff resolves the delivery and records explicit user completion. Removing a pending handoff rejects delivery; removed records remain visible with `--all`. Native conversations and running wrapper sessions are unaffected.

Operations verify all linked handoff records and the local owner's approval boundary. Metadata changes, queue resolution, and audit events commit together. Repeating the same decision is idempotent; removed or rejected handoffs cannot be marked complete. Stale wrapper delivery cannot reactivate a removed handoff. No records are physically deleted.

Validation: focused database tests cover completion, idempotence, archived visibility, rejection of reactivation, stale delivery, and rollback on audit failure. CLI compilation, strict clippy, formatting, and whitespace checks passed. Release build and install outcome are recorded in `.tmp/handoff-management-*.log`.
