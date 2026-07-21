# Product Factory Phase 8: Charter-Backed Requirement Drafts

## Objective

Product work must be traceable from a requirement back to the Charter decision that justified it. Phase 8 creates the first durable requirement path without starting any planner or implementation agent.

## Local Command

```text
/factory requirement <product_id> | <stable_key> | <content>
```

The command creates a `proposed` requirement, its immutable revision 1, and a `derived_from` link to the current Charter revision in one SQLite transaction.

## Preconditions

- Factory is enabled only by trusted local configuration.
- The authenticated loopback owner owns the product.
- The product has a current Charter revision.
- Product ID and stable key satisfy strict identifier validation; content is bounded and secret-screened.

## Integrity

`stable_key` is unique within a product. Requirement revision text is stored separately from the stable record, making later revisions possible without changing identity. The requirement link points to the exact Charter revision, not merely the product, preserving historical traceability.

## Verification

- Core test proves revision content persistence and `derived_from` link creation.
- Runtime Factory mutation tests and TUI parser tests remain green.
