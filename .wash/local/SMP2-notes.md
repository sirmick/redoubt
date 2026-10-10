# SMP2 working notes (smp2-implementer-2)

Branch wp-SMP2 (.worktrees/SMP2): the handoff's commits 1-6, then WIP beb8ec133 (oracle water-fill),
ad623d74c, 640246720, 2d55097cc (all WIP, to fold into real commits per family).

Scripts (.tmp/SMP2/): env.sh; pc.sh <arch> <smp> <case> (from prebuilt, through q; .out and console
.log to .tmp/SMP2/pc/); table.py <smp> (verdicts and notes); shares.py <log> <share> (per-budget
charges and waits in a window); rises.py.

## Built (in the WIP commits)
- Oracle: Weights (H carries the runner's stride weight [kernel change]; G; lift child weight),
  window_records, charges() shared by CHARGED-SHARE and HART-SHARE.
  HART-SHARE `name start end tol[+|-] mark[:k] w[:k]...`: whole = everything charged in the window
  net of all lock waits; part = the mark's budget (+ lifted into it) net of its harts' waits; want =
  water_fill(judged weight from trace + declared), normalised; `+` at least, `-` at most. Each line
  also counts timer interrupts nobody's / finding another's wait ended early; stale_waits_in names a
  HART-SHARE.
- `u` record: stride hook `Budgets::uncapped(b, pass)` before the uncap lift's set_state; kernel
  trace UNCAPPED; oracle restarts the count there (without it sched-share's capped 300 was charged
  28.6M ticks in a 20M-tick window).
- SHARE (count-based, net of audits) and Bench::judged_share removed: no users left.
- wake-no-preempt proof reads each hart apart, counts by sleeper (test with 2 harts).
- Programs: share, large-weight, idle-gap, sleep-gaming, exit-churn, budget-churn, timer-flood,
  carve-return, carve-inflation, deadline-flood-billed: counts -> notes, mark(), HART-SHARE.
  mark() moved to sched.rs (pub). ties: program check "highest id last" (valid to 2 harts).
  wake-no-preempt: 4 spinners, delay a note.
- Release twins (share, large-weight, deadline-flood-billed): keep_smp, count bounds as expect
  regexes. New tests/deadline-flood-billed-traced.toml (checked: test-only features need it).
- keep_smp with reasons: cluster, cluster-old-control, carve-return, budget-churn, lift-delay,
  kernel-containment, wake-no-preempt.
- Tried and reverted: crediting audits to the hart's next runner (deadline-flood at 1 hart: audits
  1.7 s of 2 s went to the creator, victim 78).

## Results
- 1 hart, both widths: every converted case PASS (before: PASS on main).
- 2 harts PASS: share (247/247/505 rv64), idle-gap, sleep-gaming, exit-churn, timer-flood,
  large-weight rv64 (547), debt-lift, server-busy, destroy-billing, carve-inflation rv64, ties rv64.
- 2 harts FAIL / decisions (asked orchestrator 2026-10-08, defaults taken):
  large-weight rv32 572 (audits uneven across harts; residual?), budget-churn shell 326/406 (lock
  waits ~850 ms of 2 s), carve-return (premise one-hart), containment ring overflow (790k dropped),
  lift-delay (launcher capped, peers spread >1 round), wake-no-preempt (no mid-slice nap).

## Left
- deadline-flood twin at 2; ties rv32 at 2 with the new check; carve-inflation rv32 at 2.
- budget-deadline at 2 (copy without keep_smp, in .tmp, not tests/); sched-latency head start
  (300 ms -> 600 ms in sched.rs steward()), smp=[1,2], 16-seed sweep at 2, numbers at 4;
  sched-latency's N=16 1000-weight server share is still a count check; sched-latency-tcg.
- New cases: sched-capped (2 and 3 harts), sched-lock-contention (2, 4); VM case: ask to cut.
- Pages: testbench.md (oracle status list: drop shares_are_judged_net_of_audits, add
  water_filling_caps_a_budget_at_its_threads, a_charged_share_across_harts..., a_hart_share_is_judged_
  of_every_charge_against_water_filling; SHARE paragraph -> HART-SHARE; Checked builds list too),
  scheduling.md (Responsiveness share paragraph lines ~531-556 numbers; R12 attacked-three-ways;
  residual "Fair kernel entry" + "Shares across harts are judged net of these waits"; "Measured on
  QEMU" retitle; status lists add deadline-flood-billed-traced), m2 Progress, SECURITY.md rows.
- Fold WIP into family commits; size-budget at each; fmt; doccheck; gate; --smp 2 sweep; report.
