SCHED5 handoff (sched5-implementer, 2026-10-20). Worktree /home/mcloonan/redoubt/.worktrees/SCHED5, branch wp-SCHED5, base 3e6f29443, head f996c9e95, clean tree. 12 WIP commits, all to be rebuilt into clean logical commits before review (kernel / rt+stub+launcher+fake / model / cases+oracle / pages). Never push.

## TRAPS FIRST
- Load rules: more than 4 boot jobs, or anything timing-judged while another member's load run holds the machine, needs the orchestrator's go. The latest instruction (sched5-hold-2) holds the timing reruns (sched-lock-contention*, irq-boot-hart-only, smp-inflight-race, map-anon-*) until B98's load run ends. Single trace boots and builds are allowed.
- Quiet slots: start nothing on q. My rv32 steward-latency job once voided B94's quiet rounds.
- Never edit the worktree while a prebuilt case runs ("tree changed").
- Always re-run `make -f scripts/jobs.mk prebuilt` after commits, before cases.
- Shell env every command: PATH=$HOME/.cargo/bin:$PATH BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper REDOUBT_TMP=/home/mcloonan/redoubt/.tmp
- In a backgrounded `( ... ) &` chain, set variables before the subshells. My rv32 smp2 run never ran because T was empty there.
- target/jobs/*.smpN.log is stale unless the case failed. Read consoles from target/testbench/run-* at once; they are pruned.
- A Logger panic shows only its first line: print values before asserting.
- Never `git add -A` (I slipped once with paths; stage by path).
- Docs gates are the bench cases: `make -f scripts/jobs.mk set CASES=docs` and `set CASES=formatting` (fix with `cargo +nightly fmt`). Both were green at c5a7e6677; re-run after the last edits.
- The scratch worktree /home/mcloonan/redoubt/.tmp/SCHED5/wt is detached at f49dc3690, with PLACE_PAGES=64 hacked in libs/client/src/launch.rs and scratch cases sched5-block-n1/sched5-measure/sched5-search (untracked). Remove it when done: `git worktree remove --force`.

## What is built
- **Kernel:** MAX_RANGE_PAGES=16 in libs/sys/src/lib.rs. Range calls (map_anon, map_fixed, unmap, set_flags, process_map) refuse over it with TooLarge after the range's shape, before any walk (kernel/src/mem.rs range_cap, process.rs). Error rows in libs/sys/src/error.rs (RANGE_LIMIT).
- **Model:** MAX_RANGE_PAGES in spec.rs; range_cap plus a range_walk ghost check in kernel.rs; mutation R22RangeCapIgnored (count 177), caught at seed 2. Tests check_order.rs a_range_over_the_cap..., map_fixed_contracts updated. No new RNG draws.
- **rt (libs/rt/src/handle.rs):**
  - map_anon over a range: one-pass search with one filler range, the Placing lock, ending on wrap at the origin (Architect ruling).
  - map_fixed, unmap and Process::map chunked.
- **Fake kernel (libs/rt/fake):** a placement Area per process (AREA_PAGES exported), the cap enforced. Tests in libs/rt/tests/ranges.rs (7).
- **Stub:** each_range chunking.
- **Launcher:** PLACE_PAGES stays 64. 16 made launches 60% longer (rv32/1 median 2.29 to 3.65 s); 64 gives back 2.29 s, the base's.
- **Test programs:** rd::each_range / map_anon_run / map_fixed_run / unmap_run / process_map_run. Many programs reworked: map-fixed-attack, map-fixed-tables(+rv32), map-anon-search-bound, page-table-reclaim, pages-exhaustion, touch-beyond-ram, fault-report-big, worst-walk, syscall-attack, smp-inflight-race (FILL_WAIT 20 s), process-map-untouched, stub-launch, spawn.rs, log-server, map-anon-placement (expects TooLarge).
- **Oracle:** the RANGE-SECTIONS check (tools/testbench/src/sched_oracle.rs check_range_sections plus a unit test).
- **New case tests/sched-range-hammer.toml** (one hart): passes both widths. Longest range section 905/877 us, driver p99 2.3 ms.
- **Contention hammer (tests/programs/src/sched.rs):** HAMMER_PAGES currently 8; it pins before its window; go lead 10 s in lock_contention; search_alone runs in a SearchAlone child (Role 36) holding the pins.
- **Search case:** REFUSAL and WAKE bounds 45 ms, AFFORDS 4,500..8,400 pages, memory_mib 128, prints values before asserting.
- **Pages updated:** memory.md (R22 rewritten, residuals, staging sentence, "larger than the area"), abi.md, scheduling.md (R12 sentence, R78 hammer, Responsiveness status), model.md, SECURITY.md R22 row, testbench.md (oracle), native.md (runtime long runs, 7 tests), init.md, budgets.md.

## Measured
- Derivation: map_fixed with tables absent, rv32 16 pages 1,012 us (970 net), 32 pages 1,768; rv64 780 / 1,413.
- Identification: .wash/local/SCHED5-identification.md. Search finding: .wash/local/SCHED5-search-finding.md (map_anon worst search 114 ms rv64 at n=2).
- Row 2 FIXED: rv32/1 steward-latency decision_wake N=1 net p50 781 us, was 32.5 ms; all rows met (.tmp/SCHED5/G-steward/smp1). That run was at 16-page batches; rerun at head.
- Row 1 (rv32/2 timer_wake N=2) NOT yet measured. The attempt failed on the launch regression (now reverted).
- Memory set (.tmp/SCHED5/set-memory): 43/52 passed first. Since fixed and passing: map-anon-placement both, smp-inflight-race rv32, sched-lock-contention-4 both (at n=16).

## Open with the Architect (thread SCHED5-cause, last message mine)
- The contention self-check: no n separates the pair on rv64.
  - n=16: positive meets both widths; negative meets on rv64 (p99 44.6, bad), misses on rv32.
  - n=8: positive misses both (p50 34/23 ms); negative misses rv64, meets rv32.
  - I proposed n=16, rv64 as a stated finding on scheduling.md, irq-boot-hart-only rv32-only, revisited with K34. Await the ruling, then set HAMMER_PAGES and the pages' hammer sentence: R78 paragraph "64 KiB window, 4,096 pages" and the sched-lock-contention.toml comment (now says 64 KiB/16 pages, matching n=16). If n=16, put back the -4 lead (5 s is enough).
- The page figures for the search (114 ms worst, the table, lease about 25-30 ms) are ruled (a), but memory.md/scheduling.md/SECURITY.md still say 9.6 ms: update per the ruling ("SCHED5: the search's true worst" in .wash/local/wave-2026-10-18-rulings.md). K34 follows SCHED5.

## Next steps
1. Apply the contention ruling. Update the pages' search figures (9.6 to the table; the case's 45 ms bounds and their n=16 shape on memory.md R22).
2. Prebuild; with the go, rerun: sched-lock-contention, -4, irq-boot-hart-only, map-anon-search-bound (both widths), steward-latency rv32 smp1 (make rv32/steward-latency) and rv32 smp2 (q run ... testbench --prebuilt ... --smp 2).
3. Judge the two rv32 cells: remove `record=rv32:1:N=1:decision_wake:p50` from tests/steward-latency.toml and make smp rv32=[1,2] if row 1 passes (rulings: no target moves). Update scheduling.md 'With the real steward' (the rv32 two-hart column and the paragraphs "Two rv32 rows are findings" / "The same finding at one hart") with the new numbers and the cause (the stub's text map_fixed, 39.8 ms rv32, now at most about 1 ms a call).
4. Negative run for sched-range-hammer: kernel range_cap stubbed to Ok in the scratch worktree, rv32. Expect a RANGE-SECTIONS failure (memory.md claims about 100 ms per call at 2,048 pages; verify the number).
5. model-mutations full sweep (a load run, needs the go): `make -f scripts/jobs.mk set CASES=model-mutations`. Plus model-host-tests, rt-miri, docs, formatting, the rv32 build, the unsafe and size budgets (stub size?).
6. Rebuild the commits; write .wash/local/SCHED5-report.md (gates with exit codes; dma_alloc's npages unbounded, noted not fixed; smp-inflight-race's 64-frame staging overflow now reachable only by destruction; the launch re-measured 2.29 s unchanged). Then member_update. Reviewers: sched5-red, sched5-simplifier, sched5-editor. The simplifier decides MAX_RANGE_PAGES vs MAX_LEND_PAGES.
