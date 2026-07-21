# Contributing to R-AI-OS

Thanks for considering a contribution. R-AI-OS is a hardened Rust kernel with a
strict security posture (see [SECURITY.md](SECURITY.md)), so a few extra rules
apply on top of the usual PR workflow.

## Before you start

- **Open an issue first** for anything beyond a trivial fix (typos, docs, small
  bug fixes are fine to PR directly). Larger changes — new tools, new MCP
  surface, new daemon workers — should be discussed first so you don't spend
  time on something that won't land.
- Read `AGENT_CONSTITUTION.md`-equivalent conventions in `README.md` and
  `SIGMAP.md` for the current architecture map before touching unfamiliar code.

## Development setup

```bash
cargo build --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo audit
```

All four must be clean before opening a PR. CI runs the same checks on
Linux/Windows/macOS plus a coverage pass — a red CI run will not be merged.

## Code standards

- **No lazy shortcuts.** No `// TODO: implement later`, no `// ...rest of code`.
  Full, compilable, production-ready diffs only.
- **Root cause over symptom fixes.** If you're fixing a bug, explain the root
  cause in the PR description, not just the patch.
- **Tests are not optional.** New behavior needs a new test; bug fixes need a
  regression test that fails before the fix and passes after.
- **Security-sensitive changes require extra scrutiny.** Anything touching
  `src/security/`, `daemon/`, `db.rs`'s `cp_*` write paths, or the MCP/HTTP
  surface must explain the threat model in the PR description — see the
  OWASP-aligned rules in `SECURITY.md`.
- Write through `cp_*` functions in `db.rs`. Never write directly to legacy
  tables.

## Commit / PR conventions

- Commit messages: short, imperative, English (`fix: ...`, `feat: ...`,
  `docs: ...`) — see `git log` for the existing style.
- One logical change per PR. Don't bundle refactors with behavior changes.
- Update `CHANGELOG.md` for any user-visible change.

## Reporting bugs / requesting features

Use the GitHub issue templates (`.github/ISSUE_TEMPLATE/`). For security
vulnerabilities, do **not** open a public issue — see
[SECURITY.md](SECURITY.md) for the private disclosure process.

## License

By contributing, you agree that your contributions will be licensed under the
project's [AGPL-3.0](LICENSE) license.
