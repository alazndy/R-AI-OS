# Terminal engine: headless gate run

- Date: 2026-10-08
- Commit: 8ad3aab (`feat/workspace-native-p2a`: Plan 4 as executed by opencode with `nemotron-3-ultra-free` and independently verified)
- Machine: Intel Core i7-8550U, 8 threads, KDE Wayland (the spec's reference machine), also running the owner's normal desktop work
- Harness: `term-bench --enforce --runs 5` (release build), three consecutive runs
- Exit statuses: run 1 = 1 (a gate failed on a quiet machine), run 2 = 0 (all passed), run 3 = 3 (inconclusive: other processes used 29% of the machine during the run, the limit is 25%)

## Summary

| Gate | Limit | Run 1 | Run 2 | Run 3 |
|---|---|---|---|---|
| G1 parse 1M plain lines (from memory) | 0.70 s | 0.169 | 0.240 | 0.157 |
| G2 parse 300K colored lines (from memory) | 1.00 s | 0.656 | 0.883 | 0.703 |
| G3 1M-line PTY flood versus the do-nothing floor | 1.20 x | 1.011 | 1.118 | 0.940 |
| G4 peak RSS during a 4M-line flood | 256 MiB | 15.1 | 14.8 | 16.0 |
| G5 snapshot p99 during a 4M-line flood | 5.5 ms | 4.38 | 4.57 | 3.44 |
| G6 snapshot max during a 4M-line flood | 33 ms | 16.6 | 10.8 | 8.1 |
| G7 4 concurrent 1M-line floods fully parsed | 2.80 s | 1.049 | 1.096 | **3.105** (busy machine) |
| G8 snapshot p99 with 4 concurrent floods | 8.3 ms | **8.325** (quiet) | 7.238 | **60.760** (busy machine) |

## Output

### Run 1

```text
machine    : Intel(R) Core(TM) i7-8550U CPU @ 1.80GHz, 8 threads
foreign cpu: 25% of the machine before the run (quiet means at most 25%); 5 runs per gate, median for times, worst for latencies

flood      : 34 MB through a PTY in 1.84 s (median), 240 snapshots, 238 wake-ups
pty plain  : 0.434 s through the engine, 0.430 s floor (reader discards)
pty colored: 1.075 s through the engine, 0.550 s floor, 0.656 s parse-only (not a gate)

G1 parse 1M plain lines (from memory)                 measured    0.169 s   limit   0.700 s   PASS
G2 parse 300K colored lines (from memory)             measured    0.656 s   limit   1.000 s   PASS
G3 1M-line PTY flood versus the do-nothing floor      measured    1.011 x   limit   1.200 x   PASS
G4 peak RSS during a 4M-line flood                    measured   15.125 MiB limit 256.000 MiB PASS
G5 snapshot latency p99 during a 4M-line flood        measured    4.378 ms  limit   5.500 ms  PASS
G6 snapshot latency max during a 4M-line flood        measured   16.606 ms  limit  33.000 ms  PASS
G7 4 concurrent 1M-line floods fully parsed           measured    1.049 s   limit   2.800 s   PASS
G8 snapshot latency p99 with 4 concurrent floods      measured    8.325 ms  limit   8.300 ms  FAIL

foreign cpu during the run: 22%
AT LEAST ONE GATE FAILED ON A QUIET MACHINE
exit=1
```

### Run 2

```text
machine    : Intel(R) Core(TM) i7-8550U CPU @ 1.80GHz, 8 threads
foreign cpu: 18% of the machine before the run (quiet means at most 25%); 5 runs per gate, median for times, worst for latencies

flood      : 34 MB through a PTY in 2.39 s (median), 259 snapshots, 257 wake-ups
pty plain  : 0.571 s through the engine, 0.511 s floor (reader discards)
pty colored: 1.350 s through the engine, 0.681 s floor, 0.883 s parse-only (not a gate)

G1 parse 1M plain lines (from memory)                 measured    0.240 s   limit   0.700 s   PASS
G2 parse 300K colored lines (from memory)             measured    0.883 s   limit   1.000 s   PASS
G3 1M-line PTY flood versus the do-nothing floor      measured    1.118 x   limit   1.200 x   PASS
G4 peak RSS during a 4M-line flood                    measured   14.832 MiB limit 256.000 MiB PASS
G5 snapshot latency p99 during a 4M-line flood        measured    4.571 ms  limit   5.500 ms  PASS
G6 snapshot latency max during a 4M-line flood        measured   10.780 ms  limit  33.000 ms  PASS
G7 4 concurrent 1M-line floods fully parsed           measured    1.096 s   limit   2.800 s   PASS
G8 snapshot latency p99 with 4 concurrent floods      measured    7.238 ms  limit   8.300 ms  PASS

foreign cpu during the run: 48%
ALL HEADLESS GATES PASSED
exit=0
```

### Run 3

```text
machine    : Intel(R) Core(TM) i7-8550U CPU @ 1.80GHz, 8 threads
foreign cpu: 16% of the machine before the run (quiet means at most 25%); 5 runs per gate, median for times, worst for latencies

flood      : 34 MB through a PTY in 1.92 s (median), 247 snapshots, 246 wake-ups
pty plain  : 0.465 s through the engine, 0.494 s floor (reader discards)
pty colored: 1.164 s through the engine, 0.564 s floor, 0.703 s parse-only (not a gate)

G1 parse 1M plain lines (from memory)                 measured    0.157 s   limit   0.700 s   PASS
G2 parse 300K colored lines (from memory)             measured    0.703 s   limit   1.000 s   PASS
G3 1M-line PTY flood versus the do-nothing floor      measured    0.940 x   limit   1.200 x   PASS
G4 peak RSS during a 4M-line flood                    measured   16.016 MiB limit 256.000 MiB PASS
G5 snapshot latency p99 during a 4M-line flood        measured    3.439 ms  limit   5.500 ms  PASS
G6 snapshot latency max during a 4M-line flood        measured    8.056 ms  limit  33.000 ms  PASS
G7 4 concurrent 1M-line floods fully parsed           measured    3.105 s   limit   2.800 s   FAIL
G8 snapshot latency p99 with 4 concurrent floods      measured   60.760 ms  limit   8.300 ms  FAIL

foreign cpu during the run: 29%
INCONCLUSIVE: a gate failed, but the machine was busy; run again when it is quiet
exit=3
```

## Reading

- **G1-G6 passed in all three runs**, with margin: parsing is 3-4 times faster than the limit for plain output and about 1.4 times for colored output; the engine adds no measurable time over a reader that does nothing (G3 between 0.94 and 1.12); memory stays near 15 MiB (the spike's prototype used 430-540 MB); one snapshot costs at most 4.6 ms at the 99th percentile while a flood runs.
- **G7 and G8 measure four floods that saturate all eight hardware threads** (each flood is a shell, a producer, a reader thread, a parser thread and a writer thread). Their result depends on scheduling more than on the engine. They passed in run 2, failed marginally in run 1 (G8 8.325 ms against 8.3 ms, a 0.3% miss, with other processes using 22-25% of the machine) and failed clearly only in run 3, which the harness itself marks inconclusive because the machine was busy.
- The part of the latency the engine controls is the time the emulator lock is held per slice (8 KiB, a few tenths of a millisecond). The rest of a p99 of 7-8 ms under this stress is the snapshot thread waiting to be scheduled among 20-plus runnable threads, which a real UI would also experience.
- The quiet-machine rule held up: a pass counts on any machine, a failure counts only when other processes used at most 25% of the machine.

## Decision (the spec's P2 stop criterion)

By the decision table of Plan 4: not "every gate passed", and not a failure of G1, G2, G4, G5, G6 or G7 on a quiet machine. Only G8 failed on a quiet machine, once, by 0.3%.

**The engine is sound; Plan 4b may start.** G8 is kept as a monitored risk, not a blocker, for two reasons: its limit is a proxy (a quarter of a 30 fps frame) built before any measurement, and the true test of the same question is the frame rate of a real GPUI window under four floods, which is exactly what Plan 4b measures. If that fails, the levers are known and cheap: smaller parse slices (4 KiB), a lower priority for the reader and parser threads, and a longer sampling interval. The limit is not changed to make the number pass.
