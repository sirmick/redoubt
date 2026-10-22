# K34 report: map_anon's placement search bounded by construction

Branch `wp-K34`, base main 2b60945b6, head 4389898d3. There are three commits:

1. 2f1cbe7d0 `runs:` the `redoubt-runs` crate (libs/runs), with its host tests and gate entries.
2. bf038e9b7 `kernel, model:` the search reads the runs: the kernel hooks, the model contracts and
   mutation, map-anon-search-bound re-derived, the pages, and the size budget (kernel, model).
3. 4389898d3 `kernel, tests, testbench:` the contention family's new hammer, judged;
   irq-boot-hart-only and its feature retired; the oracle's searches unit removed.

Design note: .wash/local/K34-design.md (option (a), approved).

## What was delivered

- **libs/runs** (`redoubt-runs`, no_std, `forbid(unsafe_code)`) keeps a bit a page (set while the
  entry is empty) and a record a group of 512 pages (head/tail/longest free run).
  - `set` flips the bit and leaves the group's record pending. The pending record is rebuilt when
    a change moves to another group, or when a search settles it.
  - `place` is the old first-fit from the rover, then the wrap. It reads at most
    `2 × (groups + 2 × group words)`: 288 for map_anon's area, 36 for the message area.
- **kernel/src/place.rs** holds map_anon's area's bits in two frames per process, mapped
  supervisor-only at PROCESS_AREA + 4 KiB and + 8 KiB. They are charged at space creation and
  freed with the space. The records, the pending words and the Messages bits live in the header
  page (`ProcessImpl::runs`).
  - **The hook:** `Space::write_leaf` (every leaf write) calls `place::changed` when an entry in an
    area goes empty ↔ non-empty.
  - **The boot spaces** (PID 1, init) get their runs before boot_budgets and are built from their
    tables.
  - **Removed:** `find_virtual_address`'s walk search, `first_occupied_page`, and ProcessInner's
    base fields.
- **Checked-build audit:** `check_runs` runs at release_owned_frames, as a stamped AUDIT_INDEXES
  audit, skipped inside destructions as `check_frame_owners` is. It checks every non-empty entry
  of an area's leaf tables against its bit, and the count against the bits. Hand mutation (the
  hook never frees): process-lifecycle and sched-exit-churn panic in the I1 assert.
- **Model:**
  - Per-process runs via the crate, updated in map_page/unmap_page.
  - Two R22 ghost contracts: the runs' answer equals the page-by-page first fit, and reads stay
    within `most_reads`.
  - Mutation R22RunsKeepUnmapped, caught in 0.3 s. ALL is now 182.
  - Costs.contexts goes 1 → 3, and the example trace is re-blessed (usage +2).
- **Contention family:**
  - The hammer is RangeHammer, a 16-page map_fixed with fresh tables, then unmap, then one
    section's p50 in user mode. The gap keeps the IRQ witness robust: without it, rv64's other
    hart took 1 alarm.
  - A SectionAlone child times the section. The window is K × ((h+1) × max + 3 ms) × 3; at × 3/2
    the four-hart rv64 run fell 85 ms short.
  - record= is off on all three. -4 is judged under icount at 15/50. -4-mttcg loses
    driver_wake_*_searches, and the oracle feature and its test go.
  - rv32 RANGE_AT moved into map_anon's area (0x6080_0000), the costliest range call there.
- **irq-boot-hart-only:** measured alone on K34's kernel, rv64 1143/1317 µs and rv32 1290/1692 µs
  net p50/p99. It met the targets, so it was retired with its kernel feature, as ruled.

## Measured (checked build, icount shift=3 unless said)

- **map-anon-search-bound:** a refused search takes 126 µs on rv64 and 154 on rv32 (was 22.4 and
  16.8 ms). The wake was 1918 and 2004 µs late. Bounds are now 400 µs and 5 ms (were 45/45 ms).
- **Range sections, net of audits (sched-range-hammer trace), longest map_fixed:**

  | Width | main | Branch, outside area | Branch, inside area |
  | --- | --- | --- | --- |
  | rv32 | 905 µs | 909 µs | 959 µs |
  | rv64 | 877 µs | 881 µs | 792 µs |

  - rv32 has a 4% margin to the 1,000 µs slice. memory.md R22 states it, and states that more
    per-page hook work breaching the slice would drop the cap to 8.
  - The first hook design (a record rebuilt each page) read 1,024 µs on rv32, which failed. The
    pending record fixed it.
- **Contention, net p50/p99/max (ms):**

  | Case | rv64 | rv32 |
  | --- | --- | --- |
  | 2 harts | 1.33/2.07/2.82 | 1.46/2.45/2.86 |
  | 4 harts, icount | 7.24/8.09/8.13 | 7.67/9.20/10.14 |
  | 4 harts, mttcg, quiet slot alone | 0.87/1.26/1.36 | 0.83/1.17/2.30 |

  Every target is met. Lock order holds: 1289/1284 waits (2 harts), 5971/7501 (4), 27402/28888
  (mttcg). Alarms taken by the other harts: [200,112] / [200,121] (2 harts);
  [21,60,31,193] / [125,151,136,134] (4); [129,190,126,125] / [146,133,168,131] (mttcg).
- **Section alone, gross:** 897/1651 µs icount, 223/229 µs p50 under mttcg.
- **beamlet-heap-allocation:** rv64 16.8 s (main about 15) and rv32 14.5 s, both pass.

## Gates as run (make -f scripts/jobs.mk set CASES=…, rc 0 unless said)

- **Both kernel builds** (release and checked, rv64 and rv32): clean, no warnings.
- **Host tests:**
  - redoubt-runs: 6 tests pass. Three hand mutations (no interior scan, `>` for `>=` in head, no
    word longest) are each caught.
  - redoubt-model: all 16 binaries pass at the final head, `--no-fail-fast`.
  - Mutation sweep, all 182 one by one: all caught. Slowest R2NoWaitCap at 14.5 s (limit 25).
  - testbench sched_oracle: 65 pass.
  - host-tests case: pass.
- **Smoke set,** both widths: userland-boot, init-boot, bench-net-peer, ipc-outcomes, sum-clear,
  and lend-untouched-page at 1 and 4 harts. All pass.
- **Own and touched cases,** both widths:
  - map-anon-search-bound, map-anon-placement, map-fixed-attack, map-fixed-tables(-rv32),
    process-map-untouched-attack, scan-bounds;
  - sched-range-hammer, sched-lock-contention, -4, -4-mttcg (alone, quiet slot);
  - budget, budget-mem-churn, -destroy-growth, -table-attack, -carve-attack, -reap;
  - ipc, ipc-outcomes, lender-touches-lent;
  - process-lifecycle, process-fill, sched-exit-churn, init-refuses-overcommit,
    init-refuses-system-fit, heap-cap, consrelay-footprint, memory-host-tests;
  - beamlet-footprint, beamlet-heap-allocation, steward-end-lease, steward-lease-cut.

  All pass.
- **Final head re-run** after the commit fold: size-budget, unsafe-budget, docs, formatting,
  no-cruft, host-tests, map-anon-search-bound, map-anon-placement, ipc-outcomes, process-lifecycle
  and sched-lock-contention. All pass.
- **Unsafe:** none added. place.rs goes through kframe's read and write. redoubt-runs is budgeted
  at 0.
- **Size budget:**
  - kernel 10899 → 11065 in commit 2, then 11061 in commit 3;
  - model 11526 → 11574;
  - libs/runs is new at 227.

  Each raise has its `Size budget:` line.
- **Not run:** the whole bench (that is the train's). The irq-boot-hart-only measurement ran on
  the pre-fold tree. Earlier sets also ran before the fold, which changed only place.rs's debug
  assertion and the crate's comments.

## Affected summaries checked

| Page | Change |
| --- | --- |
| memory.md | R22's status, search bullet (the old table is gone), the hammer's figures, the margin and the fallback cap; "Where map_anon puts pages"; the residual "up to about a hundred slices" removed; the three-walks residual |
| scheduling.md | R12 (the second exception removed); R78's contention and self-check paragraphs; the Responsiveness sentence and status count; the trace feature lists; residuals ("A hart waits for QEMU's turns", "Every hart pays…", the R12-section wake sentence) |
| SECURITY.md | R22 row (Tested-by, code, residual); R78 row (Tested-by without irq-boot-hart-only, residual) |
| boot.md | Hardware abstraction status (7) and the count paragraph |
| testbench.md | Scheduler oracle status: the deleted test removed |
| objects.md | Cost table row and text |
| processes.md | What a process costs |
| budgets.md | init's charge |
| memory-layout.md | Per-process kernel data |
| model.md | Costs default, ALL = 182, R22 mutation row |
| kernel/README.md | Object table, TCB table (kernel 16,667, runs row, total 30,515; snapshots) and crate map |

Checked, no change needed:
- README.md and GETTING-STARTED.md: no search or per-process cost claims.
- docs/todo/kernel-attack-gaps.md: the header-page note stands.
- memory.md line 186: a dated zeroing measurement.
- servers/init and image manifests: budgets are pages with margin, and the init-boot, footprint
  and fit cases pass.

## Design problems and open risks

1. rv32's worst range section is at 959/1000 µs. Any further per-page cost in a placement area's
   leaf write drops the cap to 8 (stated on R22).
2. The process-end audit is skipped inside destructions, by the same rule as check_frame_owners.
   Those spaces' runs are proved only by the model and difftests, not by the kernel audit.
3. Two pages more per process (8 KiB): no case's budget moved, but any manifest sized to the page
   now has 2 fewer.
4. The hammer's user-mode gap halves its load. The four-hart icount p50 of about 7.5 ms reflects
   QEMU's turns (residual stated), not the kernel.

Next: the review panel (kernel-red-4, simplifier, editor).

## Round 1 fold

New head: 8f62c6683, two commits on main 2b60945b6.

- bfc3a562b `runs:` the crate and its gate entries.
- 8f62c6683 `kernel, model, tests, testbench:` everything else.

The search change and the contention change are now one commit. At the old middle commit
(5c864509b, checked in a scratch worktree) `sched-lock-contention-4` rv32 failed its alarm-count
clause. With the search bounded, the old search hammer no longer holds the kernel long enough, so
the two changes cannot stand apart. Everything else passed its own gates at that commit.

### Red

- **P1 thread-limit.** It now expects the header page, 2 pages of runs and LEVELS tables (program
  and toml). A grep of every program that does usage arithmetic around process_create found these
  cases, all run on both widths and all passing: thread-limit, budget-table-attack,
  deadline-in-launch, dma-rules, handle-chain-attack, handle-chain-fault, page-table-reclaim,
  pid-pinning-attack, process, process-attack, process-chain-fault, stub-launch,
  write-only-attack, process-lifecycle, process-fill, sched-exit-churn, init-refuses-overcommit.
  thread-limit was the only exact count.
- **P2 map_anon's costliest section.** Measured under icount in sched-range-hammer's trace, in a
  scratch hammer: a page pinned in every 16-page window but the last leaf span (which has no
  table), then one map_anon of 16 pages. Its search reads every record (the rover is near the
  start) and lands at the table-less span (page 65,024 on rv64, 64,512 on rv32), making a table.
  - It holds the kernel 707 µs on rv64 and 903 on rv32, net of audits.
  - A found run reads at most about one pass of records wherever the rover is (pass 1 to the end,
    then pass 2 only up to the run).
  - So it is not the costliest; the in-area 16-page map_fixed is. The hammer is unchanged.
  - Stated on memory.md R22.
- **P2 check_runs.** It now settles the pending record and checks records_hold for both areas,
  alongside the bits.
  - Settling first is what makes the record check bite: a small process's only group is always
    the pending one.
  - Hand mutation, the kernel store writing `longest - 1` for groups not fully free:
    process-lifecycle panics at the records assert on both widths. Without the settle it passed.
- **P3 the count.** `place` counts the settle (8 words). most_reads is 296 for map_anon's area and
  44 for the message area; the pages say so.

### Editor

- **P1:** the testbench.md searches-unit passage is deleted.
- **P2:** history wording is removed from memory.md R22, scheduling.md R78 and boot.md. The pages
  now state present facts: the routing saves a wake one section, so the counts are its test.
  Comparisons are in the commit message.
- **P3:**
  - libs/runs' size and unsafe rows: new entries, and the gate passed without a Size or Unsafe
    budget line. None is needed (no raise of an existing ceiling).
  - The README counts are recounted: kernel 16,646, runs 472 (185 tests), total 30,472.

### Simplifier

- **P2:** header_phys calls Space::header.
- **P3, all taken:**
  - The crate is u64-only: the Word trait and the u32 tests are gone. Tests are now
    placement_is_the_old_searchs, dense_shapes, the_area_s_edges and
    the_longest_run_in_a_word, renamed in memory.md and SECURITY.md.
  - One `Runs` store (bits through frames from a byte offset; records and pending in the header)
    replaces DefaultArea and MessageArea.
  - `place::page_of` returns the area and page once; write_leaf passes that to `changed`.
- Size: kernel 11,048 (ceiling lowered from 11,063), runs 200, model 11,574.

### Figures at the new head (checked, icount)

| Measure | rv64 | rv32 |
| --- | --- | --- |
| Refused search | 152 µs | 187 µs |
| Timer wake during it, late | 1,938 µs | 2,031 µs |
| Longest range section, net | 883 µs | 967 µs (in area) |
| Driver wake p50/p99, 2 harts | 1.4/2.1 ms | 1.5/2.4 ms |
| Driver wake p50/p99, 4 harts | 6.7/8.4 ms | 7.7/9.3 ms |

The bounds hold: 400 µs is at least twice 187, rounded up, and 5 ms is at least twice 2.0 ms.

The folded store costs about 30 µs a search and 8 µs a 16-page range call against the pre-fold
build. rv32's margin to the slice is now 3% (967/1000), stated on R22.

### Gates at the new head (both widths where boot)

All 76 jobs pass, as run at 2f39dd31a; 8f62c6683 differs only in figures in docs, comments and
the message, and docs, formatting and size-budget pass again on it.

- size-budget, unsafe-budget, docs, formatting, no-cruft, host-tests;
- the smoke set: userland-boot, init-boot, bench-net-peer, ipc-outcomes, sum-clear,
  lend-untouched-page at 1 and 4 harts;
- map-anon-search-bound, map-anon-placement, sched-range-hammer, thread-limit;
- sched-lock-contention, sched-lock-contention-4, map-fixed-attack, map-fixed-tables(-rv32),
  process-map-untouched-attack, scan-bounds, budget;
- the exact-charge list above.

Also passing: the model suite (16 binaries, reniced) and the R22RunsKeepUnmapped and
R22MapAnonSearchesRange mutations.

Not rerun at this head:
- sched-lock-contention-4-mttcg: host clock, no slot. Its last pass was the pre-fold tree.
- The full 182-mutation sweep: the model changed only in its Store impl's signature.
