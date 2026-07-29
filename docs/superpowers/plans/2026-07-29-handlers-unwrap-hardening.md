# Daemon Handler Unwrap Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the 7 raw `.unwrap()` calls on `serde_json::to_string(...)` in `crates/raios-runtime/src/daemon/handlers.rs`'s production path, replacing each with the safe `if let Ok(serialized) = ...` pattern already used elsewhere in the same file, so a future non-serializable field can never panic a client connection task.

**Architecture:** No behavioral change on the happy path — every value passed to these calls today serializes successfully, so output is byte-identical. This is a defensive-consistency fix: `handle_client_connection` already uses the safe pattern at 4 call sites (lines 194, 214, 256, 269) and the unsafe pattern at 7 others (lines 219, 227, 261, 322, 373-374, 445). Unifying on one pattern removes an inconsistency and a (currently theoretical, but real) panic vector in a network-facing, per-connection async task.

**Tech Stack:** Rust, tokio, serde_json — no new dependencies.

## Global Constraints

- No change to wire format / JSON shape on the success path — this is a safety refactor, not a feature change.
- Every replacement must follow the exact pattern already established in this file (`if let Ok(serialized) = serde_json::to_string(&X) { ... }`), not a new pattern.
- `cargo build`, `cargo test --lib`, and `cargo clippy` must all stay clean (0 warnings/errors) after the change — this project's baseline going into this task is already clean on all three (verified 2026-07-29).

---

## Task 1: Replace the 7 unwrap call sites in `handlers.rs`

**Files:**
- Modify: `crates/raios-runtime/src/daemon/handlers.rs:219,227,261,322,373-374,445`

**Interfaces:**
- Consumes: nothing new — uses only `serde_json::to_string`, already imported/used throughout the file.
- Produces: nothing new — this task has no downstream consumers within this plan; it is a leaf hardening change.

- [ ] **Step 1: Replace line 219 (`Command` dispatch, `Ok` arm — `SnapshotUpdated` broadcast)**

Find:
```rust
                                if let Ok(snap) = crate::control_plane::service::load_system_snapshot(&conn) {
                                    let evt = raios_contracts::Event::SnapshotUpdated(Box::new(snap));
                                    let _ = writer.write_all(format!("{}\n", serde_json::to_string(&evt).unwrap()).as_bytes()).await;
                                }
```

Replace with:
```rust
                                if let Ok(snap) = crate::control_plane::service::load_system_snapshot(&conn) {
                                    let evt = raios_contracts::Event::SnapshotUpdated(Box::new(snap));
                                    if let Ok(serialized) = serde_json::to_string(&evt) {
                                        let _ = writer.write_all(format!("{serialized}\n").as_bytes()).await;
                                    }
                                }
```

- [ ] **Step 2: Replace line 227 (`Command` dispatch, `Err` arm — `CommandFailed` ack)**

Find:
```rust
                            Err(problem) => {
                                let err_evt = raios_contracts::Event::CommandFailed {
                                    idempotency_key: cmd.idempotency_key().to_string(),
                                    problem,
                                };
                                let _ = writer.write_all(format!("{}\n", serde_json::to_string(&err_evt).unwrap()).as_bytes()).await;
                            }
```

Replace with:
```rust
                            Err(problem) => {
                                let err_evt = raios_contracts::Event::CommandFailed {
                                    idempotency_key: cmd.idempotency_key().to_string(),
                                    problem,
                                };
                                if let Ok(serialized) = serde_json::to_string(&err_evt) {
                                    let _ = writer.write_all(format!("{serialized}\n").as_bytes()).await;
                                }
                            }
```

- [ ] **Step 3: Replace line 261 (`FactoryCommand` dispatch, `Ok` arm — `SnapshotUpdated` broadcast)**

Find:
```rust
                                if let Ok(snapshot) = crate::control_plane::service::load_system_snapshot(&conn) {
                                    let event = raios_contracts::Event::SnapshotUpdated(Box::new(snapshot));
                                    let _ = writer.write_all(format!("{}\n", serde_json::to_string(&event).unwrap()).as_bytes()).await;
                                }
```

Replace with:
```rust
                                if let Ok(snapshot) = crate::control_plane::service::load_system_snapshot(&conn) {
                                    let event = raios_contracts::Event::SnapshotUpdated(Box::new(snapshot));
                                    if let Ok(serialized) = serde_json::to_string(&event) {
                                        let _ = writer.write_all(format!("{serialized}\n").as_bytes()).await;
                                    }
                                }
```

- [ ] **Step 4: Replace line 322 (`"Search"` command handler)**

Find:
```rust
                        "Search" => {
                            if let Some(query) = v["query"].as_str() {
                                let s = state_for_client.read().await;
                                if let Some(ref idx) = s.index {
                                    let results = idx.search(query);
                                    let response = format!(
                                        "{{\"event\":\"SearchResults\",\"results\":{}}}\n",
                                        serde_json::to_string(&results).unwrap()
                                    );
                                    let _ = writer.write_all(response.as_bytes()).await;
                                }
                            }
                        }
```

Replace with:
```rust
                        "Search" => {
                            if let Some(query) = v["query"].as_str() {
                                let s = state_for_client.read().await;
                                if let Some(ref idx) = s.index {
                                    let results = idx.search(query);
                                    if let Ok(serialized) = serde_json::to_string(&results) {
                                        let response = format!(
                                            "{{\"event\":\"SearchResults\",\"results\":{serialized}}}\n"
                                        );
                                        let _ = writer.write_all(response.as_bytes()).await;
                                    }
                                }
                            }
                        }
```

- [ ] **Step 5: Replace lines 373-374 (`"VectorSearch"` command handler — two unwraps in one `format!`)**

Find:
```rust
                                let response = format!(
                                    "{{\"event\":\"VectorResults\",\"results\":{},\"vector_hits\":{}}}\n",
                                    serde_json::to_string(&results).unwrap(),
                                    serde_json::to_string(&vector_hits).unwrap()
                                );
                                let _ = writer.write_all(response.as_bytes()).await;
```

Replace with:
```rust
                                if let (Ok(results_json), Ok(hits_json)) = (
                                    serde_json::to_string(&results),
                                    serde_json::to_string(&vector_hits),
                                ) {
                                    let response = format!(
                                        "{{\"event\":\"VectorResults\",\"results\":{results_json},\"vector_hits\":{hits_json}}}\n"
                                    );
                                    let _ = writer.write_all(response.as_bytes()).await;
                                }
```

- [ ] **Step 6: Replace line 445 (`"HealthScan"` command handler — `report_json` reused twice)**

Find:
```rust
                        "HealthScan" => {
                            let s = state_for_client.read().await;
                            let report_json = serde_json::to_string(&s.health_reports).unwrap();
                            let response = format!(
                                "{{\"event\":\"HealthReport\",\"report\":{}}}\n", report_json
                            );
                            let _ = writer.write_all(response.as_bytes()).await;
                            let delta = format!("{{\"event\":\"HealthDelta\",\"report\":{}}}", report_json);
                            let _ = _tx_sender.send(delta);
                        }
```

Replace with:
```rust
                        "HealthScan" => {
                            let s = state_for_client.read().await;
                            if let Ok(report_json) = serde_json::to_string(&s.health_reports) {
                                let response = format!(
                                    "{{\"event\":\"HealthReport\",\"report\":{report_json}}}\n"
                                );
                                let _ = writer.write_all(response.as_bytes()).await;
                                let delta = format!("{{\"event\":\"HealthDelta\",\"report\":{report_json}}}");
                                let _ = _tx_sender.send(delta);
                            }
                        }
```

- [ ] **Step 7: Confirm no remaining unwraps in the production path**

Run: `grep -n "\.unwrap()" crates/raios-runtime/src/daemon/handlers.rs`
Expected: every remaining match is at line number ≥ 672 (inside `#[cfg(test)] mod tests`). If any production-path (line < 672) `.unwrap()` still appears, it was missed — go back and fix it with the same pattern before continuing.

- [ ] **Step 8: Build**

Run: `~/.cargo/bin/cargo build --manifest-path /home/alaz/dev/core/R-AI-OS/Cargo.toml 2>&1 | tail -10`
Expected: `Finished` with no errors, no new warnings about unused variables (e.g. an unused `report_json` binding).

- [ ] **Step 9: Run the test suite**

Run: `~/.cargo/bin/cargo test --manifest-path /home/alaz/dev/core/R-AI-OS/Cargo.toml --lib 2>&1 | tail -15`
Expected: same pass count as baseline (60 passed; 0 failed) — no regressions, since this task changes no observable behavior on the success path.

- [ ] **Step 10: Run clippy**

Run: `~/.cargo/bin/cargo clippy --manifest-path /home/alaz/dev/core/R-AI-OS/Cargo.toml 2>&1 | grep -c "warning\|error"`
Expected: `0` (matches the pre-change baseline).

- [ ] **Step 11: Commit**

```bash
git add crates/raios-runtime/src/daemon/handlers.rs
git commit -m "fix: replace raw unwrap() with safe serde_json handling in daemon handlers

Unifies 7 inconsistent serde_json::to_string(...).unwrap() call sites in
handle_client_connection with the if-let-Ok pattern already used
elsewhere in the same function, removing a panic vector in a
network-facing per-connection task."
```

---

## Deferred (out of scope for this plan)

The `raios refactor` scan flagged 20 other HIGH-severity files (nesting depth, line count) across `raios-core`, `raios-surface-tui`, `raios-surface-cli`, `raios-surface-mcp`, and the rest of `raios-runtime`. Spot-checking `agent_wrapper.rs`, `proxy.rs`, and `git.rs` showed the nesting-depth heuristic firing on idiomatic async Rust (`tokio::select!` / nested `match` / `tokio::spawn` closures) rather than genuine defects — `proxy.rs` in particular is already deliberately hardened against a past shell-injection bug, with tests. Mechanically restructuring those files for the sake of a lower nesting-depth score is not planned; if a real defect is found in one of them later, it should get its own narrowly-scoped plan the same way this one did.
