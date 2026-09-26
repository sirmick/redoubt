# Scans of every kernel-object frame

## What

Three kernel paths find what they need by scanning every kernel-object frame from the first to
`high_frame`, the high-water mark of frames ever given to a kernel object:
- `irq_device`, which finds an interrupt's IRQ object, on every interrupt (and again to bill
  it);
- `pending_notice`, which finds an exit notice owed on an endpoint, each time `receive` is
  matched there;
- the PID draw in `process_create`, which asks `object_of` for each candidate PID whether a
  process object still names it: about 2 × 62 × `high_frame` probes per call.

Any budget raises `high_frame` by creating objects, up to its page limit, and it never comes
down. So each scan grows with what other principals hold, and it runs with interrupts off. The
bound is RAM-sized, not a machine constant: at 1 GiB of RAM it is 262144 probes per interrupt.

R12 (scheduling)'s sentence on a call's kernel time forbids this: "A term linear in a fixed
kernel constant (`MAX_PROCESS_COUNT`, the platform's interrupt count, `MAX_DMA_DEVICES`, a fixed
table size) is a constant. A term linear in RAM frames or kernel-object frames is not.
R10 (destruction)'s sweeps are the one stated exception." Every scan to `high_frame` outside R10
is a departure ([budget destruction cost](budget-destroy-cost.md) covers R10's own).

## Why it matters

A budget that fills its page limit with objects makes every interrupt, every notice delivery and
every process creation on the machine slower, with no authority beyond its own pages. Interrupt
latency is paid on every device interrupt, by every driver.

Fixed in the kernel follow-up package after the documentation rewrite, before the work on
`init` and the manifest.

## Where

- [`kernel/src/process.rs`](../../kernel/src/process.rs): `find_process`, `object_of`,
  `random_free_pid`, `pending_notice`.
- [`kernel/src/device.rs`](../../kernel/src/device.rs): `irq_device`, called from `irq_fired`
  and from `bill_irq` in [`kernel/src/sched.rs`](../../kernel/src/sched.rs).
- The pages: [scheduling](../kernel/scheduling.md#r12-scheduling),
  [processes](../kernel/processes.md#residual-risks) and
  [devices](../kernel/devices.md#residual-risks).

## Done when

- Two indexes, kept at create and at free, replace the scans:
  - a PID-to-process-object table, `[Option<frame>; MAX_PROCESS_COUNT]`, so `object_of` is one
    lookup, the PID draw at most 63, and `pending_notice` looks at no more than those 63 objects;
  - an interrupt-to-IRQ-object table sized from the platform's interrupt count, set when the
    boot makes device objects and cleared when one is destroyed, so `irq_device` is one lookup.
- A host or model property checks that each index gives the old scan's answer after every
  create and free.
- A bench case, `scan-bounds`, has one budget fill its page limit with endpoints; then
  `process_create`'s latency, an exit notice's delivery and an interrupt's round trip stay
  within the latency target and within a fixed ratio of an empty system's.
- A planted mutation that skips an index update fails the property.
- R12's residual on the scheduling page drops these three scans.
