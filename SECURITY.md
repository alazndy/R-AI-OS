# Security Policy

R-AI-OS runs on your machine with elevated trust: it spawns AI agent
subprocesses, holds a hash-chained audit ledger, and mediates tool access
through its own security kernel (UMAI, policy manager, egress filter). Taking
vulnerability reports seriously here is not optional.

## Supported Versions

Only the latest released version (`main`/`master` at the current tag, see
[CHANGELOG.md](CHANGELOG.md)) receives security fixes. There is no LTS branch
at this stage of the project.

| Version | Supported |
|---------|-----------|
| Latest tagged release | ✅ |
| Older releases | ❌ |

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Preferred: use GitHub's private vulnerability reporting for this repository
(repo → **Security** tab → **Report a vulnerability**). This creates a private
advisory visible only to the maintainer until a fix ships.

If that's not available to you, email **goktugturhan74@gmail.com** with:

- A clear description of the vulnerability and its impact.
- Steps to reproduce (a minimal PoC is ideal).
- The affected version/commit.

You should get an acknowledgment within a few days. This is a small,
single-maintainer project — please be patient, but a real report will always
get a real response.

## Scope

In scope:
- The `raios`/`aiosd` binaries and all crates under `crates/`.
- The MCP tool surface, the `/a2a` HTTP endpoints, and the daemon's WebSocket
  dispatch.
- The security kernel: filesystem jail, policy manager, verify-chain (audit
  ledger), egress filter, UMAI, secret leasing, rate limiting, quarantine.

Out of scope:
- Vulnerabilities that require an attacker to already have arbitrary code
  execution as your local user (this tool trusts the local user by design —
  see the Security Kernel section in `README.md`).
- Denial-of-service reports against your own local daemon instance.
- Findings from automated scanners without a demonstrated, concrete impact
  (raw `raios security` pattern-scanner output on this repo's *own* codebase
  is not itself a report — see the caveat about that scanner's false-positive
  rate in this repo's operating notes).

## Disclosure

We follow coordinated disclosure: please give us a reasonable window to ship
a fix before any public write-up. We'll credit reporters (unless you prefer
otherwise) in the `CHANGELOG.md` entry for the fix.
