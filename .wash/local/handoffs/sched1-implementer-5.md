# SCHED1 implementer-5 handoff (2026-10-06, about 08:30 UTC)

## Branch state, traps first
- Worktree `/home/mcloonan/redoubt/.worktrees/SCHED1`, branch `wp-SCHED1`, HEAD `a787bd3d2`. The tree is clean apart from the diagnostic script's scratch files below.
- Base: IPC3's rebased tip `4cc35d84b` (wp-ipc3), which sits on main `7760bb18d`. SCHED1 merges on top of IPC3, so IPC3 first. Main has one later docs commit, `46cb2f0ca` (testbench.md only). The merge rebases IPC3 and SCHED1 onto it; nothing to do for now.
- Safety ref: `wp-SCHED1-prefold` (the pre-fold head, 39586d915 on the old IPC3 base). Keep it until the merge. Nothing is pushed. Never push.
- **TRAP, scratch kernel edit in flight.** The detached script `/tmp/s1-diag.sh` (copy at `.wash/local/evidence/SCHED1/five-cases/run-diag.sh`) temporarily edits `kernel/src/sched.rs` to `SLICE_US ... else { 10_000 }` for two release boots, then runs `git checkout -- kernel/src/sched.rs` and `rm tests/diag-*.toml`.
  - Before committing anything, check `five-cases/progress.txt` ends with `done`, `scratch-status-after.txt` is empty, `scratch-slice-after-revert.diff` is empty, and `git status --porcelain` is clean.
  - If the script died midway: revert `kernel/src/sched.rs` with `git checkout --` and delete `tests/diag-*.toml` (untracked scratch, never commit).
- Commits on the base, each with a component subject, a reason, signed off, and every file read in full:
  1. `e7a5569c7` testbench: prove the cluster, a wake that waits, and a carve's return. This is the oracle: v3 cluster parser and envelope credit, lower witness, lead-category amendment, control classification, wake-no-preempt and carve-return proofs, the red round-4 P1/P2 fix, tests.
  2. `dbca253ae` kernel: use a one millisecond scheduling slice. SLICE_US = 1_000, plus the test-only `slice-10ms` and the model; sched-wake-no-preempt and sched-carve-return adapted.
  3. `dec42bed1` tests: the cluster case and its 10 ms control.
  4. `a787bd3d2` docs: scheduling.md, timer.md, model.md, testbench.md, including the sixth sweep, the seed-3 table and the cluster results.
- **The red's round-4 P1 is DONE.** It is folded into e7a5569c7, and the body says so. Test `a_control_keeps_the_programs_own_gates`, plus the P2 tests for missing-K and double-FAIL.

## What is established (do not redo)
- **Cluster v3.**
  - Candidate PASS on rv64 (re-judged from the saved console) and rv32.
  - Control classified miss on both widths:
    - rv64: driver net 102676 µs and timer net 106472 µs;
    - rv32: timer net 69515 µs.
  - Evidence: `.wash/local/evidence/SCHED1/*result.md`, `run-v3-*`.
- **Host gates on the folded base**, all PASS: fmt, testbench (105), unsafe, size, no-cruft, formatting, docs, rv32 build, all 16 `*-host-tests`.
- **IPC3's code under SCHED1 is unchanged by its rebase.** The range-diff shows sign-offs and docs context only.
- **Sweep**: sched-latency seeds 1..20, rv64 and rv32, all 40 PASS. The numbers are in scheduling.md (the sixth sweep) and `/tmp/s1-sweep-*.log`. Do not rerun.
- **Whole bench on a787bd3d2**: STOPPED on the orchestrator's order. rv64 complete (228 targets); rv32 had 88 verdicts. Logs: `.wash/local/evidence/SCHED1/whole-bench-a787bd3d2-make.out` and `-jobs/`.
  - Failures: aio-many-reads and -two (both widths); bench-ssh-loopback, -exit and -host-key (alone class, environmental?); rv64 kernel-containment (guest exited at 247 s); sched-budget-churn ('share deadline: net 1003 of 1000'); sched-carve-inflation (409); sched-debt-lift (31.7 ms vs 19 ms); sched-large-weight (410 vs 555); sched-server-busy (195/195); sched-ties (clause 2); sched-timer-flood (stale_waits_in none); worst-walk (3600 s timeout).
  - rv32 failures seen: aio x2, ssh x3, and the budget target (probably sched-budget-churn; not checked).
- **sched-ties**, deterministic on both widths at a787bd3d2.
  - The judge (budget 1) is picked promptly but hits its slice end right after each send (I 1 -> R 1); each b runs before the next send.
  - There is about 1.2-1.35 ms of unrecorded time between the judge's pick and its send's IPC-list audit (U3, about 0.5 ms). It is unexplained.
- **Large-weight diagnosis** (checked, traced, rv64): picks are server 767 vs users 81 each (about 10:1, proportional to weight, ranks in order); audits are about 7% (1664 audits, 152 ms over 2 s); slice-end interrupt gap is 1.45 ms per 1 ms slice. That gives about 0.35 ms kernel per switch, about 30% CPU. It is not pass/accounting.
  - The figures are in `.wash/local/evidence/SCHED1/diag-1ms/README.md`. The first two diagnostic consoles were lost to run-dir rotation.
- **Code-reading estimate** of the per-switch path: 8-15k instructions, 65-120 µs, a third at most. `five-cases/switch-cost-estimate.md`.

## Rulings to follow (read whole)
- `.wash/local/SCHED1-five-cases-ruling.md` (plus its addendum).
  - ties: rebuild clause 2 as three B wakes in ONE kernel entry by construction (same absolute timeout expired by one timer entry, or one destruction), or delete the guest lines and let the oracle carry the claim, with the description restated.
  - large-weight, server-busy, carve-inflation: findings, not fixtures to adapt. No bound moves.
- Architect follow-up (message 74b26a7e), binding:
  - judge share as a ratio of counts (server over the sum, victim against carver), which is R12's statement;
  - AND report useful work per window at 1 ms vs 10 ms separately (the switching cost);
  - debt-lift's round is computed from the trace (one pick of each runnable budget); a miss in the trace's own terms is a finding;
  - (c) first: release-build per-switch cost on both widths, in µs and instructions at shift 3, and its decomposition in the checked build (SBI timer re-arm, trace ring writes, reconcile, switch).
- Addendum 7e543535: the per-switch cost is a finding of its own, a number for scheduling.md beside useful work at 1 vs 10 ms. debt-lift's 31.7 ms is about 2.7 rounds of about 11.6 ms, so the switch cost does not explain it; attribute it from the trace.
- No slice change; no fixture changes until the numbers are reported to the orchestrator and the Architect (777e51857fbe474b8e207caf6edf60f5).

## Measurement in flight (results land in .wash/local/evidence/SCHED1/five-cases/)
`/tmp/s1-diag.sh`, detached via setsid. `progress.txt` logs each step; consoles are `<tag>.console.log` and bench output `<tag>.bench.txt`.
1. traced-large-weight, traced-server-busy, traced-carve-inflation, traced-debt-lift (rv64, checked, sched-trace, oracle).
2. walk-large-weight (walk-trace): reconcile and expiry walk times for the decomposition.
3. release-1ms-rv64 and -rv32 (sched-large-weight, debug_assertions=false). The server's printed share vs the ideal 555 gives useful work.
4. Scratch SLICE_US 10_000 edit, then release-10ms-rv64 and -rv32, then revert. Diffs are recorded.

Analysis:
- `attribute.py <console> id=w,...` gives picks, charge per pick (Δpass x w / 2^20), one-tick charges, audit time in the window, share of charged ticks and share net of audits.
- Large-weight budget ids last time: server 31, users 34..55 (step 3), launcher 1; check them in the new trace.
- Per-switch decomposition from the timestamps: I (entry), U4/V4 (marks audit), next I; plus the walk-trace M/m spans.
- Release per-switch cost = (1 - useful fraction) x period; useful fraction = server share / 555 at 1 ms vs at 10 ms.
- Release has no trace, so there is no interrupt-gap median for it. Say so.

Then report to the orchestrator and the Architect, with the numbers, before touching fixtures.

## Still to do after the numbers and the rulings
1. Per the rulings: ties rebuild (or remove its guest lines); share fixtures by ratio of counts plus a separate useful-work number; debt-lift attributed from the trace. Code, tests and docs, folded into the logical commits (tests commit, docs commit), with the trace numbers in the commit messages.
2. scheduling.md: the per-switch cost and useful-work numbers, and Responsiveness's slice arithmetic restated.
3. Remaining alone reruns, held by the orchestrator: kernel-containment rv64, sched-budget-churn, aio-many-reads-two, the bench-ssh-loopback self-checks (check whether two ssh-loopback cases ran at once), worst-walk (3600 s timeout; rerun alone).
4. Whole bench once, on the final head (`make -k ... cases-rv64 cases-rv32`, detached with setsid); then the final report at `/home/mcloonan/redoubt/.wash/local/SCHED1-final-report.md`, per the 6e8393d7 assignment.

## Env and pool rules
- `eval "$(/home/mcloonan/redoubt/.wash/local/jobserver env)"` once per shell.
- PATH: `$HOME/.cargo/bin` first.
- `RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper`
- `RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper`
- `BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains`
- Unset `TESTBENCH_QEMU_SEED`.
- Cases: `make -f /home/mcloonan/redoubt/.wash/local/jobs.mk -C <worktree> rv64/<case>`. Never pass -j.
- Host work: `jobserver share cargo ...`. Exclusive host-clock cases: `jobserver all ...`. Guest boots: `jobserver take`.
- Anything longer than about 25 minutes: launch detached with `setsid nohup ... &` and wait with an until-loop in a background command. Tool background jobs die at their time limit and take their children with them.
- Copy every console out of `target/testbench/run-*` immediately. The bench rotates run dirs, and two consoles were lost this way.
- When killing, kill by exact PID or PGID, after checking `/proc/<pid>/cwd`. A pattern pgrep once matched my own shell.
- Reports and results to the orchestrator: at most 2000 bytes, with detail in files.

## Do not redo
The sweep, the host gates on the folded base, the cluster candidate and control runs, IPC3 range-diff check, the large-weight pick/audit diagnosis (its numbers stand), the fold itself.
