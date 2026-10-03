# Destruction walks every process

## What

A budget's destruction ([R10 (destruction)](../kernel/budgets.md#r10-destruction)) finds what it
ends among every process, not among the processes of the dying subtree. Three of its steps walk
every live PID or every process object, decoding each, about 12 µs a process:

- `budget::destroy_subtree` asks of every live PID whether it runs in a dying budget, to kill it;
- `process::budgets_dying` looks for the process objects charged to a dying budget with
  `find_process`, once for each object it frees and once more to find none;
- `destroy_marked`'s `migrate_held_pids` reads every process object for the PIDs still counted
  in a dying budget (R10's step 8).

The walks follow what exists: at most `MAX_PROCESS_COUNT` (511) processes.

Measured by `bench:worst-walk`, every PID in use, each process in a budget of its own holding
`MAX_THREADS` threads (checked build, net of its audits, 129,796 live threads across 510
processes): one holder's destruction takes 53.5 ms on rv64 and 58.3 ms on rv32, against R10's
30 ms. The same destruction with the system near empty takes 16.1 and 17.1 ms. Its threads'
teardown (12.9 and 13.6 ms) and the pump of its creator's exit endpoint (under 30 µs) cost the
same at both fills; the difference is the three walks.

## Why it matters

The kernel is not preemptible: every interrupt, wake and timeout on the machine waits for a
destruction. [R12 (scheduling)](../kernel/scheduling.md#r12-scheduling) says a system call's
kernel time never depends on what other processes hold, and R10's 30 ms is what a lease's end
may cost. Both hold at full occupancy only by the constants clause.

## Where

- `kernel/src/budget.rs`: `destroy_subtree`'s loop over `live_pids`.
- `kernel/src/process.rs`: `budgets_dying` with `find_process`, and `migrate_held_pids`.

## Done when

- Per-budget chains of the processes running in a budget, and of the process objects charged to
  it, make each of the three steps follow the dying subtree, never every process, in the package
  that re-cuts the destroy path.
- `bench:worst-walk`'s R10 at full occupancy is within 30 ms net on both widths: its `must_fail`
  on R10 is gone.
- [Budgets](../kernel/budgets.md#residual-risks)' and
  [scheduling](../kernel/scheduling.md#residual-risks)'s residuals say what the code then does.
- This page is deleted.
