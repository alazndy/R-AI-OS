# Local patch: relative-redirect `Location` header

Upstream `hf-hub` 0.3.2 (and still 0.4.3 as of this writing) resolves an HTTP
redirect during range-probing by passing the raw `Location` header straight
into the HTTP client:

```rust
// src/api/sync.rs / src/api/tokio.rs (upstream, unpatched)
self.client
    .get(response.header(LOCATION).unwrap())
    .set(RANGE, "bytes=0-0")
    .call()
```

If the redirect target is a relative URL (no `http://`/`https://` scheme —
seen from some HF CDN edge nodes), the underlying client cannot resolve it and
the download fails. This vendored copy resolves the `Location` value against
the original request's origin before following the redirect, in both
`src/api/sync.rs` and `src/api/tokio.rs`.

Diff against crates.io `hf-hub@0.3.2` is the addition of a `final_url`
resolution block ahead of the `self.client.get(...)` call in both files —
no other changes.

Do not move this directory back under `.tmp/` or any other gitignored path:
`Cargo.toml`'s `[patch.crates-io]` entry needs it present in every checkout,
including CI. See root `Cargo.toml` comment for context.
