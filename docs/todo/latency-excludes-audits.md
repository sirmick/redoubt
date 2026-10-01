# The latency targets exclude the checked build's audits

## What

The latency case and the containment gate are checked builds, because the scheduler trace needs
one ([R23 (no test channels)](../kernel/scheduling.md#r23-no-test-channels)). A checked build runs full audit scans
in the same kernel entry as the work they check:
- `check_object_indexes` after a destruction's Y record (`budget.rs`, `destroy_subtree`);
- `check_process_index` at each process-object free (`process.rs`, `index_process`).

A release build compiles none of them. [Responsiveness](../kernel/scheduling.md#responsiveness)
and [checked builds](../testbench.md#checked-builds) say each target excludes audit time. Today
nothing subtracts it.

## Why it matters

At the gate's full fill, with two full handle tables live, the audits take about 24 ms after a
destruction and 8 ms at a free. The deadline notice then measures 53.7 / 66.9 ms against 40 ms.
With the two audit calls removed, it measures 21.6 / 23.2 ms (the containment gate, rv64, seed 3).
GATE1 is held on this.

## Where

- `kernel/src/sched.rs`, `trace`: two test-only record kinds for an audit's start and end.
- `kernel/src/budget.rs` `destroy_subtree`, and `kernel/src/process.rs` `index_process`: stamp
  the audits they run.
- `tools/testbench/src/sched_oracle.rs`: subtract the audit time inside each measured window.
- The post-check: the deadline notice, which the program measures from `time_now`, and every wake
  measure.

## Done when

- The trace records each audit's start and end, in `sched-trace` builds only. A default build
  compiles none of it.
- The oracle reports the audit total. For every window a target judges, it subtracts the audit
  time inside that window: the deadline notice, the driver and steward wakes, R10 (destruction), and a lease's
  end.
- The oracle's host tests show a window with an audit inside it, and one with an audit outside
  it.
- A case keeps the target failing when audit time falls outside the stamps. For example: a
  test-only fault that leaves one audit unstamped is caught. This keeps the oracle from
  subtracting time it did not see.
- `sched-latency` and `kernel-containment` pass on both widths with the audits in. Their
  measured numbers are recorded on scheduling.md, and this page is deleted.
