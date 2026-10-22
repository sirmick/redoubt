# SCHED5 on main 4c2c063e8: beamlet-heap-allocation rv64 (sched5-implementer-3, 2026-10-22)

## Result

A SCHED5 regression. It is the cost of the runtime's own placement of long runs
(`redoubt_rt::handle::map_anon`, `place`), which probes each gap by mapping it.

**Rates (rv64, alone):**
- Main 4c2c063e8: PASS 15.1 s and 15.1 s.
- SCHED5 c7ccb88ae: PASS 47.1 s, then FAIL 63.6 s twice. Each failure is step 4's 60 s bound,
  waiting for `{:built, 65534}` or `:killed`.
- rv32 passed at c7ccb88ae, 23.4 s.

Logs: `.tmp/SCHED5/main-rerun/` (the set's failure), and `.tmp/SCHED5/bha/` (the counter run).

## Measured (scratch kernel at c7ccb88ae, per-process range-call counters, instr.py)

The session VM (pid 170), at the last counter dump before the timeout:

| call | calls | kernel time | refused |
| --- | --- | --- | --- |
| map_fixed | 190,637 | 24.3 s | 19,416 |
| unmap | 196,212 | 17.2 s | |
| map_anon | 26,616 | 3.9 s | |

That is about 45 s of kernel time in range calls, against a case that takes 15 s in all on main.

## Why

Step 4 builds a 65,534-byte binary by a comprehension. The buffer grows by doubling (B105's
`growths`: 2x, then 1.5x, 1.25x, 1.125x when refused), and each old block is freed (`unmap`).
So the heap area fills with gaps, each shorter than the next request, which is the shape a
growing buffer leaves.

**Before SCHED5:** `map_anon(n)` placed the run in one kernel search. It skipped every short gap
by reading the page tables (`first_occupied_page`), and mapped nothing until it found an n-page
gap.

**Under the cap:** the runtime's `place` asks the kernel for a 16-page first range, which lands in
the first gap of 16 or more pages, and extends it with `map_fixed` a range at a time until a page
is taken. A gap too short for the run is so found only by mapping it:
- every page up to the taken one is allocated and zeroed;
- it is then kept as the filler and unmapped after the next probe.

Each short gap costs pages allocated, zeroed and freed in proportion to its length, where the
kernel's search read page-table entries. The growth's retries multiply it: B105 asks again at
1.5x, 1.25x and 1.125x, each a fresh search over the same gaps.

## Options (the first two change the design; not mine to pick)

1. **Kernel: `map_anon` places a run it does not wholly map.** An argument, the run's length:
   the kernel searches for a free run of that length, as `map_anon(n)` did before the cap, and maps
   only the first `MAX_RANGE_PAGES`. The runtime extends with `map_fixed`, under the process's
   placement lock, so the extension cannot meet a taken page.
   - Restores the base cost exactly: one search, n pages mapped once, no probing and no filler.
   - The section stays bounded: the search is R12's named cost, already stated on R22, and the
     mapping is at most a range.
   - An ABI change: `map_anon`'s row, the model, the fake, `libs/sys`. Recommended.
2. **Runtime: its own record of the placement area.** A bitmap of the map_anon area's pages
   (65,536 pages, 8 KiB) that every runtime `map_anon`, `unmap` and `Process::map` keeps. The
   runtime then finds a long run's gap itself and maps it by `map_fixed`, with no kernel search.
   - No ABI change.
   - It is not authoritative (device maps, `dma_alloc`), so a refused `map_fixed` still falls
     back to probing.
   - More runtime state and lines.
3. **Cheaper probes in the runtime.** For example, try a run's last range first, so a short gap
   is refused before its middle is mapped.
   - Each probe still costs a 16-page `map_anon` (allocated and zeroed), and the search then
     walks the short gap 16 pages at a time.
   - It reduces the cost, without bounding it. Not recommended alone.

**Witness for any fix:** beamlet-heap-allocation rv64 x5 alone near main's 15 s, and its rv32;
the rt host tests for the placement; and the sets already run.
