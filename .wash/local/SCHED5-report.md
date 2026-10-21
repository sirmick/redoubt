# SCHED5 report (draft; finished after the steward runs and the mutation sweep)

Package SCHED5 (Tier A, L): every range call refuses a range over `MAX_RANGE_PAGES` (16); callers
split their ranges; the model's contract and mutation; the attack case; the two rv32 cells.
Branch `wp-SCHED5`, base d66839252 (rebased from 3e6f29443), head: see the end.

## What is delivered

- **ABI and kernel (R22):** `MAX_RANGE_PAGES = 16` in `libs/sys`. `map_anon`, `map_fixed`, `unmap`,
  `set_flags` and `process_map` refuse a longer range with `TooLarge`, by arithmetic. The check
  comes after the range's shape and before any walk or charge (`kernel/src/mem.rs`
  `range_cap`, `kernel/src/process.rs`). The error table (`libs/sys/src/error.rs`) gets the rows.
- **The derivation:** the cap is the largest power of two whose worst section fits in one slice on
  the slower width. The worst section is a `map_fixed` that has to make all its page tables.
  - rv32: 16 pages 1,012 µs (970 net), 32 pages 1,768 µs.
  - rv64: 16 pages 780 µs, 32 pages 1,413 µs.
- **Callers split their ranges:**
  - the loader stub maps, protects and unmaps each segment a range at a time;
  - the runtime's `map_fixed`, `unmap` and `Process::map` work a range at a time;
  - `map_anon` of a run longer than a range is one pass under the process's placement lock: the
    first range from the kernel, extended by `map_fixed`, a too-short gap's first range kept as a
    filler, ending when the addresses wrap past the first (the Architect's one-pass rule);
  - the launcher's batch stays 64 pages, split by the runtime;
  - the fake kernel enforces the cap and places runs in a per-process area. Under Miri a run is
    its own allocation; see below.
- **Model:** the cap in R22's contract, a ghost check on every range walk, and the mutation
  `R22RangeCapIgnored`. No new RNG draw in `gen.rs`.
- **Attack case:** `sched-range-hammer`, at one hart. A budget maps and unmaps the longest range
  the kernel takes, in a loop, each time making every page table afresh, beside the latency
  driver. The verdict comes from the kernel's own trace: `RANGE-SECTIONS` holds every range-call
  lock section to one slice net of audits. The driver's wake is judged by the post-check from
  the kernel's clock.
- **The contention family under the cap**, per the Architect's rulings on thread SCHED5-cause:
  - the hammer asks for the most a range call may name (n = 16);
  - the hammers pin before the window;
  - `search_alone` runs in a hammer-sized child;
  - the window is sized from the search measured alone, and the case checks that every driver wake
    fell inside it;
  - the driver wake's figures are recorded, not judged, in sched-lock-contention, -4, -4-mttcg and
    irq-boot-hart-only (its must_fail recorded too), until the search is bounded;
  - ticket order and -4-mttcg's bounds in searches stay judged.
- **Pages:**
  - memory.md: R22, its derivation, the search's true worst with the table, the residuals, and
    the launcher's batch;
  - scheduling.md: R12's term, R78's contention paragraphs and why the gate is unmeetable under
    the cap, Responsiveness, the residual risks, the steward cells;
  - SECURITY.md: the R22 and R78 rows;
  - abi.md, model.md, testbench.md (the oracle check), native.md, init.md.

## The Architect's questions, answered

- n = 8 trace: `.wash/local/SCHED5-trace-n8.md`.
- The corrected hammer: `.wash/local/SCHED5-cover.md`.
- The earlier n = 16 pass at 1.2 ms: the hammer's 4 s window had ended before most of the
  driver's samples. The coverage check now forbids that.

## Gates (commands and exit codes)

TODO: fill from the final runs.

## Measured (quiet, n = 16, two harts)

| | rv64 | rv32 |
| --- | --- | --- |
| search alone p50 | 22,571 µs | 17,033 µs |
| driver wake net p50/p99 (recorded) | 21.4/21.9 ms | 16.0/16.5 ms |
| wakes behind a search | 200/200 | 199/200 (the 200th too, by its gross) |
| lock waits in ticket order | 1,447, at most 1 ahead | 1,443, at most 1 ahead |
| map-anon-search-bound refusal / wake | 22,425 / 21,643 µs | 16,822 / 15,976 µs |
| sched-range-hammer longest section / driver p50,p99 | 761 µs / 1.2, 2.2 ms | 774 µs / 1.1, 2.3 ms |
| -4-mttcg driver p50 in searches (bound 3) | 2.5 | 2.2 |

The figures at n = 16 under train 89's bench read the same to within µs.

Negative runs:
- sched-range-hammer with the kernel cap stubbed out, rv32: one `map_fixed` held the lock
  968,325 ticks (96.8 ms) against the 10,000-tick (1 ms) bound. FAIL as it must.
- `irq-boot-hart-only`: its must_fail is recorded under the cap (ruled). It stopped failing because
  under the cap its positive misses too. Both wait whole searches; the negative must fail again
  when the search is bounded.

## rt-miri

`heap_over_map_anon` ran past 300 s; the base runs the whole case in about a minute. The cause:
the fake's one shared placement area, which Miri's borrow tracker and race detector both check
access by access.
- With both off: 59 s.
- With either one on alone: over 300 s.

Fix: under Miri each `MapAnon` run is its own allocation, as before areas. rt-miri passes in 64.7 s.

## Open items (noted, not fixed)

- `dma_alloc`'s `npages` is not capped by `MAX_RANGE_PAGES`.
- smp-inflight-race's 64-frame staging overflow is now reachable only by destruction.
- The launch time is unchanged: 2.29 s rv32 median.
- The stale `index.lock`: the bench's tree fingerprint (`tools/testbench/src/prebuilt.rs`,
  `git diff HEAD`) leaves `.git/worktrees/SCHED5/index.lock` behind after bench runs. Seen five
  times; each blocks the next commit until removed. `git --no-optional-locks` there would avoid
  it (not this package's file).

## Affected summaries checked

| Summary | Result |
| --- | --- |
| README.md, GETTING-STARTED.md | no claim about range calls, the search or the contention gate ("9.6" there is OpenSSH's version) |
| docs/kernel/README.md | the memory row lists the five range calls by name, no length claim: no change |
| docs/plan/m1-separation.md | R11's row cites `map-fixed-attack`, which still passes under the cap: no change |
| docs/plan/m2-usable-shell.md | R78 described as the FIFO ticket lock, still true; no wake figure: no change |
| docs/plan/m3-agents.md (current milestone) | the latency bench's rerun with the real steward; the rv32 cells are this package's, judged on scheduling.md: no change to the milestone text |
| docs/TOUR.md, GLOSSARY.md, TENETS.md | no mention: no change |
| kernel/README.md, model/README.md | point to the pages; no figure: no change |
| docs/SECURITY.md | R22 and R78 rows updated |
