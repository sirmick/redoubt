# SCHED5 report

Package SCHED5 (Tier A, L): every range call refuses a range over `MAX_RANGE_PAGES` (16); callers
split their ranges; the model's contract and mutation; the attack case; the two rv32 cells.
Branch `wp-SCHED5`, base 820d5e5ff (origin/main after train 90), head edab94595, four commits:

| Commit | Subject |
| --- | --- |
| 2a7fa23aa | rt: range calls go a range at a time, and a long run is placed in one pass |
| 558df8391 | stub: a segment is mapped, protected and unmapped a range at a time |
| 0c152a8d3 | model: a range over MAX_RANGE_PAGES is TooLarge, after its shape and before any walk |
| edab94595 | kernel: no range call holds the kernel longer than about a slice |

Each commit carries its `Size budget:` lines. Commit 1's rt and fake host tests and commit 3's
changed model tests were run at those commits. The pre-rebuild head is kept as the local branch
`sched5-pre-rebuild`; nothing is pushed.

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

All `make -f scripts/jobs.mk ...` through q.

On the final base 820d5e5ff, head edab94595:

| Gate | Command | rc |
| --- | --- | --- |
| docs (the bench case) | `rv64/docs` | 0 |
| formatting (the bench case) | `rv64/formatting` | 0 |
| size budget | `rv64/size-budget` | 0 (five ceilings raised, each with a `Size budget:` line) |
| unsafe budget | `rv64/unsafe-budget` | 0 |
| model host tests | `rv64/model-host-tests` | 0 |
| model mutations, all 177, `R22RangeCapIgnored` caught | `set CASES=model-mutations` | 0 |
| rt under Miri | `rv64/rt-miri` | 0 (64.7 s; it was over 30 min before the fake fix) |
| rv32 build | `build-rv32` | 0 |
| rt and fake host tests | `q run cargo test -p redoubt-rt -p redoubt-fake-kernel` | 0 failing |
| prebuild (every case's binaries; covers the libs/rt consumer sweep) | `prebuilt` | 0 (rv64 367 cases, rv32 353, none failed) |
| steward-latency, rv32 at two harts, with the `record=` cells | `testbench --prebuilt ... --exact --arch rv32 steward-latency --smp 2` | 0 (769 s; row 1 at 3.4 ms; the deadline notice 43.4 / 42.4 / 51.1 ms, recorded) |

On d66839252, in the quiet slots (before the base moved to 820d5e5ff and the rebuild; the tree's
content is unchanged since but for comment reflows):

| Case | rv64 | rv32 |
| --- | --- | --- |
| sched-lock-contention | 0 | 0 |
| sched-lock-contention-4 | 0 (rerun after the window fix) | 0 (rerun) |
| sched-lock-contention-4-mttcg (quiet cores) | 0 | 0 |
| irq-boot-hart-only | 0 (rerun after the window fix) | 0 (rerun) |
| map-anon-search-bound | 0 | 0 |
| sched-range-hammer | 0 | 0 |
| steward-latency at one hart | n/a | 0 |
| steward-latency at two harts | n/a | 1 before the `record=` cells (their misses only, the same on the base); 0 with them on the final base, above |

Not run here: the whole bench (the train's), and the full smoke set on the new base.

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

## SCHED5's two rv32 cells (steward-latency, net, each boot alone)

| Cell | Head | Base before the cap | Target |
| --- | --- | --- | --- |
| rv32, 1 hart, N = 1 decision wake p50 (row 2) | 528 µs | 32.5 ms (recorded) | 25 ms |
| rv32, 2 harts, N = 2 timer wake p50 (row 1) | 4.0, 4.3 ms (two runs) | 38.0 ms | 15 ms |
| rv32, 2 harts, deadline notice p99, N = 1 / 2 / 3 | 43.4 / 45.3 / 56.5 ms | 41.9 / 41.9 / 56.1 ms | 40 ms |

- Rows 1 and 2 are fixed and judged.
- Ruled (a) on SCHED5-cause: rv32 is gated at one and two harts.
- The deadline notice at two harts is recorded via `record=`. It misses on the base too and was never
  judged there before. It is its own node (B103), and the page names it without an id.

## Four harts under the cap (quiet runs; figures on scheduling.md)

| Case | rv64 | rv32 |
| --- | --- | --- |
| sched-lock-contention-4, wake net p50/p99 (recorded) | 113.0 / 113.5 ms, about 5 searches | 86.1 / 86.6 ms |
| sched-lock-contention-4, lock waits in ticket order, at most 3 ahead | 22,350 | 24,356 |
| sched-lock-contention-4-mttcg, p50 / p99 in searches (bounds 3 / 8) | 2.5 / 5.5 | 2.2 / 2.7 |
| irq-boot-hart-only, wake net p50 (recorded) | 44.1 ms, 2 searches | 33.3 ms, 2 searches |

## Observations for the Architect and the orchestrator

- **The self-check still separates in searches under the cap.** The positive waits one search a
  wake, irq-boot-hart-only two. A bound in searches, as -4-mttcg has, could gate IRQ1 before the
  search is bounded. Not done (ruled: record until the search is bounded).
- **The steward cases' whole-run status.** The page said they stay out of the whole run "until the
  two-hart rows pass on both widths". They now pass (bar the recorded cell). I left
  `whole_run = false`, whose toml reason is cost, and changed the page to say they run by name.
  Whether they join the whole run is the orchestrator's call.

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

## Fold round 1 (head 1986b1764, base 820d5e5ff)

Range-diff from the reviewed head edab94595: `.wash/local/SCHED5-range-diff-r1.txt`. The model
commit is unchanged; rt, stub and kernel are rebuilt. The reviewed head is kept as the local
branch `sched5-r0`.

**Red:**
- **P2-1 (ruled (a)).** A device owner's `dma_alloc` and `map_device` are the third cost a slice
  does not bound, stated on R22, R12 and SECURITY.md. The only calls are in each driver's `open()`,
  with constant sizes:
  - `servers/blkd/src/kernel.rs:66`: `dma_alloc(DMA_PAGES)`, 9 pages.
  - `servers/netd/src/kernel.rs:146`: `dma_alloc(REGION_PAGES)`, 9 pages, once for each of the two
    queues, from `netd.rs:109`.
  - one `map_device` each.

  Measured from one rv32 boot's trace, checked build (a scratch steward-deadline-burst with
  `hold-trace`): each `dma_alloc` 304 to 325 µs and each `map_device` at most 548 µs, net of audits.
  The optional host test or debug assertion holding the calls to `open()` was not added (the
  simplifier's call).
- **P2-2.** `place()` keeps what the run took of a too-short gap as the filler, so the next search
  passes the whole gap. The peak stays within len plus one range. New host test
  `a_gap_longer_than_two_ranges_but_short_of_the_run_is_probed_once`: it fails on edab94595
  (three searches) and passes now (two). `a_page_taken_meanwhile...`'s call sequence is updated.
- **P3-1.** Each search hammer reports `u64::MAX` if its pins finish after its window's start; the
  case checks "every hammer pinned its area before the window opened" (four tomls).
- **P3-2.** sched-range-hammer checks that its last driver sample falls inside the hammer's window.
- **P3-3 (ruled (a)).** Scratch program, the loader's first process: `process_map` of 16 pages into
  a fresh child across the widest table boundary, timed by `time_now`. Net of the timing (41 µs
  rv32, 29 µs rv64):
  - touched source: 994 µs rv32, 963 µs rv64;
  - untouched reservation source: 1,765 µs rv32, 1,445 µs rv64.

  Stated on R22 as the cap's one overrun, with the condition that would make it a node, and in a
  clause on SECURITY.md.

**Simplifier:**
- `redoubt_sys::range_chunks` beside `MAX_RANGE_PAGES`, used by the stub, rt and rd. It yields
  `(addr, 0)` once for an empty range.
- `rd::map_fixed_run` deleted, and `rd::each_range` made private.
- `HAMMER_PAGES` and the search case's `RANGE` replaced by `MAX_RANGE_PAGES`.
- The optional `check_range_len` was not done.
- Ceilings: libs/sys 1,048 (+4, `range_chunks`), stub 364 (−11), libs/rt 4,189 (−15).

**Editor:**
- History wording and superseded figures removed from scheduling.md (Responsiveness, R78, the
  table cells, the residuals), memory.md and native.md. The figures that set the -4-mttcg bounds
  are kept, tied to the runs that set them.
- The rv32 two-hart deadline-notice miss is added to the R12 targets row and as a residual.
- The rv32 two-hart N = 4 cells are marked as an earlier run, not judged.
- rv64 per-page cost: 40 µs.
- Each n = 16 figure names its run.
- The lease figure is sized: 5,248 pages, a page in every 13-page window, about 27 ms.
- The commit message gives 970 net.
- The case comment's history is removed.

**Found in the fold's boots:** five test programs still made one raw range call over the cap:
budget-victim (64 pages), smp-evict (512), smp-shootdown (64), thread-limit (255) and
lend-untouched (12,288; its case expects `OutOfMemory`). All now use `rd::map_anon_run`. A
whole-tree grep finds no other raw range call over 16 pages outside the runtime.

**Gates:**
- At b49e2477c, rc 0 each: docs, formatting, size-budget, unsafe-budget, model-host-tests, rt-miri,
  build-rv32, the rt / fake / sys / testbench tests, and the prebuild.
- At 1986b1764, which changes those five programs only: formatting rc 0 and prebuild rc 0.

**Boots, quiet slots, both widths:**
- At b49e2477c, 40/42 pass: the contention family (-4-mttcg on quiet cores), sched-range-hammer,
  map-anon-search-bound, stub-launch, map-fixed-attack, map-fixed-tables (+rv32),
  map-anon-placement, page-table-reclaim, pages-exhaustion, process-map-untouched-attack,
  syscall-attack, session-hostile-launch, touch-beyond-ram, smp-inflight-race, worst-walk,
  fault-report-bound. The two failures were budget-syscall-attack, for the reason above.
- At 1986b1764, 24/24 pass: budget-carve-attack, budget-destroy-attack, budget-destroy-kills,
  budget-forge-attack, budget-syscall-attack, budget-table-attack, lend-untouched-page, smp-evict,
  smp-evict-mttcg, smp-shootdown, smp-shootdown-mttcg, thread-limit.

## Train 91 (sched5-implementer-3, head 378431399, base 820d5e5ff)

Range-diff from the reviewed head 8106fb191: `.wash/local/SCHED5-range-diff-r3.txt`. Only commit 1
(rt) changed; stub, model and kernel are `=`.

**Commit 1 changes:**
- The WIP is folded in: `libs/client/tests/launch.rs` counts a batch's range calls, and
  `redoubt_rt::abi` re-exports `range_chunks`.
- The message adds one sentence: "The launcher's tests count a batch's range calls."
- The libs/rt ceiling goes from 4189 to 4190, for the re-export's wrapped line. The commit's
  existing `Size budget: libs/rt` line covers it.

**Failure 1, client-host-tests:** fixed by the folded test change. The tests keep their claims and
compare whole range-call lists built with `range_chunks`.

**Host-test sweep at ea9666366 (same code as the head):** each case is `make -f scripts/jobs.mk
rv64/<case>`, rc 0.
- Groups of four: blkd, consrelay, erofsd, fileserver, host, init, ipd, littlefsd, net, netd,
  piped, sshd, steward and walfsd host-tests.
- On the quiet cores: client-host-tests (19.4 s) and rt-host-tests (10.6 s).
- rt-miri: rc 0, 84.5 s.

**Gates at b67bbb948 (the fold before the ceiling fix):**
- docs: rc 0.
- formatting: rc 0.
- unsafe-budget: rc 0.
- size-budget: rc 1, libs/rt 4190 against a ceiling of 4189. Fixed in 378431399, which differs
  only by that ceiling.
- prebuilt: stopped for B101's quiet slot.
- Still to run at 378431399: size-budget and prebuilt.

**Failure 2, rv32 pipe-sixty-four:** identified as a pre-existing race in the platform's server
lifecycle, not a SCHED5 code path. Detail: `.wash/local/SCHED5-pipe64-identification.md`.
- The session VM's waiter calls piped's endpoint after piped has died. That endpoint is owned by
  the session's budget (jobs.rs `served()`), so the call is never failed.
- The waiter's place stays held, and every later pipeline waits 10 s and fails emfile.
- Rates alone: base 3/3, head 3/6, train head 0/4.
- Orchestrator's ruling: a separate node, B106; SCHED5 is held until it lands, then rebases and
  runs pipe-sixty-four rv32 x10 alone. Nothing on wp-SCHED5 changes for the race.

**Scratch, not on the branch:**
- Kernel probes: `.tmp/SCHED5/instr*.py`.
- Scratch worktrees: `.tmp/SCHED5/wt-{base,head,merge,train}`, kept for the B106 witness.
- Logs: `.tmp/SCHED5/q64/`.
- Local branch `sched5-pre-fold3` (ea9666366): kept until acceptance, never pushed.

**Affected summaries:** no change from round 2's table. The fold changes a test and one re-export,
and no claim on any page.

### Red P3s folded (head 42d50c520)

The red team renewed at b67bbb948: OK with notes. Both P3s are folded into commit 1:
- **P3-1:** `moved_in`'s doc says the call list no longer tells batches apart (`range_chunks` is
  the runtime's own, and 64 is a multiple of 16), and that the batch unit rests on `held_peak`.
- **P3-2:** new test `a_refusal_inside_a_batch_leaves_its_moved_range_in_the_child`. It refuses the
  third batch's second range call and asserts the child holds the stub, two batches and one range,
  and that the launcher holds what it held before.

Gates at 42d50c520, each rc 0:
- client-host-tests
- `cargo test -p redoubt-client --test launch`: 12/12
- formatting
- size-budget
- docs
- prebuilt: 353 cases a width, 0 failed

Range-diff from 8106fb191: `.wash/local/SCHED5-range-diff-r3.txt`. Only commit 1 changed; the
other three are `=`.
