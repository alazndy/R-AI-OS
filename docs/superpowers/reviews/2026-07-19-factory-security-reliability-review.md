# Product Factory Security and Reliability Review

Date: 2026-07-19

## Verified local controls

- Factory writes remain disabled by default, loopback-local, owner-bound,
  idempotent, audited, and secret-screened at the runtime boundary.
- Cycle pause/resume/cancel prevents new stage activation or completion while
  paused and preserves terminal cancellation history.
- Stage completion requires non-empty evidence. Dependency-linked evidence is
  marked stale on its requirement revision; stale evidence blocks release.
- Artifact bytes are content-addressed outside SQLite. Text resembling a secret
  and oversized content are refused before artifact storage.
- React Native/Expo is classified before generic Node. The Factory inspector
  remains read-only and does not inspect credentials or start provider work.

## Test evidence

The following passed from the current worktree:

| Surface | Result |
| --- | --- |
| `raios-core` library | 449 passed |
| `raios-contracts` library | 5 passed |
| `raios-runtime` library | 223 passed |
| `raios-surface-tui` library | 16 passed |
| Core/runtime clippy with `-D warnings` | passed |
| Factory React Native `npm run verify:closed-testing` | passed |

The pilot verification covers TypeScript, public Expo configuration, web
export, and the high/critical `npm audit` threshold.

## Dependency and scanner findings

- `raios deps .`: no known Rust CVE and no outdated dependency reported.
- Pilot `npm audit --audit-level=high`: no high or critical finding; 10
  moderate Expo CLI/config-chain advisories remain. The offered `--force` fix
  would downgrade Expo to SDK 46, so it was not applied without a supported,
  compatibility-tested remediation decision.
- `raios security . --full` returned four non-Factory pattern findings:
  one dynamic SQL construction in the database-budget scanner using its fixed
  internal table list, two legacy VS Code URL validation strings permitting
  `http`, and one escaped `innerHTML` error rendering path. These need normal
  owner review but were not changed blindly by this Factory review.

## Operational constraints

- Shared `workspace.db` is 2.8 GB against the 500 MB health cap. This is a
  workspace-wide retention/index issue; no cleanup was performed because it
  would be destructive shared-state work.
- `adb devices` reports no attached/emulated Android device.
- `xcrun` and EAS CLI are unavailable on the current Linux host.
- There are no configured store account identifiers, signing credentials, or
  tester channels. No external build, submission, or store status was claimed.

## Remaining acceptance evidence

The goal is not closed locally until an owner-provided Android/iOS device or
approved hosted provider records real install/test evidence, and authorized
Google Play/TestFlight accounts record actual tester-channel availability. The
Factory must bind that future external evidence to an artifact and exact owner
approval rather than treating this local review as a substitute.
