# K21-dma-pool: the ruling (architect-7)

## The design: (a), a fixed DMA pool

- **Size.** `DMA_POOL_PAGES` = 1024 (4 MiB) on both widths. It is taken always, with no boot
  argument and no condition on the device list.
  - Why 1024: the pool holds one device's `MAX_RUNS` (32) runs at 32 pages each. That is more
    than three times the largest run a driver asks for today: 9 pages, for `netd`'s ring
    (1 + 16 × 2048 B) and for `blkd`'s queue (1 + 64 sectors). Today's drivers take under 30
    pages in all, and the cases' probes take 4.
  - 4 MiB is 1/64 of the default 256 MiB guest. It is 1/8 of the three 32 MiB cases
    (`pages-exhaustion`, `touch-beyond-ram`, `lend-untouched-page`), which the whole bench
    confirms.
  - The constant lives in one place that the kernel and the test program both read (for
    example `redoubt_layout`), not as a copy.
- **Whose it is.** The kernel's, like the trace ring.
  - It is taken at boot as one contiguous stretch from the top of RAM, through the same path
    as the kernel's own frames, before `boot_budgets`. It is not part of `root`'s pages.
  - Its frames are `DMA_OWNER`'s in the ownership table for life, free or held. A bitmap of
    1024 bits tells free from held.
  - So `is_dma_frame` and every owner check keep refusing a pool frame, free or not. Pool
    frames are never on the free list.
  - The checked-build audits must accept a free pool frame that is `DMA_OWNER`'s with no run;
    say which audit changed.
- **Charging is unchanged.** A run is charged to the caller's budget as today. R6, the
  Quarantine charging and `dma_migrate_quarantine` stay as built. A driver's DMA is bounded
  by its budget and by the pool. The cost is that a charged run leaves as many general frames
  unused, at most 1024. budgets.md's "the sum of all charges never exceeds the free frames"
  stays true.
- **The search.** First fit of `npages` free bits, linear in 1024, a constant (as
  `map_anon`'s area is). Freeing a run clears its bits.
- **Asking for more.** A request longer than the pool's longest free stretch is
  `OutOfMemory`. There is no cap per run and no new error.
- **Quarantine.** A quarantined run's pages stay out of the pool until reboot.
- **Refused.**
  - (b): a run allocator over the free list has no constant bound without a buddy or
    segregated structure, past size M.
  - One region per DMA slot: it reserves for every flagged slot (8 on QEMU `virt`) whether
    used or not, and it caps a device lower.

## The page lines (exact; written in the commit that makes them true)

**devices.md, `### dma_alloc`.** In the first paragraph, replace
"`OutOfMemory` (the pages, the page tables, or the device's `MAX_RUNS` (32) runs all in use)."
with
> `OutOfMemory` (the device's `MAX_RUNS` (32) runs all in use, the caller's budget, no free run
> of `npages` in the DMA pool, or the page tables).

Then add this paragraph before "Each allocation is a **run**":
> Runs come from the **DMA pool**: `DMA_POOL_PAGES` (1024) contiguous pages, 4 MiB on both
> widths, which the kernel takes from the top of RAM at boot and keeps outside every budget. A
> run is the first free stretch of `npages` in the pool, found by a search linear in the pool's
> 1024 pages, a constant, never in RAM ([R12 (scheduling)](scheduling.md#r12-scheduling)). The
> pool holds one device's `MAX_RUNS` runs at 32 pages each; the largest run a driver asks for
> today is 9 pages. A request longer than the pool's longest free stretch is `OutOfMemory`,
> whatever the caller's budget holds.

**devices.md, Residual risks.** The quarantine bullet's first sentence becomes:
> **Quarantine costs memory for good**, in exactly the hostile case, a device that ignores its
> reset: its runs stay charged to the dead driver's budget, then its parent, and out of the DMA
> pool, until reboot.

**budgets.md.**
- In the boot table's `root` row, replace "every RAM page the kernel did not keep for
  itself, less `root`'s own page" with "every RAM page the kernel did not keep for itself or
  for the DMA pool, less `root`'s own page".
- In "Who pays", replace "`root`'s limit is the RAM frames the kernel did not keep, less
  `root`'s own page" with "`root`'s limit is the RAM frames the kernel did not keep for itself
  or for the DMA pool, less `root`'s own page".

**memory.md, `### A page's life`.**
- The brief's sentence goes before the diagram, as:
  > A RAM frame is taken from a free list and given back to it, so backing a page, a page table
  > or an object searches nothing, however much of RAM is in use; a `dma_alloc` run comes from
  > the DMA pool instead, a fixed 1024 pages ([devices](devices.md#dma_alloc)).
- In the diagram, replace the two `Free --> DMA` and `DMA --> Free` lines with:
  ```
      state "DMA pool" as Pool
      [*] --> Pool: taken at boot
      Pool --> DMA: dma_alloc (zeroed first)
      DMA --> Pool: its process ended and every<br/>device that could hold it was reset
  ```
- The caption becomes:
  > *Figure: the states of a RAM frame. A frame is mapped by at most one process at a time, and
  > is zeroed whenever it leaves Free or the DMA pool. A pool frame is never Free, and a Free
  > frame is never DMA-held.*

The scheduling.md line in the brief is unchanged.

## `dma-reset-quarantine`'s new shape (file granted)

Granted: `tests/programs/src/bin/dma-reset-quarantine.rs` and `tests/dma-reset-quarantine.toml`.

- The part of the case up to and including the destruction of the drivers stays as it is.
- The search becomes a search of the pool.
  - There is one search, through the empty slots left. It runs in a budget whose free pages
    exceed `DMA_POOL_PAGES` plus its tables, so every refusal is the pool's.
  - Destroying `users` and the checker that takes all of `root`'s free pages go, unless the
    budget's room needs them.
  - It halves the chunk from the largest power of two at or below the pool, as now.
- The verdicts:
  1. the pages allocated plus the two quarantined runs (2 × `RUN_PAGES`) equal
     `DMA_POOL_PAGES`;
  2. no run overlaps a quarantined one;
  3. the last `dma_alloc(1)` is refused `OutOfMemory` while the budget still had room.
- The header comment says so. Reduce `WAIT` if the shorter search allows.

Any other case whose arithmetic counts `root`'s RAM and turns red comes back as a message
before it is touched.
