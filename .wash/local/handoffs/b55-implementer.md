B55 handoff (b55-implementer). Branch frozen, accepted for train 22.

## Branch state
- wp-B55 in /home/mcloonan/redoubt/.worktrees/B55, base db6575a3f (main), head 41f5609e6, clean tree. Two commits:
  - eb3956cf1 testbench: an Elixir case's build runs before its deadline starts (tools/testbench/src/elixir.rs, case.rs)
  - 41f5609e6 tests, docs: the Elixir oracles build outside their 10 s deadline (tests/elixir-oracles.toml, tests/bench-elixir-oracles-broken-guard.toml, userland/otp/tools/build-oracles (new, 0755), docs/testbench.md)
- Reviews: steward-red OK at a945746eb with notes P2/P3; take two at 41f5609e6 renewed OK, nothing open.
- Report: /home/mcloonan/redoubt/.wash/local/B55-report.md (round 1 + "Round 2"). Range-diff r1->r2: /home/mcloonan/redoubt/.tmp/B55/range-diff-r2.txt.

## Traps
- Never `pgrep -f` a pattern you typed: my wait loop matched its own shell (I stopped it with TaskStop).
- The scratch load worktree ($REDOUBT_TMP/B55/load) is already removed. Scratch scripts in .tmp/B55: env.sh, steps.sh (times each run-traces step), warm.sh, runs.sh (both cases x3, run 1 after cargo clean -p), load.sh (prebuild then a boots loop until .tmp/B55/stop-load exists).
- testbench's package name is `testbench`, not redoubt-testbench. beamlet is in the userland/otp workspace with its own target (userland/otp/target), not the root target/.

## Design
- Cause: an elixir case's timeout_secs covered the scripts, and the scripts did their own cargo builds: build-beamlet (release, RE_ENGINE=rust) 16.1 s cold plus `cargo run` of redoubt-model's steward-traces example 11.2 s, on 4 cores. The oracle itself is about 1.5 s. Nothing built these before the clock started, and the two cases build the same targets at once, so one waits on the other's cargo lock. Trains 15 and 18b hit 46.2 s and 44.9 s; trains that touched nothing they build ran in about 2 s.
- Build before the clock: `Elixir.build: Vec<String>` (serde default) holds commands run before the scripts, under the case's own q lease (they run inside the bench process; jobs.mk unchanged; both cases stay in the bounded 4-core class). The scripts' deadline counts from the end of the builds. The log records "# the builds took X s" and "# the scripts took Y s".
- One shared build list: both tomls set `build = ["userland/otp/tools/build-oracles"]`. That script runs RE_ENGINE=rust build-beamlet, then `cargo build --locked -p redoubt-model --example steward-traces` and `cargo build --locked -p redoubt-steward-trace` as two commands, matching run-traces' feature sets so its own builds are no-ops (0.1 s, checked after cargo clean -p).
- BUILD_SECS = 600.0 (elixir.rs const): the builds together run under `timeout`; past it the case fails "build <cmd>: timed out: the builds' deadline is 600 s". It bounds only a hung build. run_within(…, build_secs) exists for the test.
- Err-vs-judge rule: a failed build, a late build and a script past the case's deadline return Err; main.rs's Elixir branch maps Err to Fail("bench error: …") without calling judge(), so no must_fail can match them. Only a script's own nonzero exit is Ok(Some(why)) and is judged. main.rs was not changed.
- Deadline: timeout_secs = 10 in both cases, four times elixir-oracles' 2.5 s alone time (broken-guard is 1.3 s alone); quoted in each toml.
- Tests: testbench::the_scripts_share_the_case_deadline (now expects Err) and the new testbench::the_deadline_starts_after_the_build (must_fail ""; slow build passes; failing script is Ok(Some); late script, broken build and a build past a 0.3 s bound are Err). docs/testbench.md's Elixir oracles status lists both.

## Measurements (build s / scripts s; both cases at once via jobs.mk set)
- Cold, steps on 4 idle cores: 29.0 s total (beamlet 16.1, model example 11.2, elixirc 0.8, on_beamlet 0.5, check 0.2).
- Scripts alone, built: broken 1.31-1.33, run-traces 1.30-1.32, run-vectors 1.20-1.24. Beside two prebuilds: 1.68-1.69, 1.62-1.70, 1.43-1.50.
- Alone x3: oracles 9.2/2.5, 0.1/2.5, 0.2/2.6; broken 9.2/1.4, 0.1/1.3, 0.1/1.3. In run 1 both cases show 9.2 s of build: the lock wait.
- Beside two prebuilds x3: oracles scripts 3.1, 3.0, 3.6; broken 12.0/1.6, then 1.6, 1.6.
- Beside boots on both widths, a prebuild and a release test build x3: oracles 2.6, 3.0, 2.7; broken 11.5/1.6, then 1.3, 1.4. All PASS.
- Round 2 gates: cargo test -p testbench 188 passed; docs, rv64/formatting, rv64/host-tests rc=0; elixir-oracles 3.4 s (0.2/3.1), broken-guard 1.7 s (0.1/1.4).

## Known and left
- The case log carries cargo's replayed warnings twice (build-oracles plus the scripts' cargo run): about 474 KB. The reviewer said to leave it.
- The PASS line's time still includes the build; only the deadline excludes it.

## Next
Nothing. Train 22 carries it; the orchestrator ends this session after the push.
