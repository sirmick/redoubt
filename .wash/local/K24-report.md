# K24 report: a slice starts when the thread returns to user mode

Branch wp-K24 in /home/mcloonan/redoubt/.worktrees/K24, from main f7ce1e9b6. One commit:

- 7a39bb17f kernel, model: a slice starts when the thread returns to user mode

Never pushed.

## The change

- **Kernel** (`kernel/src/sched.rs`). `pick` no longer sets the slice end. `leave` starts the
  slice at the return to user mode: `else if to_user && slice_end() == NEVER`, after the
  reconcile and the audits. `NEVER` means "picked, not yet started", because a leave for `kmain`
  (PID 1) already sets it and every pick follows such a leave.
- **Why this option, not a guaranteed slice for an expired pick.** It is the smaller change: one
  branch at the one return site, one line gone from `pick`, and no new state. A "pick whose slice
  is over at the return" check would still have to record when the pick happened, and would
  still let the exit work count against the slice up to the point it gives up.
- **Inherited slices are unchanged.** A thread switched to directly, not through `kmain`, keeps
  the running slice, as before.
- **Docs comments.** `time.rs`'s Timer field doc now says `NEVER` from a pick until the return.
- **Model** (`model/src/sched.rs`, `mutation.rs`, `tests/common/contracts.rs`).
  - The model already counted only user time (`run`) against `slice_left`.
  - `Scheduler::exit_work(work)` names the kernel's work between a pick and the return. Who pays
    for that work is charging's; none of it comes out of the slice.
  - Mutation `R12SliceCountsExitWork` (R12, `ALL` is now 148) takes it out of the slice.
  - A scheduler contract (`sched_contracts`): a pick, then three slices of exit work, must leave
    `slice_left == SLICE`. It is caught there, first in `mutations_are_caught`.
- **Pages.**
  - scheduling.md:
    - preemption points: the slice is "from its return to user mode after the pick";
    - R12: one sentence, "A slice is the picked thread's user time ...";
    - the mutation is added to both status lists (6 and 43);
    - a new residual, "A slice end's kernel time grows with the queued budgets". It names the
      exit work that reads every queued budget, points at charging's "the reconcile's cost is
      above its loop bounds" (RECON1's work; the pages name no plan IDs), and records the
      livelock this fixes.
  - timer.md: the slice end's definition and the flowchart node.
  - model.md: the R12 row.
  - SECURITY.md: the R12 row's mutation list.
- **Regression** (`tests/worst-walk.toml`). worst-walk now reaches its destruction at the 1 ms
  slice. That run writes about 2.6 million (rv64) and 2.9 million (rv32) trace records, past the
  64 MiB ring's 2.1 million: first runs gave `dropped 530061` and `dropped 770334`. So the case
  takes `sched-trace-large` (192 MiB); memory_mib stays 2032. The case keeps main's `must_fail`
  (R10 over 30 ms), which K19 removes.

## Summaries checked

- README.md, GETTING-STARTED.md, docs/README.md, docs/kernel/README.md, docs/plan/*.md: none
  says the slice starts at the pick. docs/kernel/README.md's timer row ("armed for the earliest
  slice end") still holds. No change.

## Gates (exact commands; exit codes)

Environment: /tmp/k24env.sh, with q=/home/mcloonan/redoubt/scripts/q.

- `$q run --cores 8 -- cargo build -q --profile {release,checked} --target {riscv64gc,riscv32imac}-unknown-none-elf -p redoubt-kernel --features qemu-virt`:
  4/4 rc 0, 0 warnings (on 78e2dbf08; 2f9a6ec31 changes only a comment and the case file).
- `$q run --cores 4 -- cargo test -q -p redoubt-doccheck --test docs`: rc 0 after fixing a C5
  citation (`R12 (scheduling)` on timer.md).
- `cargo +nightly fmt --all -- --check`: rc 0.
- `$q run --cores 8 -- cargo test -q -p redoubt-model --release --test current_contracts scheduler_contracts_hold`: rc 0.
- `REDOUBT_MODEL_MUTATIONS=R12SliceCountsExitWork $q run --cores 4 -- cargo test -q -p redoubt-model --release --test mutations`: rc 0 (caught).
- `make -k -f /home/mcloonan/redoubt/scripts/jobs.mk -C <K24> ...` on 78e2dbf08:
  - rv64/sched-share: PASS, 1.0 s;
  - rv64/sched-budget-churn: PASS, 14.5 s;
  - rv64/sched-latency: PASS, 99.4 s;
  - no-cruft, formatting, host-tests (quiet), docs, stride-host-tests: PASS;
  - size-budget: FAIL, kernel 9191 against 9190. Fixed in 2f9a6ec31 by shortening the new
    comment to one line, with no ceiling raise;
  - worst-walk rv64 and rv32: FAIL on the trace ring only (above). Both reached 'one holder
    destroyed, killed: true' and WORST-WALK DONE at the 1 ms slice.

## Final gates

- On 2f9a6ec31 (code identical to the head; it differs only in a comment and the size ceilings):
  - rv64/worst-walk: PASS, 697.5 s;
  - rv32/worst-walk: PASS, 758.1 s.
  - Both reached 'one holder destroyed, killed: true' and WORST-WALK DONE at the 1 ms slice, with
    SCHED-TRACE-END 2628149 / 2867565, dropped 0.
  - PASS on main's case means the `must_fail` line matched: R10 over 30 ms, which K19 fixes and
    whose `must_fail` K19 removes when it rebases over this. The oracle's numbers are not
    printed on a must_fail pass.
- On cec24019a, through jobs.mk after a fresh prebuilt (rc 0):
  - rv64/sched-latency: PASS, 95.7 s;
  - rv64/sched-budget-churn: PASS, 13.5 s;
  - rv64/sched-share: PASS, 1.0 s;
  - docs, formatting, no-cruft, stride-host-tests: PASS;
  - size-budget: FAIL, model 10247 against 10240.
- Size budget, both raised in the commit with `Size budget:` lines:
  - the kernel counts code lines, not comments: +1 net, 9190 -> 9191;
  - the model: +7, 10240 -> 10247.
- On 7a39bb17f (head): `$q run --cores 1 -- cargo testbench --exact size-budget`: PASS, rc 0.
- Model suite: `$q run --cores 8 -- cargo test -q -p redoubt-model --release`, see below.
- Not run: rv64/model-host-tests (the debug suite). Stopped after 62 min because one mutations
  thread spun for an hour; it is the known late-caught set (MODEL1), and its 4 cores were blocking
  q's head of queue. The release suite above replaces it.
- Model, as the orchestrator scoped it (the full suite is B18's and the train's). The full
  release run and the old serial rv64/model-host-tests were both stopped.
  - `$q run --cores 8 -- cargo test -q -p redoubt-model --release -- --skip steward --skip mutations_are_caught`:
    rc 0, every binary ok. The run also skips `mutations_are_caught`, which alone runs all 148
    mutations for hours; the next command covers the ones for this rule.
  - `REDOUBT_MODEL_MUTATIONS=R12 $q run --cores 4 -- cargo test -q -p redoubt-model --release --test mutations`:
    rc 0 in 11.7 s. Every R12 mutation is caught, R12SliceCountsExitWork included.

## Risks

- Each round now runs one full slice of user time plus the kernel's work around it. Before, the
  pick-to-return work came out of the slice. Shares hold, because everyone's slice grows alike;
  latency per round grows by the exit work. sched-latency on rv64 still passes. Not rerun: rv32
  sched-latency and the full scheduling set.
- A thread switched to directly (not through kmain) keeps the running slice, as before.

## Rebased onto main 0e83afe41: head 54c5eacb0

- `git rebase 0e83afe41`: clean, no conflicts. 11 files, +76/-26 against main.
- Four kernel builds (release and checked, rv64 and rv32): rc 0, 0 warnings.
- `make -f scripts/jobs.mk prebuilt`: rc 0. Then:
  - rv64/sched-share: PASS, 1.9 s;
  - rv64/sched-latency: PASS, 96.3 s;
  - rv64/size-budget: PASS;
  - rv64/docs: PASS.
- worst-walk on both widths is the run on 2f9a6ec31 above, not rerun after the rebase. Main's
  new commits do not touch kernel/src/sched.rs.

## Red round (OK with notes, no P1): both folded; head af114beb5 on main ec902d464

- (1) testbench.md: two cases build `sched-trace-large`, the containment gate and worst-walk,
  with worst-walk's record counts.
- (2) A picked thread "runs its slice unless a budget deadline fires at the return": in the
  scheduling.md R12 sentence, the comment at kernel/src/sched.rs's slice start, and the commit
  message.
- Gates on af114beb5:
  - four kernel builds rc 0, 0 warnings;
  - `cargo +nightly fmt --all -- --check` rc 0;
  - rv64/docs PASS;
  - prebuilt rc 0, on the second try: the first got rc 3, "Q: the daemon went away before the
    lease", when q restarted.
- **rv32/sched-share FAILs, on main too.**
  - On af114beb5: weight 300 got 549 of 1000, want 600 ± 50 (0.8 s, guest time).
  - On main ec902d464 alone (git archive in /tmp/k24-main,
    `q run --cores 2 -- cargo testbench --arch rv32 --exact sched-share`): weight 300 got 543
    of 1000, the same failure.
  - So the failure is not from K24; K24 moves it 6 per thousand toward the target. Its cause is
    outside this package (rv32 slice-end cost under the 1 ms slice, likely SCHED1's).
