# GATE1 progress (resumed on K15, main a678d8928) — 2026-10-01

Branch wp-gate1 rebased onto a678d8928: the five commits (conflicts resolved: sched-latency
imports its workload constants from sched.rs; Bench keeps both samples() and receive_report()),
the WIP CT_SPLIT commit dropped (the samples carry each window's end), plus 1bd502c47 "the
containment gate's latencies are judged net of the audits" (driver p1=1, stand-in windows and
hand_over, launcher prints gross lines and b.samples(.., "gate"), its target rows and the
retired target consts go, the case's post_check gets sched-latency's bounds, 'target missed'
forbid gone with the rows that printed it).

## (3) rv64, seed 3: `in-dev cargo testbench kernel-containment --arch rv64` → exit 0, PASS 225.2 s
- driver wake (200): net 8805/10687/19544, gross max 22961, audits 3417 µs: met (15000/50000)
- timer wake (64): net 8045/9651, audits 0: met
- decision wake (9): net 8454/8543, audits 0: met (25000/95000)
- deadline notice (18): net 21803/23545, gross 46228/54549, audits 498788 µs: met (p99 40000)
- R10 (18): 21190/23874 µs, up to 17525 frames, no audit inside one: met (30000)
- lease end: 8543 + 23874 = 32417: met (125000)
- oracle: 543616 records, 108328 picks in rank order; 230 audits, 2789380 µs total
- every ok row as expected (the case's expect list passed)

## Parked 2026-10-01
Sweep state: .wash/local/GATE1-sweep.md (rv64 seeds 1-12 pass; 13-16 and all rv32 not run).
Left: rv32 at seed 3; finish the sweep (rv64 13-16, rv32 1-16), pin the worst seed in the case file;
the page (docs/kernel/README.md Containment: planned -> built, status line names
bench:kernel-containment, the sweep table under The run); doccheck; `cargo testbench --allow-skip`
(one SKIP: bench-ssh-loopback-openssh); `./build --arch rv32 --programs`; fmt; fold the six
commits into logical ones (1bd502c47 is the protocol adoption).

## Resumed 2026-10-02 (gate1-implementer-2), branch wp-gate1 on main 0a82e2090
- Rebased with no conflicts. Sweep (32 runs, all PASS): .wash/local/GATE1-sweep.md, logs GATE1-sweep2-logs/.
- Pinned qemu_seed = 4 (worst notice: rv32 31336 µs p99 vs 40000).
- Page: docs/kernel/README.md Containment is built and tested by bench:kernel-containment; the sweep table and the bimodality note are under The run; "Open: none." dropped (C1). docs/plan/m1-separation.md: the row names `kernel-containment`, the gate moved from Remaining work to Progress.
- Bench::collect/collect_words restored to main's (60 s, 4 slots); the gate does not use them.
- QA citations removed from code comments (C11, the bench's docs case failed on them).
- Whole bench `in-dev cargo testbench --allow-skip`: 273 PASS, 1 SKIP (bench-ssh-loopback-openssh), 1 FAIL (docs, C11). After the comment fix: `cargo testbench docs` PASS, sched-latency (4 rows) PASS.
  Gate in that run: rv64 notice p99 24015, rv32 31336; PASS 163.8 s / 133.0 s.
- doccheck 0, fmt 0, ./build --arch rv32 --programs done; commit 1 builds alone (rv64).
- Fold: d7d1ab92d (child panic), 555aeb6a5 (the gate, case, page). Tree identical to the pre-fold tip.
- unsafe: no change (no kernel source, no new unsafe).

## On K18 (main ca5a6437b), 2026-10-02
- Rebased --signoff, no conflicts. Seed 4, two leases live at D's deadline: PASS on both widths. Notice net p99 was 25292 on rv64 and 25863 on rv32; R10 p99 22128 / 22172; lease end 30071 / 30505.
- Sweep, seeds 1-16 on both widths: 32/32 PASS. Logs are in GATE1-sweep3-logs/.
  - Worst notice p99: rv64 seed 13 at 35586, 89% of target. rv64 seeds 3, 7, 8 and 13 are about 34-36 ms; every other run is about 25 ms. rv32's worst is seed 12 at 27492.
  - Worst R10: rv32 seed 16 at 22475. Worst lease end: rv32 seed 16 at 30823. Driver p99 at most 11113.
- Pinned qemu_seed = 13. The page's sweep text was rewritten: the table, 89%, the groups, and the per-width seed sentence.
- Whole bench, `in-dev cargo testbench --allow-skip`: exit 0, 281 PASS, 1 SKIP (bench-ssh-loopback-openssh). The gate at seed 13: rv64 notice 35586, rv32 26249.
- doccheck 0, fmt 0.
- Fold: 57704ccf6 (child panic) and 1b7bac2e9 (the gate). The tree is identical to the pre-fold tip.

## Fix round 2 (editor), 2026-10-02
- scheduling.md's gate sentences were re-measured at seed 13. Normal run, from the whole-bench log: rv64 35586 net / 116284 gross / 1179940 audit; rv32 26249 / 97696.
- audit-unstamped, rv64: 95015 net (GATE1-neg-unstamped-rv64.out). The first attempt's trace was rejected ("a record after the end line"); the rerun was clean.
- audit-billed: rv64 44838, rv32 45633 net (GATE1-neg-billed.out). Both were run by adding the feature to the case file locally, never committed.
- budgets.md: 4,091 = MAX_HANDLES (4096) - the agent's 5 other handles (CT_FILL_ENDPOINTS), and the fill row is ok in every run. 102/251 ms describe the kernel before K12 items 3 and 5; no run on this base can reproduce them.

## Fix round 2 (red B1 L1 L2, editor P1-P3), 2026-10-02
- B1: take_notices now uses one receive bounded by deadline + 5 s (no 200 ms poll). L1: the victim counts calls per lease and expects CT_CALLERS for each (bit 256). L2: the README says two leases are live at every deadline's end; H is alone at its destroy.
- Re-sweep, one seed at a time (it ran before the parallel advice), 32/32 PASS: GATE1-sweep4-logs/. Notice p99 24716-26197, one group. Worst per target, seed 13 on both widths: notice rv32 26197 / rv64 25794; R10 22517 / 22380; lease end 30889 / 30251. Driver p99 at most 11113.
- seed 13 stays pinned. README's table and text are rewritten, and the old 35 ms group is explained as the poll.
- scheduling.md at seed 13: 25,794 net / 95,579 gross / 1,158,114 audit on rv64; 26,197 / 97,703 on rv32. Negatives: audit-unstamped rv64 84,710; audit-billed 44,761 / 45,577 (GATE1-neg-*.out).
- budgets.md: 4,091 confirmed (MAX_HANDLES - 5). 102/251 ms are pre-K12 history, unchanged.
- Whole bench: exit 0, 281 PASS, 1 SKIP. doccheck 0, fmt 0.
- Fold: 57704ccf6 and d8edc36a6 (the gate commit's subject reworded). Tree identical to the pre-fold tip.
- For the next sweep: run seeds in parallel, after one build. Each case and width logs to a fixed target/testbench/<case>-<arch>-smp1.log, so either 2-way across widths or separate log directories.
