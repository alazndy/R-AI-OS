# Workspace native: UI gate measurements (2026-10-13)

First run in which windows receive frames (GPUI `gpui_platform` features `wayland`/`x11` were
missing before). Command: `ui-bench all --enforce --runs 5` (release build, XWayland, i7 laptop,
machine **not quiet**: 24-29 % foreign CPU, another agent was building).

| Gate | Measured | Limit | Result |
|---|---|---|---|
| U1 spawn to first painted frame | 288.8 ms | <= 300 ms | PASS |
| U2 fps, 4M-line flood | 49.8 | >= 45 | PASS |
| U3 fps, 4 concurrent floods | 28.0 | >= 30 | FAIL (busy machine) |
| U4 keystroke to painted echo, p99 | 15.1 ms | <= 16 ms | PASS |
| U5 idle PSS, 4 terminals | 65.5 MB | <= 150 MB | PASS |
| E1 open 30K lines | 281.4 ms | <= 350 ms | PASS |
| E2 open 500K lines | 322.7 ms | <= 700 ms | PASS |
| A1 rwk-app spawn to first frame | 296.4 ms | <= 300 ms | PASS |
| A2 rwk-app idle PSS | 75.6 MB | <= 150 MB | PASS |

Verdict: INCONCLUSIVE, as the harness itself reports for a failing gate on a busy machine.
U3 must be re-run on a quiet machine; U1 and A1 are within 4 % of their limits and need the same.
Frames are painted frames, not presented ones; keystroke latency excludes compositor and display.
