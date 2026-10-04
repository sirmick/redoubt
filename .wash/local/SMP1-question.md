# SMP1: several harts, started ahead of M2 on the owner's word

The owner (2026-10-03, after GATE1's merge): "start looking at SMP now". The plan's `smp` step
is M2's and needs M1, so this starts on an override with the owner's word; the M1 steps (fsd,
beamlet-redoubt, steward, sshd) continue in parallel. Design the first package and write its
brief (SMP1) at `.wash/local/SMP1-implementer.md`, with the plan node under the `smp` step.

## The plan's order (m2-usable-shell.md "Several harts")

1. any boot hart; 2. per-hart kernel state (trap stack, current process and thread through
`sscratch`, scheduling on every hart); 3. one big kernel lock at trap entry with one global run
queue; 4. cross-hart interrupts, shootdowns and fences; 5. finer locking.

## What the design must settle

- **How far SMP1 goes.** My guess: steps 1-3, user code running on every hart under the big
  lock, with step 4's shootdown before any frame is reused as a precondition now that K21's free
  list makes a stale mapping an integrity hole (your held M2 line).
- **K16 and K21.** Per-thread contexts in the IPC page, 16-bit PIDs, ASID 0 in `satp` (K16); the
  free list and `kernel_frame` from the tail (K21). Does SMP1 need them merged first?
- **K19.** "Pumps at the boundary" (`.wash/local/destroy-simplify.md`): a mark/sever/reap/pump
  destruction has no window a second hart's trap could observe mid-kill. If K19 should precede
  SMP1, say it as SMP1's need and I put K19 to the owner as the gate for SMP.
- **The attack cases.** The existing single-hart cases booted with 2 and 4 harts; a shootdown
  case (unmap on hart A, read through a stale TLB on hart B); a lock-contention latency case;
  R12's "no budget gains share by being spread across harts".
- **The scheduler's rule** under one queue, and **what the targets become** (one hart's numbers
  stay the targets, measured on hart 0 with the others loaded?).
- **The bench**: the `smp=` knob, icount with several harts (determinism), the trace ring with
  several writers.
- **The pages**: scheduling.md, memory-layout.md's `satp` and TLB sections, processes.md, the M2
  page.
- **Owned paths** against K16 and K21; **size and tier** (my guess: two or three packages, SMP1
  Tier A size L).
- **The owner's**: how many harts the targets are stated for; whether M1's remaining steps wait
  on it (my answer: no). Put those to them with your recommendation.
