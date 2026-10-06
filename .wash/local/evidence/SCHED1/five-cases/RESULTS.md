# SCHED1 five-cases measurements (2026-10-06)

Sources:
- the consoles here (`*.console.log`, SHA256s in `sha256.txt`);
- `run-diag.sh`, which ran them;
- `attribute.py`, which reads them.

The scratch `SLICE_US` edit is in `scratch-slice-edit.diff`. Its revert is verified:
`scratch-slice-after-revert.diff` and `scratch-status-after.txt` are empty, and the worktree is
clean.

## Release build: per-switch cost and useful work, 1 ms against 10 ms

The release builds have `debug_assertions = false`, no trace and no audits. The case is
`sched-large-weight`: a weight-1000 server against eight weight-100 users, all spinning, over a 2 s
window. The guest prints the server's count over (its calibrated alone rate x the window).

| Width | Slice | Calibrated alone rate (iterations/ms) | Server share printed |
| --- | --- | --- | --- |
| rv64 | 1 ms | 21701 | 464 (FAIL, wants 505) |
| rv64 | 10 ms | 24619 | 537 (ok) |
| rv32 | 1 ms | 7362 | 455 (FAIL) |
| rv32 | 10 ms | 8747 | 533 (ok) |

- **The server's useful work** (share x rate) at 1 ms against 10 ms: 0.762 on rv64, 0.718 on rv32.
- **Fraction of the CPU that is user work at 1 ms**: f = 464 x 21701 / (555 x 24619) = 0.737 on
  rv64, and 0.690 on rv32. This takes the 10 ms rate as the alone reference.
- **Kernel time per 1 ms slice**, a switch between budgets:
  - rv64: 1/f - 1 = 0.357 ms, about 44,600 instructions at icount shift 3 (125 per µs);
  - rv32: 0.449 ms, about 56,000 instructions.
- **Re-picking the same budget** (the calibration, alone) at 1 ms costs:
  - rv64: 24619 / 21701 - 1 = 0.134 ms per slice;
  - rv32: 0.188 ms per slice.
- The release build has no trace, so no interrupt-gap median can be taken for it. The cost above is
  derived from useful work.

**Conclusion:** the per-switch cost is not checked-build or trace overhead. The release build
loses as much per switch as the checked one (0.35 ms on rv64).

## Checked build decomposition (walk-trace, rv64, `walk-large-weight`)

| Part | Value |
| --- | --- |
| Median gap between slice-end interrupts | 1460 µs per 1 ms slice |
| Reconcile walks | 4897 in 1541 slices (about 3.2 per slice); median 72 µs, mean 86 µs. About 0.23-0.27 ms per slice: most of the kernel time |
| Audits | 1649, median 75 µs (about one per slice) |
| Expiry | 2 walks only: no expiry work at a plain slice end |
| Remainder per slice | about 0.1 ms: traps, 2 SBI timer calls, pick, switch, about 17 trace records |

The reconcile (K22's, `settle` plus `reconcile`, timed by `walk-trace`'s RECONCILE span) costs about
9,000 instructions each. That is far more than its loop bounds suggest (see
`switch-cost-estimate.md`), and it is the item to explain next. A candidate is
`marks.settle -> ready_now -> ready_count()` over every thread slot of each marked process.

## large-weight attribution (traced, checked, rv64; `attribution-large-weight.txt`)

- **Picks**: server (budget 31, weight 1000) 763; each user (weight 100) 77-78. That is 10:1, the
  weight ratio. Ranks are in order. There are no one-tick charges.
- **Charge per run**:
  - each user about 8865 ticks (887 µs: the slice less overhead);
  - the server, median 11619 ticks (1162 µs).
  - Correction, 2026-10-06 (implementer 6), from `decompose.py`: the difference is an artifact of
    `attribute.py`. It counts the pass change from a `K` to its `R`. The kernel time billed to a
    budget after its own deschedule ("kmain's pick and switch after a deschedule are the descheduled
    budget's", scheduling.md, Charging) falls inside that span only when the budget is re-picked
    straight after itself. The server is re-picked so most of the time; a user never is.
  - Every pass rise (`decompose.py`) gives each budget's whole charge: the server 1026.9 ms
    (555/1000 of all charges), each user 102.7-104.4 ms (55-56/1000). All charges cover 917/1000 of
    the window; audits take 53/1000.
- **Audits inside the window**: 107 ms of 2018 ms (5.3%).
- **User time**: server about 763 x 886 µs = 676 ms; users about 68 ms each, 546 ms together. The
  server's share of the useful work is 676 / 1222 = **553/1000**, the 555 R12 asks.
- **So R12's relative share holds.** The fixture fails because it measures throughput against the
  alone rate: about 40% of the window is kernel and audit time at 1 ms.

## server-busy, carve-inflation, debt-lift attribution (traced, checked, rv64; implementer 6)

How these were made:
- `attribute.py` gives picks, `K`-to-`R` charges and audits in the window.
- `decompose.py` gives every pass rise per budget (the whole charge) and splits the slice-end gap.
  Its median columns are "interrupt to marks audit", "audit", and "audit end to next interrupt
  less 1 ms".
- Budget ids and weights come from the trace's `K` and `v` records.
- In every case below, each budget's whole charge is its weight's share to within 1/1000. There are
  no one-tick charges, and the picks are in rank order (the oracle PASSes).

### server-busy (server 32, user A 33, users B 34 and C 35; all weight 100)

| | server (32) | A (33) | B (34) | C (35) |
| --- | --- | --- | --- | --- |
| Picks in the window | 265 | 1042 | 387 | 387 |
| Share of all charges | 258/1000 | 229/1000 | 257/1000 | 257/1000 |

- The window is 2997 ms. All charges take 585/1000 of it.
- **Audits take 286/1000** (858 ms; 882 `U3` IPC-list audits among 3225, about 0.5 ms each at A's
  sends and the server's replies).
- About 129/1000 is neither, which is idle or unbilled entry time.
- The slice-end gap median is 1288 µs: 115 + 37 audit + 137.
- **Finding:**
  - By charge, R12 holds. The server stays within its quarter (258), and B and C keep theirs (257
    each, at least 250).
  - The guest's 194 is throughput against the alone rate. About a quarter of each charged slice is
    kernel time (see the reconcile finding), and in the checked build 29% of the window is IPC-list
    audits.
  - Picks are not a share measure here: A and the server block per call. The ratio of their
    charges is the measure.
  - The guest's server figure (67, from 132 calls) counts iterations inside wall-clock 2 ms work
    spans that preemption interrupts. It is not the server's CPU.

### carve-inflation

| Phase | Victim picks | Carver's subtree picks | Victim against carver, by count | Victim's share of charges | Guest |
| --- | --- | --- | --- | --- | --- |
| Depth 1 (U 31 free 100, C 35 100, V 32 200) | 795 | 797 | 499/1000 | 500/1000 | 458 |
| Depth 4 (U 40 free 100, C1 44 free 50, C2 47 free 25, C3 50 free 13, C4 53 12; V 41 200) | 740 | 746 (370 / 187 / 94 / 49 / 46) | 498/1000 | 496/1000 | 410 |

- Audits take 2.4% (depth 1) and 3.9% (depth 4) of the window.
- The gap medians are 1257 µs at depth 1 (103 + 30 + 124) and 1352 µs at depth 4 (139 + 51 + 162).
  The extra kernel time per slice at depth 4 matches its six queued budgets against three.
- **Finding:** carving moves share and never duplicates it (R7 holds by counts and by charge). The
  guest's 410 is throughput against the alone rate, at a larger per-slice kernel cost with more
  budgets queued.

### debt-lift (16 spinners 31..76 step 3; U 79, weight 100 -> 99 after the carve; G 82, weight 1; sibling S 85)

The round, computed from the trace per the Architect's ruling:
- S woke for its window (the `go`) at entry 2050, at about 1091.76 ms.
- The next 16 picks were the launcher (1) and the 15 spinners not picked since S's start: 76, 73, 70,
  67, 61, 64, 58, 52, 55, 49, 46, 31, 40, 34 and 37. Spinner 43 had run at entry 2043.
- S was picked at entry 2100, at about 1119.5 ms. That is **27.8 ms after its wake, behind exactly
  one pick of each other runnable budget: one round of 17**.
- S's wake was not placed behind G's raw debt. The lift is recorded at entry 1944 (`L`/`l`/`A`), and
  the oracle checks it by the rule.
- The interrupt-to-interrupt gap in that stretch is 1711 µs per 1 ms slice (median over the case:
  1676 = 259 + 129 audit + 289).
  - 16 x 1.71 ms is 27.4 ms. The guest's 30.6 ms adds the launcher's work between S's creation and
    the `go` (S's start and its 0.6 ms `U3` audit, at entries 2038-2049).
- The fixture's bound, (N + 1 + 2) x SLICE_US = 19 ms, assumes no kernel time per slice.
- **Finding:** in the trace's own terms the claim holds (one round). The miss is the per-slice kernel
  time, 0.68 ms at L = 17 in the checked build. It grows with the number of queued budgets (the
  reconcile finding). It is not a scheduling error.
- U (79) has 16 picks before its destruction; G has none in this window (it ran before it).

## The reconcile's cost and count (implementer 6)

See `reconcile-finding.md`. In short:
- The candidate `settle -> ready_count` is not the cost.
- `Queue::raise_floor` walks every queued budget's 7-word `State` through checked `kframe` reads. It
  runs twice, unconditionally, in every `Queue::reconcile`, and once each in `fold` and
  `deschedule`. With `Queue::pick`, that is about 9 queue walks per slice end.
- The kernel time per slice end is linear in the queued budgets: about 23 µs per budget checked,
  about 28 µs per budget in release.
- About 3.2 spans run per slice end: two `leave`s (the timer trap's exit to kmain and the switch
  ecall's exit to the thread) and kmain's `pick`. In 94% of slices that is exactly 3.
