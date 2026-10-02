# A checked build bills its audits to the budget that runs them

## What

A checked build runs full audit scans after each destruction and at each process-object free
([checked builds](../testbench.md#checked-builds)). The bench subtracts their time from each
latency window, but the scheduler still charges it, so the audits still change the schedule:

- the running budget pays the audit's time at its weight, and its pass rises by that much;
- the audit uses up the running thread's slice, so a thread a release build would leave running
  is requeued.

In the containment gate, with two full leases live at a deadline's end, the steward stand-in takes
the agent's `killed` notice. A 10.8 ms audit at that process-object free then uses up its slice.
It is requeued, and the victim and one hostile budget run a full slice each at the floor before it
takes the sub-agent's notice. The notice is 57 ms net on rv64 (48 ms on rv32), against a 40 ms
target. Without the audit, both notices fit one slice.
[R12 (scheduling)](../kernel/scheduling.md#r12-scheduling) is behaving as designed; the cause is
the checked build.

## Why it matters

Every latency target counts the kernel a release build runs
([responsiveness](../kernel/scheduling.md#responsiveness)). Subtracting an audit's time from a
window does not take out the slices it makes others run, so a checked build schedules unlike the
kernel it checks. The tenets ask that it be the same kernel checked harder, not a special build
([tested to hell and back](../TENETS.md#6-tested-to-hell-and-back)).

## Where

- `kernel/src/sched.rs`: the charge (ticks since the slice began) and `pick`'s
  `set_slice_end`.
- The two audits and their stamps: `budget::destroy_subtree` after `Y`, and
  `MemoryManager::index_process`.

## Done when

- In a checked build, an audit's time is charged to no budget and does not count against the
  running slice. The slice's end and the charge's start move forward by the audit's length, so
  the thread that ran it is picked and preempted as in a release build. The audits stay full.
- The containment gate's two-lease run (seed 4) meets the deadline notice on both widths.
  `sched-latency` regresses no target. A recorded negative run with the audit billed misses.
- scheduling.md "Targets exclude the checked build's audits" and testbench.md "Checked builds"
  say that the audits neither fill a window nor move the schedule. The residual goes, and this
  page is deleted.
