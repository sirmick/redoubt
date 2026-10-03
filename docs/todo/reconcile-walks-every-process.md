# Reconcile walks every process

## What

At the end of every kernel entry the scheduler reconciles its queue with the budgets that have a
ready thread ([scheduling](../kernel/scheduling.md)). It finds them by walking every live process:
`Runnable::fill` (`kernel/src/sched.rs`) reads each process's ready threads, and keeps a budget
once by searching the list it has built. The queue's `reconcile` (`libs/stride`) then wakes them
one at a time: each wake searches the whole runnable list for the highest id not yet queued, and
each candidate is looked for in the queue. With R budgets waking at once among N runnable, that is
quadratic in R, times N.

Measured by `bench:worst-walk`, every PID in use, each process in a budget of its own holding
`MAX_THREADS` threads, when one deadline wakes 250 budgets at once (rv64, checked build, net of its
audits, 129,796 live threads across 510 processes): one reconcile takes 0.32 s, where its median
over the run is 11 µs. rv32 is not measured: the case runs on rv64 alone.

## Why it matters

Reconcile runs at every kernel entry and is not preemptible, so its time is added to every
system call, interrupt and wake. Its cost follows how many processes exist and how many budgets
wake together, not what the entry did: [R12 (scheduling)](../kernel/scheduling.md#r12-scheduling)
asks for a call's kernel time to follow the objects it names.

## Where

- `kernel/src/sched.rs`: `Runnable::fill` and `reconcile`.
- `libs/stride/src/lib.rs`: `Queue::reconcile`'s wake loop.

## Done when

- A budget is marked when one of its threads becomes or stops being ready, at the process
  table's transitions; reconcile visits only the marked budgets and wakes them in one pass, by
  descending id. `Runnable::fill` goes. A checked build audits the marks.
- The same events come in the same order: the model, the scheduler oracle, the stride
  differential and every R12 mutation are unchanged.
- Reconcile with 250 budgets waking at once takes far less than 0.32 s, on both widths, and the
  scheduling gate's targets do not regress.
- [Scheduling](../kernel/scheduling.md#residual-risks)'s residual and the
  [budgets](../kernel/budgets.md#residual-risks) measured list say what the code then does.
- This page is deleted.
