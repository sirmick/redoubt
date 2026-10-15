# B55 report

Branch wp-B55 in .worktrees/B55. Base db6575a3f (main), head a945746eb. Two commits:

- a2b4d239b testbench: an Elixir case's build runs before its deadline starts
- a945746eb tests, docs: the Elixir oracles build outside their 10 s deadline

Scope as the orchestrator ruled (option A, answer 872f51e9): tools/testbench/src/{elixir,case}.rs,
which makes B55 Tier A for that part. Steward-red reviews.

## What the 46 s deadline measured

`libs/steward/elixir/run-traces --break not_locked`, each step timed from a cold target on 4 idle
cores under q (job b55-cold, 29.0 s in all):

| step | s |
| --- | --- |
| build-beamlet (RE_ENGINE=rust, release, userland/otp/target) | 16.1 |
| `cargo run` of redoubt-model's steward-traces example (debug, root target/) | 11.2 |
| elixirc | 0.8 |
| on_beamlet | 0.5 |
| steward-trace check | 0.2 |

Built, the same script takes 1.3 to 1.7 s. Neither the prebuild nor anything else built these
artifacts before the case's clock started, so the deadline mostly measured cargo. The whole-bench
logs agree (.tmp/train-*/bench*.log):

| train | broken-guard | elixir-oracles |
| --- | --- | --- |
| 15 | 46.2 s, FAIL | 31.4 s |
| 18b | 44.9 s | 15.9 s |
| 17b | 16.7 s | 2.7 s |
| 20 | 1.8 s | 3.1 s |

So the time follows whether the train touched what these cases build. Both cases build the same
targets at once, so one waits on cargo's lock for the other. Alone run 1 below shows it: each
reports 9.2 s of build, one of them spent waiting. No class change helps, because every class
would still build inside the clock.

## The fix

- `Elixir.build` (case.rs): commands run before the scripts, outside the deadline. Each must
  exit 0. A failure is `build <cmd>: <status> (the whole output: <log>)`, never a timeout. It
  carries no script output, so it cannot match a `must_fail`. The log records `# the builds took
  X s` and `# the scripts took Y s`. The commands run inside the bench process, under the case's
  own q lease (jobs.mk unchanged; both cases stay in the bounded class).
- One build list: both tomls name `userland/otp/tools/build-oracles` (new). It runs build-beamlet
  with RE_ENGINE=rust, then the two `cargo build`s exactly as run-traces runs them (one `-p` each,
  so the feature sets match). Checked: after `cargo clean -p` of model, steward-trace, steward
  and beamlet, build-oracles took 9.1 s, and then run-traces' own build steps took 0.1 s each.
- Deadline 10 s in both tomls, quoted there. Built, the scripts take 2.5 s alone (elixir-oracles)
  and 1.3 s (broken-guard), so 10 s is four times elixir-oracles' alone time. Beside load they
  peak at 3.6 s and 1.6 s.
- Host test `testbench::the_deadline_starts_after_the_build`: a 0.8 s build with a 0.5 s deadline
  passes; a 0.8 s script after it fails "ran past the case's deadline of 0.5 s"; a build exiting 3
  fails "build broken-build: exit status: 3".

## Runs (scripts/runs: .tmp/B55/runs.sh; run 1 of each series after `cargo clean -p` of the built crates)

Both cases ran at once through `make -f scripts/jobs.mk set CASES="elixir-oracles
bench-elixir-oracles-broken-guard"`, from `cargo testbench` (no prebuilt).

| series | run | elixir-oracles build / scripts | broken-guard build / scripts | verdicts |
| --- | --- | --- | --- | --- |
| alone (other members' prebuild and two boots on the machine) | 1 | 9.2 / 2.5 | 9.2 / 1.4 | PASS PASS |
| | 2 | 0.1 / 2.5 | 0.1 / 1.3 | PASS PASS |
| | 3 | 0.2 / 2.6 | 0.1 / 1.3 | PASS PASS |
| loaded: two prebuilds (scratch worktree at main + NSCR2's), all 20 general cores leased | 1 | 0.1 / 3.1 | 12.0 / 1.6 | PASS PASS |
| | 2 | 0.1 / 3.0 | 0.1 / 1.6 | PASS PASS |
| | 3 | 0.1 / 3.6 | 0.1 / 1.6 | PASS PASS |
| loaded: init-boot and sched-share on both widths in a loop, B57's prebuild, K33's release test build | 1 | 0.1 / 2.6 | 11.5 / 1.6 | PASS PASS |
| | 2 | 0.1 / 3.0 | 0.1 / 1.3 | PASS PASS |
| | 3 | 0.2 / 2.7 | 0.1 / 1.4 | PASS PASS |

Scripts alone, built, 3 repetitions each (job b55-warm): broken 1.31/1.33/1.33, run-traces
1.31/1.30/1.32, run-vectors 1.24/1.20/1.20. Beside two prebuilds: 1.68/1.69/1.68,
1.66/1.70/1.62, 1.50/1.48/1.43.

## Gates (exit codes)

- `make -f scripts/jobs.mk docs`: PASS, rc=0
- `rv64/formatting`: PASS, rc=0
- `rv64/host-tests` (includes testbench's unit tests): PASS, rc=0
- `q run --cores 4 -- cargo test -p testbench elixir`: 3 passed, rc=0
- elixir-oracles and bench-elixir-oracles-broken-guard: 9 runs each, all PASS (above)

Not run: the whole bench; rv32 (these cases have no arch); unsafe/size budgets (no unsafe and no
budgeted crate touched).

## Summaries checked

- docs/testbench.md: the bounded-class prose (an Elixir case's build runs under its lease, outside
  its deadline), the heavy-case table row, the `elixir` kind's fields row (`build`), and the
  Elixir oracles section (the build, the deadline from its end, why). Status gains the new host
  test. All updated.
- scripts/jobs.mk class comment ("the Elixir oracles (they build beamlet): 4 cores"): still true,
  so not changed.
- docs/servers/steward.md (lines 82-83, 302, 386), docs/servers/wire.md, docs/kernel/ipc.md,
  docs/SECURITY.md: they name the cases, not their deadlines or builds. No change needed.
- userland/otp/tools/env.sh comment (versions pinned in tests/*elixir-oracles*.toml): still
  true. userland/otp/README.md lists the everyday tools, not the bench's helpers. No change
  needed.
- README.md, GETTING-STARTED.md: no mention of these deadlines. No change needed.

## Notes and risks

- The case log now carries cargo's replayed warnings twice: once from build-oracles and once
  from the scripts' own `cargo run --quiet` (753 warning lines). broken-guard's log is 474 KB.
  The build's stderr is kept on purpose, since a build error must be in the log.
- The reported case time (the PASS line) still includes the build. Only the deadline excludes it.

## Round 2 (steward-red, OK with notes on a945746eb)

New head 41f5609e6 (eb3956cf1 testbench, 41f5609e6 tests/docs). Range-diff against a945746eb:
.tmp/B55/range-diff-r2.txt. The code changes are amended into the testbench commit. The docs
change is amended into the docs commit, which introduced the paragraph it edits.

- P2: the builds run under `timeout` with `BUILD_SECS` = 600 s together (elixir.rs; a cold build
  takes 29 s alone and has taken 46 s beside a bench). Past it the case fails "build <cmd>: timed
  out: the builds' deadline is 600 s". docs/testbench.md's Elixir oracles section and the field
  doc in case.rs state the bound.
- P3: a failed build, a late build and a script past the deadline are `Err`. main.rs's Elixir
  branch maps Err to Fail("bench error: ...") without calling judge(). A script's own failure
  stays `Ok(Some(why))`, the only outcome a `must_fail` can match.
- Test `the_deadline_starts_after_the_build`: its case's `must_fail` is "", which matches any
  text. A slow build with a quick script passes. A failing script is `Ok(Some)`, so it is judged.
  A late script, a broken build ("build broken-build: exit status: 3") and a build past a 0.3 s
  build bound (via run_within) are all Err, so main never judges them.
  `the_scripts_share_the_case_deadline` now expects the timeout as Err.

Gates: `q run --cores 4 -- cargo test -p testbench`: 188 passed, rc=0. docs rc=0.
rv64/formatting rc=0. rv64/host-tests rc=0. rv64/elixir-oracles PASS 3.4 s (build 0.2,
scripts 3.1), rc=0. rv64/bench-elixir-oracles-broken-guard PASS 1.7 s (build 0.1, scripts 1.4),
rc=0.
