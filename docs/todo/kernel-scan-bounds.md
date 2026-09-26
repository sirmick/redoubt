# Scans of every kernel-object frame

## What

Three kernel paths find what they need by scanning every kernel-object frame from the first to
the highest one ever given to an object (`high_frame`):
- `process_create`'s PID draw, which asks for each candidate PID whether a process object still
  names it, one scan per candidate, twice over;
- the search for an exit notice owed on an endpoint, run at each delivery there;
- the search for an interrupt's IRQ object, run on every interrupt, and again to bill it.

`high_frame` is bounded only by RAM, never comes down, and rises whenever any budget creates an
object. So each of these costs grows with what other processes hold, which R12 (scheduling)'s
bound on a call's kernel time forbids. The time is billed (to the caller, or for an interrupt to
the IRQ object's owner), but the kernel runs with interrupts off, so every wake on the machine
waits for it. R10 (destruction)'s own scan is the stated exception and has its own follow-up
([budget destruction cost](budget-destroy-cost.md)).

## Why it matters

Any budget that creates many objects makes every process creation, every notice delivery and
every interrupt slower for everyone, with no authority beyond its own page limit. Interrupt
latency matters most: it is paid on every device interrupt.

## Where

- [`kernel/src/process.rs`](../../kernel/src/process.rs): `find_process`, `object_of`,
  `random_free_pid`, `pending_notice`.
- [`kernel/src/device.rs`](../../kernel/src/device.rs): `irq_device`, and its callers
  `irq_fired` and `bill_irq` in [`kernel/src/sched.rs`](../../kernel/src/sched.rs).
- [`kernel/src/message.rs`](../../kernel/src/message.rs): `pump`, which calls `pending_notice`.
- The pages: [scheduling](../kernel/scheduling.md#residual-risks) and
  [processes](../kernel/processes.md#residual-risks).

## Done when

- The PID draw, the owed-notice search and the IRQ lookup each cost a constant or a term
  linear in what the call names: for example a table from PID to process object, a per-endpoint
  list of owed notices, and a table from interrupt number to IRQ object.
- A bench case creates many objects in one budget and shows that another budget's
  `process_create`, a notice delivery and an interrupt's handling take no longer than with few.
- R12's residual on the scheduling page drops these three scans.
