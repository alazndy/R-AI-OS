---
name: Bug report
about: Something in R-AI-OS is broken
title: "[BUG] "
labels: bug
assignees: ''
---

**Describe the bug**
A clear description of what's broken.

**To Reproduce**
Exact steps / commands to reproduce. Include the exact `raios ...` or `aiosd`
invocation if applicable.

**Expected behavior**
What you expected to happen instead.

**Logs**
Relevant output from `journalctl --user -u aiosd -n 100` (daemon) or the raw
CLI output. Redact anything sensitive — do not paste secrets, tokens, or
`workspace.db` contents.

**Environment**
- OS: [e.g. Ubuntu 24.04, Windows 11, macOS 15]
- `raios --version` output:
- Install method: [cargo install / built from source / Windows installer]

**Additional context**
Anything else relevant — did this work in a previous version? Any recent
config/policy changes?
