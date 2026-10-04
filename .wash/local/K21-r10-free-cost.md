# K21: the free list's share of R10 (kernel-containment, seed 13, checked build)

Command: `TESTBENCH_QEMU_SEED=13 .wash/local/in-dev cargo testbench kernel-containment` (both widths).
Instrumentation (scratch, not committed): `.wash/local/K21-free-instrumentation.patch`. It does
`now_ticks` around `push_free` and sums the ticks and calls. `budget.rs` resets them at R10_BEGIN
and prints `K21-FREE top r10_us calls free_us` after R10_END.

## (c) Before: tip 155a38c54, three writes a free

| | rv64 | rv32 |
| --- | --- | --- |
| frees a destruction | 4274 | 4270 |
| push_free time, slowest destructions | 6633 µs | 7583 µs |
| per free (one timer read included) | 1.55 µs | 1.78 µs |
| R10 p99 (instrumented) | 31093 | 32594 |
| R10 p99 (plain, whole bench) | 30081 | 30617 |

The ~8 ms excess over main is mostly the free list: about 6.1 ms (rv64) and 6.6 ms (rv32), net of
the timer reads. Every destruction in the case frees about the same 4270 frames.

## (a) After: the head's PREV word means nothing, two writes a free

| | rv64 | rv32 |
| --- | --- | --- |
| push_free time a destruction (instrumented) | ~4786 µs | ~5533 µs |
| per free | 1.12 µs | 1.30 µs |
| R10 p50 / p99, plain (exit 0, PASS) | 24552 / 28286 | 24756 / 28583 |
| main (K18/GATE1 logs, same case) | ~18700 / ~22370 | ~18650 / ~22170 |

Dropping one of the three writes saved ~0.43 µs a free, so each `kframe::write` costs ~0.4–0.5 µs
here. The cost goes with the write count, not with touching a page: the dropped write was to the
same page as the one kept. The gate passes with 1.4–1.7 ms of margin, where main has ~7.6 ms. The p50
is ~6 ms above main's too. The handoff's "p50 unchanged" does not hold.

## Fallbacks, for the Architect

1. A kframe helper taking the physmap address once a frame. It only saves `at`'s asserts, so it is
   probably small: the writes themselves seem to be the cost.
2. A destruction splices its freed frames as one chain: 1 write a frame (NEXT) plus O(1). PREV
   would then be filled lazily, or the claim-by-address unlink needs another way to find its
   neighbour.
3. A singly linked list: 1 write a free. But unlinking a frame where it lies, for a claim by
   address, becomes a walk, against R12, unless claims stop needing it.
4. A bitmap, two-level for a bounded search: a free is one bit and touches no frame, and a take
   scans words. It costs RAM/8 bits of memory and a search bounded by RAM/64/64.

## State

wp-k21 tip d0af5ff50 (47e6345fe free list amended, 394f2c94c, a30b60fae, d0af5ff50). fmt is per
file, `rustfmt +nightly`. doccheck exit 0. size-budget PASS at commit 1 (7915/7915) and at the tip
(7955/7955). Not yet run on the new tip: scan-bounds, sched-budget-churn, dma*, the whole bench.

A stray stable `cargo fmt` reformatted the tree. It was reverted at once with `git checkout` on
exactly those paths, and nothing of it was committed.

## Round 2 (after QA K21-free-cost)

### Cases on d0af5ff50, both widths, all exit 0
- scan-bounds: PASS on rv64 and rv32.
- sched-budget-churn: PASS. Shell share 490 (rv64), 480 (rv32), under the 550 ceiling, so no
  churn-ceiling change is needed.
- dma-destroy-quarantine, dma-reset-quarantine, dma-reset-reuse, dma-rules: all PASS.

### Main baseline, the same host and seed 13 (5f9f9d61c, detached, then back to wp-k21)
| R10 p50/p99 µs | main | d0af5ff50 | excess |
| --- | --- | --- | --- |
| rv64 | 18648 / 22383 | 24552 / 28286 | +5.9 ms |
| rv32 | 18713 / 22524 | 24756 / 28583 | +6.1 ms |
The free list's measured share is 4.8 / 5.5 ms of that excess. The rest is set_owner's work around it
and the unlinks a destruction makes.

### (4) A bitmap: its shape and cost
- **Shape.** One bit a RAM frame, set when the frame is free, plus summary levels: a bit is set when
  the word below it is not zero. With 64 bits a word, L levels cover 64^L frames.
  - rv32: at most ~2 GiB of RAM, 520K frames, so L = 4.
  - rv64: the physmap allows 128 GiB, 32M frames, so L = 5. Fix L at the build's maximum.
- **A free.** Set the bit. Only when that word was zero, set the bit above it, and so on up. That is
  1 read-modify-write, and rarely more.
- **A take.** Descend L words by trailing_zeros, which gives the lowest free frame (deterministic,
  like first fit). Clear the bit, and while the word becomes zero clear the bit above it. That is L
  reads and 1 to L writes, a constant.
- **kernel_frame.** Descend by leading_zeros to the highest free frame: about 8 lines.
- **A claim by address.** Clear the frame's bit and propagate up, with no search.
- **Storage.** frames/8 bytes: 64 KiB for rv32 at its maximum, 8 KiB for 256 MiB of RAM, 4 MiB for
  rv64 at its maximum. That is too big for a static on rv64, so take the frames at boot from the top
  of RAM, owned by KERNEL_PID, as the trace ring is.
- **Cost per free depends on how the bitmap is reached.** Measured here, a kframe write costs ~0.43
  µs (QEMU without icount, so these times are host wall time).
  - Through kframe (no new unsafe): a read and a write, ~0.5-0.9 µs a free, or 2-4 ms a
    destruction. That is p99 ~24-26 ms: better than (a), still not main's margin.
  - As a `&'static mut [u64]` built once at boot (one new documented unsafe, the same pattern as
    `allocations`): ~10 instructions, about main's byte store. R10 near main's ~22.4 ms. The unsafe
    budget would rise by 1 with a stated reason.
- **Audit.** For each frame, its bit == `allocations[i].is_none()`, and each summary bit == (its word
  below != 0). Plus the count, and the DMA pool check as now. Linear, in the stamped audit only.
- **Integrity.** No kernel link lives in a free frame, so the stale-mapping hazard goes away. Its
  bullet in memory.md's Residual risks and the free-frame item in kframe.rs's list go too.
- **Lines.** An estimate of 75-95 counted lines (init, set/clear with propagation, descend twice,
  audit) in place of the list's ~60, so ceiling 7915 rises to ~7930-7950 at the first commit.
- **Order.** The lowest free frame first, where the list gives LIFO. scan-bounds' bound is unaffected
  (a constant take). Reused frames are no longer the most recently freed, which matters to nothing
  measured.

## Round 3: the bitmap (4) as ruled, on main 082e00ccf

Branch wp-k21 is at b0fcee1f2:
- 5dde8d361: the bitmap, replacing the free-list commit.
- ad80ccf96, 05f68a306, b0fcee1f2: the rollback walk, scan-bounds and the DMA pool, replayed.

The bitmap's words are reached through kframe, so no new unsafe. Size ceilings: 7949 / 7949 / 7954
/ 7989. doccheck exit 0. The (a) version on 082e00ccf is 25c643f0c, unreferenced but still in the
reflog.

| R10 p50/p99 µs, seed 13 | rv64 | rv32 |
| --- | --- | --- |
| main 5f9f9d61c (this host) | 18648 / 22383 | 18713 / 22524 |
| (a) list, 2 writes a free | 24552 / 28286 | 24756 / 28583 |
| (4) bitmap through kframe | 24562 / 28298 | 25253 / 29029 |

Free cost a destruction (4273 frees, scratch stamps, one timer read included):

| | rv64 | rv32 |
| --- | --- | --- |
| list, 3 writes | 6633 µs (1.55 µs a free) | 7583 µs (1.78) |
| list, 2 writes (a) | 4786 (1.12) | 5533 (1.30) |
| bitmap, kframe read + write | 6298 (1.47) | 7736 (1.81) |
| bitmap, raw physmap volatile (scratch unsafe) | ~4990 (1.17) | ~6280 (1.47) |

The raw run is .wash/local/K21-bitmap-raw-instr.patch. A timer-read pair costs ~0.3 µs.

**Reading.** What costs is touching memory outside the ownership table on each free, about 1 µs a
free in these destructions:
- It is not kframe's asserts: raw access saves only ~0.3 µs.
- It is not frame writes: the bitmap writes none, and costs as much as the list.
- A bitmap free is 1 read and 1 write, the same order as the list's 2 writes.

My guess, not measured: the destruction does an sfence.vma for each page it unmaps, and every
physmap access after a flush pays a QEMU softmmu TLB refill. Main's free touches only the table,
whose mapping may survive the flushes. The 1 µs is a TCG artefact either way, but R10 is judged on
it.

What might reach main's margin, untested:
1. Defer the frees of process frames to after the walk, as `free_deferred_frames` already does for
   object frames. They would run with no flushes in between, so the bitmap words stay
   TLB-resident. Cost: a per-destruction list or a second walk.
2. Keep level 0 next to the ownership table, in the same mapping (for example in the loader-built
   region). That is a loader change.
3. Profile first: which frees, process or object, dominate the 4273, and whether the flushes are
   the cause.

The bitmap commit's message says the bitmap fixes R10. It does not, and the message must change
with whatever is decided.

## Round 4: profile, table, final shape

kernel-containment runs under `-icount shift=3`: guest time is 8 ns a guest instruction and runs
repeat exactly. So every cost here is an instruction count, not TLB or memory behaviour.

### Profile (scratch stamps, rv64 / rv32, a destruction)
- Frees: 4273 / 4269. Only ~100 / 96 of them fall inside the process walk
  (`release_owned_frames`); the rest are object frames freed in the destruction.
- Process walk: 2586 / 2034 µs on the tip, against 2491 / 1990 on main.
- Deferred object frees: ~0 µs on both.
- Bitmap `mark_free`, stamped: 3385 / 4790 µs. That matches the R10 excess.
- With no `sfence.vma` inside R10 (scratch flag), the numbers are identical: the flush is not the
  cost.

The cost was `mark_free`'s code: `word`/`set_word` per level, with their indexing. A lean loop over
`level_start` with `&mut bits[..]` cuts ~2.6 ms. Deferral (1) is not needed.

Patches: K21-prof-tip.patch, K21-prof-noflush.patch.

### Table: uninstrumented R10 p50/p99 µs, seed 13, the lean mark_free

| build | rv64 | rv32 |
| --- | --- | --- |
| main 5f9f9d61c | 18648 / 22383 | 18713 / 22524 |
| slice, per-page flush (tip) | 19194 / 22923 | 19857 / 23511 |
| slice, no flush in R10 | 19195 / 22924 | 19869 / 23661 |
| kframe, per-page flush | 24492 / 28220 | 25229 / 28987 |
| kframe, no flush in R10 | 24492 / 28228 | 25220 / 29044 |

By the Architect's rule, the slice stands: kframe without flushes is +5.8 / +6.5 ms over main's p99.
The flush stays per page, unchanged.

### Final tip
wp-k21 is at 0f1590e05:
- 2dc04ae37: the bitmap (slice, lean mark_free), its message reworded.
- 6d4f2cccb: the rollback walk.
- 1120d6d25: scan-bounds, with the negative re-recorded.
- 0f1590e05: the DMA pool.

Gate PASS on both widths: 19194/22923 and 19857/23511. Focused cases on the tip all exit 0: scan-bounds,
sched-budget-churn (shell 500 / 487), dma-destroy-quarantine, dma-reset-quarantine,
dma-reset-reuse, dma-rules, page-table-reclaim. size-budget PASS (7983/7983).

Negative re-record (checked build, forbid PANIC only), a one-page map_anon:
- `alloc-first-fit`: 1174 vs 374 µs on rv64, 1274 vs 474 µs on rv32, both FAIL.
- Without the feature: 354/354 and 453/453.

**unsafe-budget FAILS: a checker bug.** `size.rs::reasons` splits the line at its first `": "`.
So a budget whose name contains `": "` ("kernel: core") can never be matched, and the line
`Unsafe budget: kernel: core: <reason>` is read as the budget "kernel". No earlier commit raised a
budget with a colon in its name. Fix: match the line by prefix `Unsafe budget: {name}: ` for each
changed budget. That is a tools/testbench change with a host test. It is not done; it needs a
decision.
