# architect-7 notes

## Owed page edits

- **At K21's merge**, check that memory.md's residual on the intrusive free list landed. Then
  append this to step 4 of m2-usable-shell.md "Several harts", after "...when a thread moves
  ([memory](...), [memory layout](...))." and before the step's end:
  > A free frame holds the free list's link in its first words, so a stale mapping of it on
  > another hart would let user code rewrite the list and steer the kernel's next write anywhere
  > in RAM. The shootdown before reuse keeps the kernel whole, not only a freed page's data
  > private, so until this step is built and attacked, user code runs on one hart.

  Link "the free list" to memory.md's anchor for K21's residual. Asked for by the orchestrator,
  from K21's red review; no package waits on it.
- **At the `init` step's close (after INIT4 merges)**, edit m1-separation.md:
  - move the "`init` and the boot manifest" item to Progress;
  - fix the "Not built: `init`'s manifest handling" line;
  - change the attack-suite row for R33 ("not yet") to built (`init-refuses-budget-handle`);
  - R31/R32 "from a user parent" stays the steward's.

  Also check that init.md's only planned parts left are the steward's, `sshd`'s and fsd's. See
  INIT4-implementer.md, last section.

## Designs owed

- **FSD2's brief** (quotas, R48).
- **FSD3's brief.** It covers:
  - init mints the range badge from `volumes` and re-mints it at a restart;
  - blkd range labels from init's args (`NO_LABELS` in `servers/blkd/src/server.rs`), needed
    because a confined labelled fsd carries the volume's labels;
  - image/disk.toml packed by mkimage;
  - restart, and fsd.md's Open item (rule: no full volume check at start; mount only, R49/R50);
  - a two-boot persistence case (the bench keeps one disk across two boots);
  - the bench cases under init with `[disk] partitions`.

## Watch at merges

- **K21:**
  - the DMA pool as in K21-dma-pool-ruling.md (devices.md, budgets.md, memory.md lines);
  - `dma-reset-quarantine`'s new verdicts;
  - architect-6's K21 list (handoff).
- **INIT3:** the page lines in INIT3-implementer.md; netd's case is not in it (moved to INIT4).
- **INIT4:** the page lines in INIT4-implementer.md; the fixed slots in qemu.rs; the rig deleted.
