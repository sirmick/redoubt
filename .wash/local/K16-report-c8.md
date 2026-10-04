# K16 c8: worst walk, measured (STOP-AND-REPORT: R10 > 30 ms net)

Run: worst-walk at tip 248031a17 (WIP c8; 511 limit), rv64 checked build, sched-trace +
walk-trace, icount shift=3 sleep=off, qemu seed 3, 4608 MiB. Console:
`[worst-walk] 509 holders of 539 pages, 129796 threads live`; 250 holders waited for one deadline:
true; one holder destroyed, killed: true; `SCHED-TRACE-END 1811098 dropped 0`.
RERUN (impl-7, tip ff3ad8464, same tree but for c5's reflow and message; detached, 08:53-09:15):
`cargo testbench --allow-skip worst-walk` exit 1, `FAIL worst-walk [rv64, smp=1] 1290.9s
sched_oracle: R10's p99 is 11716783 µs over 2 destructions, above 30000`. The same four console
lines and `SCHED-TRACE-END 1811098 dropped 0`. The oracle's R10 equals the script's to the µs.
On a miss the oracle prints only that line, not its walk summary, so the walk, pump and reconcile
numbers below stay the script's (same rules: M/m spans, U/V audits subtracted, pumps inside X/Y),
checked against the oracle by R10. Log .k16/r511-worst-walk.log; run dir
target/testbench/run-1-1791042807416065548. The case takes 21.5 min, not 2 h.
Conditions for every number: (rv64, checked build, net of its audits, 129,796 live threads
across 510 processes) for the maxima; p50s include the fill, at lower occupancy.

## R10
- The holder's destruction (255 threads, one budget): 11,716,783 us = 11.7 s net (no audit
  inside). One pump inside it: 6,878,583 us = 6.9 s, i.e. 59% of R10.
- The probe's destruction (near-empty system): 12.9 ms, one pump of 109 us.

## Walks, net of audits (us)
- pump: 1,027 pumps, p50 4,310,572 / p99 11,285,800 / max 11,489,011; audits inside 273,485 us
  total. One delivery at full occupancy: the last pumps are 4.6 s to 11.5 s each (the main
  thread's receive of the sender's message: 6.9 s).
- expiry: 10 walks; one at the shared deadline, ending 250 waits: 28,935,720 us = 28.9 s (the
  wakes' pumps nest inside it). Others ~0.2-0.3 ms.
- reconcile: 548,525; p50 11, p99 18, max 315,692 us (0.32 s, the 250 budgets waking at once:
  red's note 2: the O(R^2 x N) wake loop is real at R=250).

## Destruction steps that pump
process_ending: once per distinct endpoint the dying threads waited on; the exit notice:
pump_endpoint on the creator's exit endpoint, once per killed process (process.rs:742);
fail_wait: per wait the destruction ends whose endpoint is pumped. Measured: one pump inside the
holder's R10 (its exit notice to init's exit endpoint), 6.9 s, walking every live thread.

## Reading
A pump walks every live thread of every process (find_thread up to three times, next_sender per
receiver): at 129,796 threads one delivery costs seconds, and R10, the timer and every wake wait
for it. This is K16-pump-ahead.md's case: owner decision (merge with residuals + the delivery
redesign package, or lower MAX_THREADS / MAX_PROCESS_COUNT). No page lines written; c8 not
committed (WIP 248031a17).
