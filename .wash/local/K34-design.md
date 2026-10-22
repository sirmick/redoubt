# K34 design note: map_anon's placement search bounded by a per-process free-run summary

Base: main 2b60945b6 (SCHED5). Branch wp-K34. Nothing built yet.

## The choice: (a), the summary. Not (b).

(b) walks the tables once and iterates the entries. That removes the walk each start costs, but a
refusal still reads every entry of the area: 65,536. SCHED5's table gives that floor. Per start,
(22.1 - 113.6 ms) / (4,096 - 32,768 starts) is about 3.2 us on rv64, and taking the starts out of
22.1 ms leaves about 9 ms for the reads, about 138 ns an entry on both widths. That is the
"9.6 ms when every table is present" figure: about ten slices. (b) cuts a constant factor and
bounds nothing, so R12 would keep its second exception. That is the reason ruling 6 refused (3)
on "a long run is placed by the kernel's search": "lowers a cost it does not bound". Only (a)
meets the node's "a constant small against a slice whatever the area holds".

## The structure (per process, per placement area)

- **Level 0: a bitmap.** One bit per page of the area, set when the page's leaf entry is empty
  (free); any nonzero entry is taken: a mapping, a reservation, a lent page (S) or a borrowed
  one. `first_occupied` reads entries the same way today. Words are `usize`, so 64 pages a word
  on rv64 and 32 on rv32. Default area: 65,536 bits = 8 KiB. Messages area: 1,024 bits = 128 B.
- **Level 1: one record per 512-page group.** Three u16s: the free run at the group's start, the
  free run at its end, and its longest free run. 128 groups for Default, 2 for Messages: 780 B.
  The group is the Sv39 leaf table's span: 8 words on rv64, 16 on rv32.
- **Kept in step at the leaf writes.** Every leaf write of a user space is in
  `kernel/src/arch/riscv/mem.rs`, through `Space::write_leaf`/`set_leaf` (map_page_in,
  unmap_page_inner, return_page_inner, lend_out/lend_back/drop_lent, unmap_from,
  ensure_page_exists, set_user_page_flags). The one hook goes there. When an entry inside a
  placement area changes between empty and non-empty, the hook flips the page's bit and rebuilds
  its group's record from the group's 8 or 16 words. Building the longest run uses word ops, the
  run test being a shift-and done O(log) times. The cost per page is a constant, about 0.4k
  instructions. The space is named by its ASID (= PID). A write into a child's space
  (`process_map`, `map_into_with`) updates the child's summary. A dying space's summary goes with
  its frames.
- **The search keeps its semantics exactly.** First fit from the run placed last, up to the last
  start that fits, then the wrap from the area's start. Every address it returns is the one today's
  search returns. The search reads the group records. It enters a group's words only for the
  partial group it starts in and for the one group whose longest run is at least the request.
  Because the records are exact, a refusal never enters an interior. A search therefore costs at
  most about 2 x 128 records plus 3 x 16 words, whatever the area holds. My estimate is a few
  thousand instructions, tens of µs under `icount` shift=3. It will be measured and stated on R22.
- **Where it lives and what it costs.** Two frames for the Default bitmap, charged to the
  process's budget when its space is created, as the header page is. They are kernel-only (no U),
  mapped beside the header in `PROCESS_AREA` at an offset clear of the magic return addresses
  (still to check on Sv32, where `EXIT_THREAD` is at `PROCESS_AREA + 0x3000`). That way the
  space's teardown walk frees them with everything else, and a non-current space's frames are
  reached through its own tables. The group records and the Messages bitmap fit in the header
  page's free space, about 3.5 KB. **Every process costs two more pages.** The boot processes
  (PID 1, init) get theirs at boot, built once from the tables the loader made. The release
  build's search code can also be removed (`first_occupied_page` stays for `range_available_in`).
- **The maintenance cost lands in R22's derivation.** A 16-page call adds at most 16 group
  rebuilds, estimated at under about 50 µs, to `map_fixed`'s worst section (0.76 ms on rv32,
  checked). I re-measure that section and state on R22 that the cap's derivation still holds. If
  it does not, I add a per-word longest-run cache (1 KiB, inside the third page's worth) and
  report it.

## The model and the fake

- **Placement does not change**, so the model's `place`, the fake's `place` and the difftest's
  address agreement stay as they are. The difftest and the model's host tests become the
  end-to-end check that the summary is in step: a missed hook (a lend's return, a reservation, a
  `process_map` into a child) shows as a different address, or as a refusal where the model
  places.
- **New model contract (R22):** the model computes the summary-shaped cost of each placement
  (group records read plus words scanned, as above) and records a ghost violation over
  `SEARCH_BOUND`, a constant derived from the structure (2 x groups + 3 x group words).
  **Mutation `R22SearchPerStart`:** the model charges one walk per candidate start, today's search.
  It exceeds the bound once the generator's pinned pages fragment the area, and the contract must
  catch it. gen.rs gets no new draws: its existing "a page in the placement area" step makes the
  shape. Mutation deadlines are re-checked.
- **Kernel unit level:** the summary is a pure module (`kernel/src/place.rs`: bitmap, records,
  first-fit). It gets host tests: a randomized property against brute-force first fit on both word
  widths, plus the record invariants. If the kernel crate has no host-test harness I put it where
  `cargo testbench`'s host tests reach. The checked build asserts after each range call that its
  pages' bits equal their entries. That costs at most 16 reads; there is no whole-area audit on any
  path, because the 9 ms scan would move the timing cases.
- **The fake:** unchanged. It has no clock contract, and its placement semantics are the same.

## Cases that judge it

1. **`map-anon-search-bound`.** Same shape (a page in every 16-page window, 4,096 pinned, request
   16), plus the summary's own worst shape: a rover at a group start and the one fitting run in
   the last group, so a partial group and a found group are both entered. Its refusal and wake
   bounds are re-derived as twice the measured worst, rounded up. I expect them far under 1 ms,
   down from 45 ms. The description and page follow.
2. **The R22 derivation.** I re-measure the worst section (a 16-page `map_fixed` with every table
   absent, rv32 checked) and run the memory set: `map-fixed-attack`, `map-fixed-tables*`,
   `scan-bounds`, `process-map-*` and `sched-range-hammer`. `beamlet-heap-allocation` runs on both
   widths because it places a lot.
3. **The contention family's wake targets:** `sched-lock-contention`, `-4`, `-4-mttcg` and
   `irq-boot-hart-only`. `record=` comes off and they are judged at 15/50 ms.
   **A risk I want ruled before I build the case side.** The self-check
   (`irq-boot-hart-only`, must_fail on the driver wake) misses only because the boot hart sits
   in a long section with interrupts off. With the search bounded, the hammer's section is tens
   of µs, and the costliest call left is about a slice (a 16-page `map_fixed`, 0.76 ms). The boot
   hart then takes the alarm within about a slice, so the negative is likely to meet 15/50 ms and
   pass where it must fail. That is by construction: after K34, no section is long enough for the
   interrupt's routing to cost a 15 ms p50. I will measure this first. If the negative meets the
   targets, my proposal is that the self-check's verdict moves to the structural witness the case
   already prints: the per-hart alarm count, where the other hart takes 0 when only the boot hart
   is enabled. The wake figures would then be judged on the positive only. The other option is
   for the Architect to rule a different self-check. No target moves either way.
4. **Gates:** the model's host tests and mutation sweep, difftest, rv32 builds, the unsafe count
   (the summary frames are reached through the physmap like tables), the size budget (the kernel
   grows), docs and formatting as the bench cases, and a budget sweep. I grep the cases and
   manifests whose budgets are exact, because +2 pages per process may move their numbers. These
   are reported, not widened.

## Pages

- memory.md R22: the search paragraph and table are rewritten with the new bound and its
  structure, and the "up to about a hundred slices" residual goes.
- scheduling.md R12: the second exception is removed; R78 and the contention text are re-judged.
- SECURITY.md: the R12 and R22 rows.
- memory-layout.md, "Per-process kernel data": the summary pages.
- objects.md, "what objects cost": a space is two pages more.
- model.md: the R22 contract and mutation.
