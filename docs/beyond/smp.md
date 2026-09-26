# SMP

## Idea

The kernel runs user code on every hart, not only the boot hart. The FPGA platform's 32 hardware
threads make it matter ([the FPGA platform](fpga-platform.md)).

What exists: a two-hart spike, in which a second hart started through SBI's hart management
contends with the first on the kernel lock without losing updates (`bench:smp-spike`, a checked
build), and a few cases booted with two or four harts, where the extra harts stay parked.

## Why it is not a goal

One hart is enough for every milestone, and a second one changes every rule that assumes a single
running thread in the kernel: completions, TLB flushes, instruction fences, the scheduler's
queue. Doing that before the rules are attacked on one hart would multiply what can go wrong while
nothing needs the speed.

## What it would need

- **Any boot hart.** The firmware may choose any hart; nothing assumes hart 0 or that the boot hart
  owns the interrupt context it happens to use ([boot-hart context](../todo/boot-hart-context.md)).
- **Per-hart kernel state**: a trap stack and the current process and thread per hart, reached
  through `sscratch`; scheduling on every hart.
- **One big kernel lock** taken at trap entry, and one global run queue, before anything finer
  ([scheduling](../kernel/scheduling.md)).
- **Cross-hart fences and flushes.** Inter-processor interrupts to reschedule, and a TLB shootdown
  (SBI remote fences, by address-space id) on unmap, lend and return before a page is reused; an
  instruction fence on every hart when a page becomes executable, and when a thread moves
  ([memory](../kernel/memory.md#residual-risks),
  [memory layout](../kernel/memory-layout.md#residual-risks)).
- **Locking** for the per-process thread-context pages, and finer locking only once the above is
  stable and attacked.
- **One budget per core,** and `keyd` on a core of its own, where the platform allows.

**Attack cases:** a page unmapped, lent or returned on one hart and still reachable through another
hart's TLB; a page made executable on one hart and run stale on another; completion races between
harts on one call, which today are argued, not attacked
([kernel attack gaps](../todo/kernel-attack-gaps.md)); every scheduling case rerun with several
harts, to show no budget gains share by being spread across them.
