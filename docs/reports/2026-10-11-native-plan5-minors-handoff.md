# Task 2 — Deferred Plan 5 workbench changes

Status: implemented and verified locally in `.worktrees/workspace-native-p3`; uncommitted.

Toasts dismiss after five seconds using a retained GPUI Task and the GPUI executor timer. A replacement toast cancels the preceding task; a command also clears the prior toast/timer. The deterministic headless test checks visibility before expiry, replacement surviving the old expiry, and disappearance at its own deadline.

`CommandId::Files` now opens the existing PaletteView in filename mode. A background recursive traversal uses ProjectRoot::list and its directory/symlink exclusions, with a 10,000-entry budget covering both files and directories. The filename query is case-insensitive and matches every query word; relative paths distinguish duplicate basenames. Displayed matches are bounded to 100 and the list scrolls with selection. The loading and truncated states have Turkish messages. A completed traversal preserves a query typed during loading. Closing the palette or switching projects cancels its retained task; project identity and palette identity also reject stale completions. Keyboard confirmation and mouse selection route through Workbench::request_open_file, preserving the unsaved-edit guard.

Tests were written before production edits. The first red run failed compilation on the missing file_paths API; the completed headless regressions pass. The UI test searches a nested filename, excludes target noise, checks unsaved edits block switching, and exercises cancel and discard. Core tests cover tree exclusions/symlinks, traversal limits, duplicate filenames, filename-only matching, and result limits. Existing save/conflict guard tests remain passing.

Commands from the native worktree, with CARGO_TARGET_DIR=/home/alaz/dev/core/R-AI-OS/.cache/rwk-native-target, CARGO_BUILD_JOBS=2 and isolated GIT_CONFIG_GLOBAL/GIT_CONFIG_SYSTEM:

```sh
cargo test --manifest-path tools/raios-workspace-native/Cargo.toml -p rwk-ui --test workbench_headless --locked
cargo fmt --manifest-path tools/raios-workspace-native/Cargo.toml --all --check
env -u RWK_UI_DISPLAY_TESTS cargo test --manifest-path tools/raios-workspace-native/Cargo.toml --workspace --locked
cargo clippy --manifest-path tools/raios-workspace-native/Cargo.toml --workspace --all-targets --locked -- -D warnings
```

Results: focused workbench 27/27; complete native suite 320 passed, zero failures, two existing ignored highlighting benchmarks; view_window explicitly skipped. Strict clippy, fmt and whitespace checks passed. Evidence in the main checkout: `.tmp/native-workbench-red.log`, `.tmp/native-workbench-green.log`, `.tmp/native-tests-final.log`, `.tmp/native-clippy-final.log`.

No terminal module or performance harness was changed. Real-window/reference-machine gates remain NOT MEASURED; they were not rerun or relaxed. Plan 6/P4 was not implemented. No commit, push, PR or merge.
