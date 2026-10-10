# SMP4 report (smp4-implementer, 2026-10-09)

Branch `wp-SMP4` (`.worktrees/SMP4`), six commits on main 864a2ac82 (IRQ1 and PIPE1 merged), folded, no WIP:

1. `3c34638f2` kernel, testbench: a traced kernel records each section of its lock, and the oracle says what a lock wait waited behind (`hold-trace`, `h`/`j`; both contention cases carry it)
2. `582917b1f` kernel: a frame is zeroed in one checked store loop, not a check a word (P0)
3. `d689ff869` kernel: every scan of a whole table a checked build makes is an audit (P1; plus: no audit before the clock runs — the boot-time account audit had stamped garbage spans of thousands of seconds)
4. `9f22cd610` kernel, testbench: a hart's wait behind another hart's audit is billed to no one (P2, `y`, `audit-wait-billed`)
5. `a5e8b54f2` kernel, testbench: a program's fault prints one line, not its address space's map (P3, `fault-report-bound`)
6. `49028c0a8` tests, docs: the cases that keep one hart say what keeps them now (exit-churn threads-exit stays `@1`, reason restated)

Size budget chains 9938 → 9982 → 9983 → 9995 → 10046 → 10048. Unsafe count unchanged.

## Short gate (all exit 0, both widths; run on a5e387664, the smoke set and own cases again on 864a2ac82: 49 PASS, 0 FAIL, logs `.tmp/SMP4/logs/gate2/`)

docs, formatting, size-budget, unsafe-budget, no-cruft, host-tests; smoke set userland-boot,
init-boot, bench-net-peer, ipc-outcomes, sum-clear, lend-untouched-page (1 and 4 harts); own and
affected cases: fault-report-bound, sched-exit-churn (1 hart; and `--smp 2`: 502/467, then 502/465), sched-lock-contention,
sched-lock-contention-4, sched-budget-churn-shell, deadline-flood-billed-traced,
sched-wake-no-preempt (1 hart, kept), sched-latency (1, 2), smp-boot (2, 4), smp-shootdown,
smp-evict, smp-fence, smp-lock-wait, irq-boot-hart-only. Logs `.tmp/SMP4/logs/gate/`. Not run: the
whole bench; kernel-containment in the gate config (it passed at 2 harts in the table runs below).

Recorded negatives: `fault-report-bound` on the old report fails (837 / 865 ms against 15 ms);
`audit-wait-billed` on sched-budget-churn-shell at 2 harts reads 329 / 339 (pre-IRQ1 base).

## Before / after on IRQ1, two harts, `hold-trace` in both (logs `.tmp/SMP4/logs/{tb,ta}`)

"Before" is main + commit 1 only. rv64 / rv32.

| Case | Longest section net of audits, ticks (before → after) | Shares (before → after) | Waits per mille, excused |
| --- | --- | --- | --- |
| sched-budget-churn-shell (kept) | 86860 / 85840 → 18769 / 17771 | 330 / 364 → 387 / 509 | 399/386 → 389/252, 86 / 78 % excused |
| deadline-flood-billed-traced (kept) | 83738 / 84314 → 15637 / 16271 | 875,885 / 881,882 → 413,402 / 516,515 | 108/105 → 370/321, 89 % excused |
| sched-exit-churn | 158159 / 162457 → 25785 / 29351 | threads 503 / 470 → 441 / 483 | 147/220 → 316/268 |
| sched-budget-churn | 89053 / 89486 → 28340 / 30587 | all pass both | 130/129 → 105/102 |
| sched-share | 85005 / 85931 → 17045 / 17854 | all pass both | 130/195 → 141/196 |
| sched-timer-flood | 87324 / 88033 → 19231 / 20112 | all pass both | 187/190 → 223/205 |
| kernel-containment | (not split) | bystander 513 / 445 → 463 / 442 (`@1`) | 226/280 → 233/286 |
| sched-lock-contention | 720360 (the setup's unmap) both | driver wake p50/p99 8551/10610 → 8436/9189 µs rv64; 8528/10453 → 8217/9093 rv32 | 167/177 → 226/217 |
| sched-lock-contention-4 | same | 4366/49814 → 3990/59482 rv64; 5377/58836 → 5381/41034 rv32 | 487/483 → 476/503 |
| sched-wake-no-preempt (kept) | | fails both, before and after | |

The longest sections fall 4-6x everywhere but the contention cases (their worst is the one-off setup
unmap of a full area, 72 ms, and the 9.4 ms search, untouched by SMP4).

## Two things the reviewers must see

1. **P2 changes the schedule, not only the billing.** An excused waiter keeps its slice (its end
   moves by the excuse), so it stays on its hart and meets the next audit; waits behind audits grow
   (deadline-flood rv64: 11.7M → 40.1M ticks, 89 % excused). On IRQ1, deadline-flood rv64's share
   moved from the high mode (875) to the low one (413); before IRQ1 the same case swung 407-884 on
   main. It stays kept, so no gate changes, but it is a direction a reviewer may not want.
2. **sched-exit-churn threads-exit at two harts is near its bound.** Across builds of this branch:
   482/480, 505/478, 468/463, 502/467 (gate config, IRQ1), and 441/483 with `hold-trace` (IRQ1). The
   un-keep holds in the case's own config, by 17-52 per 1000 on rv32 and down to −9 under the extra
   trace records on rv64. The case at two harts runs only under `--smp 2` sweeps.
