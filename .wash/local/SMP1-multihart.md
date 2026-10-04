# SMP1-design: one budget on several harts (architect-8)

The owner's goal: a beamlet VM (one process, one budget) runs its schedulers on several harts at
once. beamlet on Redoubt already splits its threads: two run the schedulers, the rest wait on calls
(docs/userland/beamlet.md), so two harts give the VM real parallelism.

## ASIDs: not needed, on any path

K16's ASID 0 with a full flush at every `satp` switch stays. With it, a hart holds translations
only of the process it is running *now*. So the harts that must flush when a mapping leaves
process P are exactly the harts whose current process is P: a set the kernel already knows from
each hart's per-hart block. ASIDs would make that set "every hart that ran P since P's ASID was
last flushed there", which is more state to keep right and to audit. ASIDs come back only as a
measured performance change, never as a correctness need. **K16's commit 3 proceeds.**

A core whose harts share one TLB (the FPGA platform's barrel) changes none of this
(architect-10). The privileged spec requires a translation cache harts share to appear private
to each hart ("the meaning of an ASID is local to a hart"), so the shared TLB tags its entries by
hart, and the fpga page states that as the core's requirement. A hart's `sfence.vma` or `fence.i`
may also flush its sibling's entries, which costs time, never correctness. ASIDs stay open there
as on any platform, as a measured change: the shootdown's set then becomes "every hart that ran P
since P's ASID was last flushed there".

M2's step 4 says "SBI remote fences, by address-space id"; that wording changes to "to the harts
running the process" when the shootdown lands.

## What a multi-hart budget takes

1. **Shootdowns.** Every call that removes a mapping from a process (`unmap`, a lend's end, a
   transfer, `process_map`'s move, a reply returning a lend, any permission narrowing) sends a
   flush request to the *other* harts currently running that process, and waits for their
   acknowledgement, under the lock, before the call returns or the frame is reused. The
   acknowledgement is the eviction's, generalised: taken at trap entry and while spinning for the
   lock, without the lock, so no deadlock. When no other hart runs the process (the common case),
   it costs a check.
2. **Instruction fences.** An executable mapping added to a process running on another hart, and
   a thread moving to another hart, need `fence.i` there (a remote fence, or the per-hart flag).
3. **The pick.** A hart takes the lowest-pass budget that has a runnable thread not running
   elsewhere. Several harts can run one budget's threads.
4. **Charging and the floor.** The stride arithmetic per budget is unchanged (pass += charge /
   weight), but a budget can now have several runners: each hart charges its runner, under the
   lock, so the budget's pass rises as fast as it uses harts, and its share of the machine stays
   its weight's. The floor's "running budget at its last-charged pass" must count a budget with
   several runners once. R12 is restated across harts: a budget gets its weight's share of the
   machine, at most one hart per runnable thread, and no budget gains share by being spread
   (M2's line). The stride crate, the model and the mutations change with it.
5. **The lock.** The big FIFO lock stays. The bound is still N-1 kernel sections however the
   runners are spread, so Q2's targets (gated at 1 and 2 harts) are unaffected. A VM's speed-up
   on two harts is limited by its kernel time; finer locking (step 5) is what lifts that, later.

## Paths

| Path | Packages | Kernel code to audit | Meets the goal |
| --- | --- | --- | --- |
| **A (recommended): many harts by design, built in order.** SMP1 as briefed, with its eviction built as a shootdown to a *set* of harts; SMP3 lets a budget run on several harts (shootdowns at every removal site, the fences, the pick); SMP2 last: R12 and the targets across harts once, with several runners, and a case with the VM's two schedulers on two harts. | SMP1 (L), SMP3 (M), SMP2 (M-L) | SMP1's lock, per-hart block, HSM and IPIs, plus the shootdown (about 150-300 lines more than one-hart-per-budget, my estimate: a running set, the request and acknowledgement, a call at each removal site, the remote fence, the pick) | yes |
| **B: many harts in one package.** The same code, SMP1 L+ (too large for one review), then SMP2. | SMP1 (L+), SMP2 | same as A | yes, one big merge |
| **C: one hart per budget.** The earlier recommendation. Lifting it later adds SMP3 *and* redoes SMP2's R12 work for several runners. | SMP1 (L), SMP2 (M) | least now | no |

Under A, nothing SMP1 builds is removed: its one-runner rule is one line of the pick, stated on
scheduling.md as interim, and its eviction is the shootdown with a set of one. SMP2 waits for SMP3
so R12 across harts is written and modelled once.

## If the owner picks A

- SMP1's brief: rule 6's pick stays (interim); rule 7's eviction becomes "a flush request to a set
  of harts", per-hart flag and acknowledgement, used by destruction; no other change.
- New node SMP3 (needs SMP1): budgets on several harts. Cases: `smp-shootdown` (a process's
  thread on hart 1 writes a page its other thread on hart 0 unmaps or returns as a lend; the
  checker sees no stale write; a recorded negative feature), `smp-fence` (new code mapped while
  another hart runs the process), the whole bench at `--smp 2`.
- SMP2 needs SMP3; its body gains R12 restated across harts with several runners, the model's
  several harts, and the VM-on-two-harts case (once beamlet runs on Redoubt).
- M2's step 4 reworded: shootdowns "to the harts running the process", no ASIDs.
