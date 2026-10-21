# SCHED5: the contention gate with the hammer covering the window

Commit 8e1466a3e (n = 8, my worktree) and the same with `HAMMER_PAGES = 16` (scratch worktree,
`.tmp/SCHED5/wt`). The window is now derived from the search measured alone,
`K x (hammers x max search alone + 3 ms) x 3/2`, and the case checks every driver wake ended inside
it. `make -f scripts/jobs.mk <w>/sched-lock-contention`, two harts, checked build, icount. These
ran while train 89's bench held the machine. Under icount the figures are virtual time and
deterministic, but by the orchestrator's note they are readings, not verdicts.
Consoles: `.tmp/SCHED5/cover-n16/`, `.tmp/SCHED5/cover-n8/`. Phase fraction: `.tmp/SCHED5/phase.py`
(a wake counts as "behind a section" if a lock wait over 5 ms overlaps its window).

| n | width | search alone p50 | searches | last wake / window end (s) | behind a section | net p50 / p99 (ms) | verdict |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 16 | rv64 | 22.6 ms | 322 | 16.63 / 19.38 | 200 of 200 | 21.4 / 21.9 | p50 missed (15) |
| 16 | rv32 | 17.0 ms | 321 | 15.96 / 18.00 | 199 of 200 | 16.0 / 16.5 | p50 missed (15) |
| 8 | rv64 | 35.6 ms | 314 | 20.24 / 24.30 | 200 of 200 | 34.5 / 35.0 | p50 missed (15) |
| 8 | rv32 | 24.4 ms | 315 | 18.79 / 21.57 | 199 of 200 | 23.3 / 23.8 | p50 missed (15) |

The one rv32 wake not counted "behind a section" at each n still reads a whole search gross
(17.2 and 25.4 ms): my filter's window, not a different mechanism.

## Reading

Every loaded wake waits one whole search, phase-locked as in the n = 8 trace
(`SCHED5-trace-n8.md`): the driver's `receive` queues in ticket order behind the hammer's next
search, R78's one-section bound at two harts. Under the cap the shortest refused search is
n = 16's, 22.6 ms on rv64 and 17.0 ms on rv32, both above the 15 ms p50 target. So the positive
misses at every buildable n on both widths, whatever the interrupt routing. The two-hart gate's
wake targets cannot be met under the cap until the search is bounded (K34).

Why the earlier n = 16 run passed (p50 1.2 ms) is not traced. A 4 s window at about 25 ms a
loaded wake would have covered most samples if the lock-step had formed. So either it never formed
there, or that run's hammer was not loading. The coverage check now rules out both.
