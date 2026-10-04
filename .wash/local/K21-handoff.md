# K21 handoff (k21-implementer -> k21-implementer-2), 2026-10-02

## State
Branch `wp-k21` in `/home/mcloonan/redoubt/.worktrees/k21`, base main 81b5ea38b, tip **e49d1dbbf**.
The tree is clean (the scratch scan instrumentation lives only in `.wash/local/K21-scan-instrumentation.patch`).
Every deliverable is built and committed. What is owed: **the whole bench**, run once with
`cargo testbench --allow-skip` (both widths in one run) **only on the orchestrator's word**, since
GATE1's merge-gate bench had the host. Then the final report.

Commits, one per deliverable:
1. `125b5c016` kernel: a RAM frame is taken from a free list, never found by a search of RAM
2. `81f8ecaab` kernel: a refused process_create gives back what it built by one walk of it
3. `0ca182093` testbench: scan-bounds times taking frames after RAM fills, on both widths
4. `e49d1dbbf` kernel: dma_alloc's runs come from a fixed DMA pool, never from a search of RAM

The full detail (every table writer, what was deleted, files outside the brief, residuals) is in
`/home/mcloonan/redoubt/.worktrees/k21/.wash/local/K21-report.md`. The consoles and logs are beside it.

## The free list (mem.rs, `FreeList`, designed against K16)
- **Representation:** an intrusive, doubly linked list through the free frames, via `kframe` and
  the physmap.
  - Word 0 (`FreeList::NEXT`) holds the next free frame's index + 1; word 1 (byte 8,
    `FreeList::PREV`) holds the previous one's. 0 means none.
  - `MemoryManager.free = FreeList { head, tail, len }`, each a `u32`, index + 1. All zeros is the
    empty list, so it stays compatible with K16's all-zero `.bss` MemoryManager (`LoaderTable`).
  - It is independent of the Pid width in the table: K16 widening Pid touches only
    `allocations`' element type.
- **One writer:** `set_owner(index, owner)` is the only writer of `allocations`.
  - None -> Some unlinks the frame (`unlink_free`, O(1), anywhere in the list: a claim by address
    needs that).
  - Some -> None pushes it at the head (`push_free`).
- **Taking frames:**
  - `alloc_frame` pops the head.
  - `kernel_frame` (the sched-trace ring) takes the **tail**: the highest free frame, under the
    DMA pool. This is GATE1's "ring on top", kept.
- **Clearing:** a frame leaving the list has both link words zeroed in `unlink_free`, at the moment
  it is taken. Consumers still zero frames as before (R11); free frames held arbitrary data before
  this package anyway.
- **Boot:** `init_from_memory` builds the list with one table scan, pushing in descending order so
  the lowest frame is at the head. Then `take_dma_pool` runs.
- **Trap:** a frame given back is written to at once. So:
  - `for_each_owned_frame` now reports each table after everything under it, root last
    (arch/riscv/mem.rs).
  - `free_empty_tables` clears the parent slot and flushes the TLB before freeing the table.
  - Getting this wrong caused an I1 panic in check_frame_owners: the walk read a freed table's
    links as entries.
- **Audit:** `check_free_list` (checked build) runs inside `check_object_indexes`, the stamped
  destruction audit.
  - It walks the list (cycle bound, every entry None, prev links right, tail right) and checks
    count == None entries == len.
  - It also checks that every pool frame is `DMA_OWNER`'s.
  - Cost: the destruction audit went from 14.8 to ~70 ms (rv32 churn). It is stamped, so it is out
    of every share and window.

## The rollback walk (commit 2)
- `MemoryMapping::allocate` is whole-or-nothing:
  - `satp` is set right after the root.
  - `add_context_page` frees its own frame if the mapping fails.
  - On any failure, `mm.release_owned_frames(pid, self)` walks the partial space, then `satp = 0`.
- `release_owned_frames(pid, space)` is now the one safe table walk, serving both cases:
  - `Process::terminate` (ptable.rs).
  - `drop_unstarted` (process.rs), which walks `ss.get_process(child)`'s mapping if the slot holds
    one.
- The unsafe `release_all_memory_for_process` is deleted. The kernel core's unsafe count went 19 -> 17,
  and the ceiling in unsafe-budget.toml was lowered to match.

## The DMA pool (commit 4; the ruling is `.wash/local/K21-dma-pool-ruling.md`): DONE
- **Size:** `redoubt_layout::DMA_POOL_PAGES = 1024`.
- **Boot:** `take_dma_pool` takes the highest stretch of 1024 free frames (a boot scan), owner
  `DMA_OWNER`, before boot_budgets. `boot_budgets`' `kept` adds the pool.
- **Bitmap:** `DmaPool { first, held: [u64; 16] }`.
  - `dma_pool_take(npages)`: first fit over the bits, then zero the run.
  - `dma_pool_give`: clears the bits.
- **Callers:** dma.rs calls these. `alloc_contiguous`/`free_contiguous` are deleted.
- **Tests:** dma-reset-quarantine is rewritten per the ruling (granted files).
- **Pages:** the page lines are in devices.md, budgets.md and memory.md exactly as the ruling gives
  them. memory.md's free-list sentence moved from "Backing and zeroing" (commit 1) to "A page's
  life".
- **Residual:** the model's dma_alloc doesn't model the pool's bound.

## scan-bounds and the negative (commit 3)
- The case now runs on both widths. It adds:
  - a one-page map_anon;
  - a budget_create;
  - a rolled-back process_create (a 2-page budget). It must be refused, with usage 0 after.
- It holds a 1024-page `map_anon` "plug" after the filler exits. Without it, first fit reuses the
  filler's freed frames, and the negative passed.
- Negative (`alloc-first-fit`, a test-only feature, so a checked build):
  - Run by temporarily adding `debug_assertions = true`,
    `kernel_features = ["alloc-first-fit"]` and `forbid = ['PANIC']` to the toml.
  - map_anon FAILs on both widths: 1216 vs 376 µs (rv64) and 1316 vs 476 µs (rv32).
  - The same checked build without the feature passes it (354/354, 453/453). In checked builds
    "process_create to notice" fails either way, because of the audits.
  - This is recorded in scheduling.md.

## Page lines
- **Done:** scheduling.md R12, the brief's line plus the negative in the features list; memory.md;
  devices.md; budgets.md.
- **Owed:** none.

## Measurements (seed 3)
- **Main:**
  - rv32 churn shell share: 499 (ring 8192) -> 559 FAIL (16384).
  - alloc_frame mean scan: 8806 -> 16997 slots.
- **Tip churn shell share:** 497/493 (8192, rv32/rv64) and 492/502 (16384).
- **sched-latency and sched-latency-tcg:** PASS on both widths, all targets met.
- **scan-bounds (release):** PASS on both widths; filled within 4 µs of empty.
- **Also passing:** dma*, device*, proc*, pages-exhaustion, touch-beyond-ram,
  lend-untouched-page, unsafe-budget, size-budget (kernel 7980, layout 61), fmt (nightly, via
  in-dev), doccheck.

## What to do first
1. Wait for the orchestrator's word, then `/home/mcloonan/redoubt/.wash/local/in-dev cargo testbench --allow-skip`
   once, with output to a file, reading it through grep. Expect one SKIP: bench-ssh-loopback-openssh.
2. If another case that counts root's RAM turns red (the pool takes 4 MiB): **message the
   orchestrator first, no fix** (the ruling says so).
3. The final report goes through member_update, under 1900 bytes, built from K21-report.md, with the
   tip.

## Rebase notes
- K16 (`wp-k16`) changes `allocations` to `LoaderTable` and later widens Pid.
  - My code indexes `self.allocations[...]` and calls `.len()`/`.iter()`, which work through
    `Deref`.
  - `take_dma_pool` and the list build loop use `self.allocations.len()`.
  - Expect conflicts only in `init_from_memory` and the struct literal.
- GATE1 also edits `kernel_frame`. Keep my version: it takes from the tail.

## What consumed context
- Many full testbench outputs. Grep them instead.
- Reading mem.rs and process.rs ranges, and the dma-reset-quarantine test in full.
- Splitting the work into four commits by hand-crafting intermediate trees.

## ADDENDUM 2026-10-03: the open failure (read this first)

**Current commits** (rebased onto main 5f9f9d61c; fix round 1 is folded in; the SHAs above are stale):
- a357783de: the free list
- 44aabf1b3: the rollback walk
- 6d05871b9: scan-bounds
- 155a38c54: the DMA pool (tip)

**Changes made at the rebase, already in the tip:**
- INIT1 runs the first test program in `root`, which keeps only init's share of pages. So:
  - scan-bounds' filler now reports its count by message on a minted send handle and parks.
    There is no plug any more.
  - dma-reset-quarantine destroys `users` before the pool search, so root has room for the pool.
- The re-recorded negative (alloc-first-fit, checked build): map_anon 1177 vs 376 us on rv64 and
  1276 vs 476 us on rv32. The same checked build without it: 354/354 and 454/454.
- Size ceilings: kernel 7915 / 7915 / 7920 / 7955 after each commit (main is 7828 under 7863).

**Whole bench on 155a38c54:** 289 PASS, 1 SKIP (bench-ssh-loopback-openssh), **2 FAIL**. Both are
`kernel-containment` (qemu_seed 13, checked build, sched-trace), on R10's p99 rule of 30000 us:

| R10 p99 | rv64 | rv32 |
| --- | --- | --- |
| main 5f9f9d61c (alone) | 22383 | 22524 |
| tip 155a38c54 | 30081 | 30617 |

p50 is unchanged (~18.7 ms). The slowest of the 18 destructions grows by ~8 ms.

**The ruling (orchestrator):** R10's 30 ms is a rule, not a target to absorb. The fix is a cheaper
free. Do (c) then (a), and amend the free-list commit.

**Reading (not yet measured):** each freed frame now does the free list's work, where it used to
do one byte store into the ownership table.
- That work is `set_owner` -> `push_free`, which is three `kframe::write`s: the new frame's
  NEXT and PREV words (`set_free_links`), and the old head's PREV.
- Each write goes through `kframe::at`'s asserts, and the checked build adds overflow checks.
- Destructions in kernel-containment free thousands of frames: object frames through
  `free_deferred_frames`, and process frames through `release_owned_frames`.
- So the cost is still linear in what is destroyed, but with a larger constant.
- Unlinks happen on allocation, not in R10, except where a destruction allocates.

**(c) Measure first.** Use a scratch instrumentation and don't commit it; follow the pattern of
`.worktrees/k21/.wash/local/K21-scan-instrumentation.patch`.
- Time the free-list part of a free. Wrap `push_free`, or `set_owner`'s list arm, with
  `crate::arch::irq::timer::now_ticks()` before and after, and add the difference to a static
  AtomicUsize or u64 under `#[cfg(feature = "sched-trace")]`. Count the calls in another static.
- Report it per destruction: in `budget.rs`, at the `R10_END` record (line ~1222), take the
  accumulated ticks and calls and reset them.
- Get the numbers out of the trace in one of two ways:
  - `record` them as a scratch record kind, e.g. `b'F'`, with id = calls and the ticks in the
    pass field. `/tmp/churn.py`'s parser then reads them.
  - Or `println!("K21-FREE {} {}", calls, us)` at R10_END. That is simpler, but it prints inside
    the destruction.
- Convert ticks to us with `crate::arch::irq::timer::ticks_to_us`.
- Run `TESTBENCH_QEMU_SEED=13 .wash/local/in-dev cargo testbench kernel-containment` (both
  widths, ~2 min each). Compare the free-list share of the slowest destruction with its ~8 ms
  excess.

**(a) A cheaper free:** make the head's PREV word meaningless, since a frame is the head exactly
when `free.head == index + 1`.
- `push_free`: write only the new frame's NEXT. Skip its PREV, because a head needs none. The old
  head still gets its PREV set to the new frame, since it stops being the head. That is 2 writes,
  down from 3.
- `unlink_free`:
  - `prev = if self.free.head == index as u32 + 1 { None } else { free_link(index, PREV) }`.
  - When the head is unlinked, the next frame becomes the head: skip writing its PREV.
  - On allocation, still zero both words of the frame taken, as the pages promise ("both are zeroed
    when it is taken").
  - Optionally zero only the words that are set. A head's PREV may be stale but is never read.
- `check_free_list`: skip the PREV check for the head.
- Other cuts if (a) is not enough:
  - Fold the two writes per frame into one u64? No: NEXT and PREV are separate words.
  - Avoid `kframe::at`'s repeated asserts by computing the physmap address once per frame. That
    needs a kframe helper writing two words (no new unsafe: put it inside kframe's existing
    block).
  - Or have the destruction batch its frees: chain the freed frames locally and splice them onto
    the list once. That costs 1 write per frame (NEXT) plus O(1), but needs PREV for later
    unlinks... PREV could be filled lazily. Report this before doing it.

**Then:**
1. Amend the free-list commit a357783de: rebuild by detached checkout and cherry-pick, as the
   earlier rounds did. Never stash.
2. Re-run kernel-containment at seed 13 on both widths, plus sched-budget-churn, scan-bounds and
   dma*.
3. Run fmt and doccheck.
4. Run the whole bench alone only on the orchestrator's word.
5. Report the tip.
