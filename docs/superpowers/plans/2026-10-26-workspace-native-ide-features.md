# Workspace native: IDE features (LSP, git, tasks, editor)

Status: **mostly shipped** (PRs #75–#90). Owner asked (2026-10-26) to work through the gap list in order. Checkboxes below mark what is in `master`; they were checked against the code on 2026-11-03, not against a user-facing test run.
Builds on `2026-10-05-raios-workspace-native-design.md` and `2026-10-24-workspace-native-orchestration-design.md`.

## Order and why

1. **LSP and a Problems panel**: the largest gap; without it the editor is a text box with colours.
2. **Git status, stage, commit**: the quickest visible win; also where an agent's edits are reviewed.
3. **Task runner**: build/test commands whose output links to files.
4. **Find and replace, split editor**, then the smaller items (auto-save, formatter on save).

Each step ends with tests, clippy `-D warnings`, the core boundary check and its own PR. No step is started before the previous one is merged.

## 1. LSP

### Boundaries

- `rwk-core::lsp` holds the protocol client: Content-Length framing, JSON-RPC, a child process over stdio, document sync, diagnostics. Toolkit-free, thread-based (no async runtime; `tokio` is forbidden in core). Uses `lsp-types` (already in the lock through `gpui-base`).
- `rwk-ui` turns the client into the `gpui-base` provider traits (`CompletionProvider`, `HoverProvider`, `DefinitionProvider`, `CodeActionProvider`) and a `DiagnosticSet` per tab, and draws the Problems panel.

### Security rules

- A server is chosen from a **fixed catalog** (`rust-analyzer`, `typescript-language-server --stdio`, `pyright-langserver --stdio`, `gopls`, `clangd`). The project cannot name a command: no project-relative lookup, no shell, only absolute `PATH` entries.
- Incoming URIs are mapped back to project-relative paths and dropped when they leave the project root or contain `..`.
- A message is capped at 16 MiB and the header block at 16 lines; a server that misbehaves is killed, never trusted to stop.
- Server requests the client does not implement get an empty answer, so a server never waits on us.
- A server that is missing is a quiet "no language server for this file", not an error dialog.

### Slices

- [x] 1. Core client: framing, URI mapping, catalog, client with initialize/didOpen/didChange/didClose/shutdown and `publishDiagnostics`, tested against a fake server (this file's first commit).
- [x] 2. A `LanguageServers` manager in core: one server per language per project, started on first file of that language, restarted once after a crash, stopped with the project.
- [x] 3. UI: diagnostics into the editor (underlines, gutter), Problems panel with click-to-open, status line count.
- [x] 4. UI: hover, go to definition, completion.
- [x] 5. Code actions, rename symbol, format on save.

## 2. Git (outline)

- [x] Status, stage/unstage/discard, hunk staging, commit, branches, pull/push/stash, tree and tab badges (#75, #77, #79–#81).
- [ ] Not verified: the secret-looking check on the commit message.

`git status --porcelain=v2 -z` through the existing bounded-process pattern; file list with stage/unstage/discard (discard asks); commit message box with the secret-looking check the handoff uses; branch list and switch (refuses with a dirty tree that conflicts); tree and tab badges. An agent's working-tree changes show up here without extra wiring.

## 3. Task runner (outline)

- [x] Named commands run in a terminal pane; output paths are clickable (#75, #76).
- [ ] Not verified: that commands come only from the user-owned config file.

Named commands per project from a user-owned file in the config directory (never from the project), run in a terminal pane, output matched against `path:line:col` patterns so a click opens the file.

## 4. Editor (outline)

- [x] Find and replace (with regex), project replace, split editor, auto-save, format on save (#76, #77, #82).
- [ ] Not verified: the project-wide replace preview.

Find and replace in file (regex off by default) and project-wide replace through the search index with a preview; side-by-side editors; auto-save on focus loss; formatter on save through the LSP where offered.
