# SMP2 report (smp2-implementer-3): R12's shares and the targets across harts

Branch `wp-SMP2` (worktree `.worktrees/SMP2`), 12 commits on main c2b328ea2, head 660660c52 (after the kernel red's two P2s). Nothing pushed.
Tier A. The first six commits are the predecessors' (rebased; the model and stride commits' size lines re-chained
for main's model growth: model 10564 to 10777 to 10778). Six are new or refolded:

1. `testbench, kernel: a share across harts is the kernel's charges, judged against water-filling`:
   HART-SHARE (part = the marked budget's charges net of its harts' lock waits; whole = every charge net of all
   waits; want = water-filling at the trace's harts, every want stated); CHARGED-SHARE across harts; SHARE
   (count-based) removed; trace: `H` carries the runner's weight, `u` UNCAPPED (stride hook `uncapped`); every share
   program converted (counts are notes); release twins keep their counts at one hart; `deadline-flood-billed-traced`;
   `sched-budget-churn-shell` split out; restatements (ties, wake-no-preempt); keep_smp with reasons;
   `sched-latency` head start 600 ms and smp [1, 2]; `gate_harts` (targets gated to N harts, recorded above).
2. `kernel: the large trace ring is 256 MiB, so the containment gate judges two harts`: ring 65536 frames kept as
   u32 page numbers (the 64K-entry usize table overflowed the kernel's 1 MiB RAM region); compile-time assert that
   the physmap's last page number fits u32 (memory.md states the bound: physmap ends at 128 GiB on rv64); 576 MiB;
   `@<harts>` on HART-SHARE and CHARGED-SHARE; the bystander judged `@1`, `threads-exit` judged `@1`; seed 4
   re-pinned; README's 2-hart sweep table.
3. `testbench, stride: the model's capped scenarios run on the machine, and a floor counting the capped fails them`:
   `sched-capped` at 2, 3, 4 harts (late join and uncap @2, second cap @3, spread @2 and @4); `Role::SpinThreads`.
   Kernel red P2 1: `sched-capped-holds-floor`, a must-fail twin at 2 harts on a kernel whose floor counts the capped
   budgets (stride feature `capped-holds-floor`, its queue audit broken the same way, kernel feature with a boot
   line the case expects): late-b 52 (rv64) / 56 (rv32) of 1000 against 250, late-c 464 / 455; passes (fails as
   required) on both widths. Size: libs/stride 839 to 841, kernel 9732 to 9734 (then 9738, 9739 in 4 and 5).
4. `kernel, testbench: lock waits take the kernel lock in ticket order under R12's costliest call`: `lock-trace`
   feature, `k` record (ticket, sections ahead; release builds carry neither), oracle checks ticket order;
   `sched-lock-contention` (2 harts) and `-4` (4 harts), `Role::SearchHammer`; driver wake recorded (ruling (a)).
5. `kernel: the termination telemetry prints only with debug-print` (ruling): from 2f7196679; nothing read it.
   `sched-latency-tcg` keeps one hart: the kernel's kill line (read by budget-destroy-kills) still interleaves with
   program lines under plain TCG at 2 harts (residual "The console has two writers"; ruled (a); CONW1 un-keeps it).
6. `testbench: worst-walk's reconcile bound is a seed sweep's worst plus a tenth` (ruling): 10000 µs.

## Tests and gates (commands through scripts/q and scripts/jobs.mk from a detached snapshot of the head)

- Host: `cargo test -q --release -p redoubt-stride` 0 (27 tests); `cargo test -q -p redoubt-model --release --test
  properties --test current_contracts` 0; `REDOUBT_MODEL_MUTATIONS=R12 ... --test mutations` 0; `cargo test -q -p
  testbench` 0 (166, later 168 with the new oracle tests: 51 sched_oracle); `cargo +nightly fmt --check` 0;
  `cargo test -q -p redoubt-doccheck` 0.
- Gate at 38867b2c5 (before the rulings' changes): each case at its own harts, both widths, via jobs.mk: 116 of 117
  (worst-walk rv32 reconcile 8942 > 8900, see 6); `--smp 2` and `--smp 4` runs of 29 cases a width: 55 of 58 (sched-exit-
  churn rv32 threads-exit 448, sched-latency-tcg twice: rulings 1 and 2).
- Rerun at the squashed head (1425f26fd): smoke set and the changed cases at their own harts and at 2 (and lend at 4):
  all pass, including sched-capped 2/3/4, sched-exit-churn 1/2, kernel-containment 1/2, sched-latency 1/2.
- Final at 8b61ad9c6 (the head less doc-only lines in README.md and scheduling.md), via jobs.mk prebuilt: docs,
  no-cruft, unsafe-budget, size-budget, the smoke set on both widths at 1 hart and at 2 (lend-untouched-page at 4 too),
  sched-latency-tcg (1 hart), worst-walk on both widths (bound 10000): 22 of 22 and 12 of 12 pass. Doccheck at
  157b0b56a: pass.
- Size budget at every commit (per-commit pass in a second snapshot): all pass; kernel 9719 to 9737 across
  commits 1, 2, 4, 5 with their `Size budget:` lines; libs/stride 837 to 839.
- Sweeps: kernel-containment 16 seeds at 2 harts per width (every target met; bystander bimodal 445/545 rv32,
  478-515 rv64); sched-latency 16 seeds at 2 harts per width (32/32 pass); worst-walk 8 seeds per width at the head.

Attack cases and why the verdict is the system's: `sched-capped` and every share case are judged by the oracle from
the kernel's trace alone (charges, lock waits, harts), never from a program's count; `sched-lock-contention`'s
order verdict is the kernel's own ticket records; its driver wake is the RTC's time.

Kernel red P2 2: scheduling.md's "Fair kernel entry" residual names the blind spot: the share judge subtracts each
lock wait from the budget the hart's runner record names, so a wait billed to the wrong budget, or none, passes it,
until the waits are short enough (step 5) to judge shares without taking them out.
After the P2s: sched-capped 2/3/4 and the twin, both widths; stride (27) and oracle (51) host tests; docs,
unsafe, no-cruft; size budget at each changed commit: see the final member_update.

## Numbers (pages hold them)
- Shares at 1 and 2 harts: scheduling.md "Responsiveness" table. Latency at 2 and 4 harts (seed 3) and the 2-hart
  sweep's worst: same section. At 4 harts every p99 meets; rv32's N=16 driver p50 17.2 ms (target 15) is recorded.
- Lock waits per mille at 2 harts: 84 to 210 in the share cases, 229/280 in containment.
- sched-lock-contention: 2 harts driver p50/p99 18.5/19.2 ms rv64, 18.3/19.1 rv32 (recorded; two 9.6 ms searches per
  wake, interrupts reach only the boot hart: IRQ1); 4 harts 4.3/59.3 and 7.1/86.9 ms; order held on 1696-2362 waits.

## Documentation checked
Updated: docs/kernel/scheduling.md (R12 statement and status, attacked-three-ways, R78 + lock contention,
Responsiveness status/shares table/2- and 4-hart tables, residuals: fair entry with lock-wait numbers and IRQ fix,
console two writers, keep table with classes and 2-hart coverage, "Measured on QEMU", trace feature lists, per-hart
rewording of one-runner sentences), docs/kernel/timer.md (each hart's timer and slice end), docs/testbench.md
(HART-SHARE/CHARGED-SHARE/`@`/`gate_harts`/`lock-trace`/ring, status lists), docs/kernel/memory.md (page-number
bound), docs/kernel/README.md (containment at 1 and 2, ring, 2-hart sweep, seed 4, feature list, residual),
docs/SECURITY.md (R12 and R78 rows), docs/plan/m2-usable-shell.md (Progress), docs/kernel/model.md (variant count).
Checked, no change: README.md, GETTING-STARTED.md, docs/README.md (no hart or share claims); m2 step 3 ("a budget's
share is judged across harts as on one", still true).

## Not done / open
- VM case: cut as BEAM18. Device interrupts on every hart: IRQ1. One console writer: CONW1.
- keep_smp list for SMP4 to un-keep: sched-budget-churn-shell, deadline-flood-billed-traced, sched-wake-no-preempt;
  `@1` lines: containment's bystander, exit-churn's threads-exit.
- The oracle does not replay the cap set and floor at each pick on its own (it checks wakes against the floor and
  every pick's rank); the model and the stride differential prove the cap set.
- sched-capped at 3 and 4 harts: lock waits 300-400 per mille, so one-thread shares move with placement; judged only
  where each scenario is about.
