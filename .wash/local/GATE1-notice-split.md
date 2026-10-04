# GATE1 — deadline notice split (rv64, seed 3, 2026-10-01)

Instrumentation: WIP commit on wp-gate1 (sched.rs: CT_SPLIT_A/B, reported after take_notices(D);
kernel-containment.rs prints them). Run: `in-dev cargo testbench kernel-containment --arch rv64`
with `'target missed'` dropped from forbid locally so the trace dumps (restored, not committed):
exit 1, 164.5 s, deadline notice 53675 / 66857 / 66857 (18), as before (prior run 53401/66860).
One run; repeated? no, it matches the two earlier runs' notice numbers to within 0.3 ms.

X/Y from sched-trace (µs, kernel time::now_us, the clock time_now returns); X matched as the first
X at or after each D deadline. a = X - deadline, b = Y - X, c = receipt - Y (first notice = the
agent's; last = the sub-agent's). entry = the stand-in's time_now entering take_notices(D).

| r | entry - deadline | a | b | c first | c last |
| --- | --- | --- | --- | --- | --- |
| 0 | -66092657 | 341 | 20899 | 24393 | 32557 |
| 1 | -32617487 | 270 | 20806 | 24422 | 32599 |
| 2 | -32598577 | 270 | 20423 | 37663 | 46164 |
| 3 | -32571049 | 270 | 20806 | 24434 | 32611 |
| 4 | -32604881 | 270 | 20807 | 24422 | 32599 |
| 5 | -32605989 | 270 | 20947 | 24422 | 32599 |
| 6 | -32687346 | 276 | 20806 | 24422 | 32599 |
| 7 | -32612009 | 270 | 20952 | 24423 | 32599 |
| 8 | -32609473 | 270 | 20806 | 24422 | 32599 |

p50 / p99: a 270 / 341; b 20806 / 20952; c first 24422 / 37663; c last 32599 / 46164.

Outcome **S**: in every lease the stand-in had already entered take_notices(D) ~32.6 ms (r0: 66 ms)
before the deadline, so it was blocked on exit[0] at X (H's decision, destroy, notices and
victim_control were all done). The deadline fires on time (a < 0.35 ms); the destruction is ~21 ms;
the wake after Y is 24-38 ms to the first notice and +8 ms more to the sub-agent's.

Trace after Y: the first 11 picks are budgets 42, 2, 43, 45 (system-side ids, alternating; the
destroyed lease's parent id then appears). I did not map the steward budget's kernel id (the
program holds only handles), so "the stand-in's first pick after Y" is reported via its own receipt
time rather than a trace record; c's 24 ms is ~2-3 slices of 10 ms after Y.
