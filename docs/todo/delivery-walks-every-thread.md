# Delivery walks every thread

## What

A delivery on an endpoint looks for its receivers and senders among every live thread of every
process, not among the threads waiting there. `pump` (`kernel/src/message.rs`) walks the threads
up to three times per message it delivers, through `find_thread`: for the abandoned-call notices,
for the exit notices, and for the pick. For each receiver waiting on the endpoint the pick then
calls `next_sender`, which walks every thread again to find the oldest message under
[R2 (fair waiting)](../kernel/ipc.md#r2-fair-waiting). One delivery is O(T) at least and
O(R x T) at worst, T the live threads and R the receivers waiting on the endpoint. `irq_ready`
walks every thread for a device's waiters the same way, and a destruction's message reach
([R10 (destruction)](../kernel/budgets.md#r10-destruction), step 4) walks every thread too.

The walks follow what exists: only the PIDs with a process and the threads with an IPC page, at
most `MAX_PROCESS_COUNT` x `MAX_THREADS` (511 x 255).

Measured by `bench:worst-walk`, every PID in use, each process in a budget of its own holding
`MAX_THREADS` threads (rv64, checked build, net of its audits, 129,796 live threads across 510
processes):

- one destruction, a process of 255 threads in one budget: R10 11,716,783 µs, against a target
  of 30 ms. 6.9 s of it is one pump, the exit notice to its creator's exit endpoint. The same
  destruction with the system near empty takes 12.9 ms, its pump 109 µs;
- one receive's delivery, a message from a thread of the same process: 6.9 s. The pumps at full
  occupancy take 4.6 to 11.5 s each;
- the timer interrupt whose deadline ends 250 waits at once: its walks for the due waits and the
  pumps of the waits it ends ([expiry walks once per wait](expiry-walks-once-per-wait.md)).

rv32 is not measured: the case runs on rv64 alone, with 4.5 GiB of guest RAM.

## Why it matters

The kernel is not preemptible: every interrupt, wake and timeout on the machine waits for a
delivery. [R12 (scheduling)](../kernel/scheduling.md#r12-scheduling) says a system call's kernel
time never depends on what other processes hold. A walk of every thread is a constant only by the
letter: its cost is everyone's threads, and at the limits it is seconds. R10's 30 ms, and with it
a lease's end, holds only while few threads exist.

## Where

- `kernel/src/message.rs`: `pump`, `next_sender`, `find_thread`, `irq_ready`, and
  `budgets_dying`'s walks of the threads for a dying endpoint's waiters and senders.
- The model (`model/`) keeps receivers per endpoint in arrival order; the kernel finds them by the
  walk.

## Done when

- Each endpoint keeps lists of its own: its receivers in arrival order, R2's groups ordered by
  (due, group) with sends and calls in separate chains
  ([R4a (open calls)](../kernel/ipc.md#r4a-open-calls)), its owed abandoned-call notices and its
  open calls; each device object keeps its IRQ waiters. `pump`, `next_sender`,
  `irq_ready`, R10's message reach and an endpoint's destruction walk those, never every thread.
  A checked build audits each list.
- The model, the scheduler oracle, the ABI and every mutation of R2,
  [R3 (lends and abandoned calls)](../kernel/ipc.md#r3-lends-and-abandoned-calls) and
  [R4 (delivery)](../kernel/ipc.md#r4-delivery) are unchanged.
- One delivery at full occupancy takes the time it takes at the smallest fill, within noise, and
  R10 is within 30 ms net at full occupancy, on both widths: `bench:worst-walk` passes with its
  `must_fail` and `whole_run = false` gone, and runs on rv32.
- [IPC](../kernel/ipc.md#residual-risks)'s residual, the
  [budgets](../kernel/budgets.md#residual-risks) measured list and the
  [timer](../kernel/timer.md#residual-risks)'s say what the code then does.
- This page is deleted.
