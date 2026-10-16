# STEWARD3A handoff (steward3a-implementer-2, 2026-10-16, checkpoint at 70 % context)

## Branch state

- `wp-STEWARD3A` at **2d831801b**, ONE commit on **794449d21** (STEWARD3Q accepted, over 3R and 3S; main's plan commit is 6713d6ffa). Tree clean. Worktree `/home/mcloonan/redoubt/.worktrees/STEWARD3A`. The rebase is COMPLETE; all three review folds (two P1, one P2) are IN the commit. The commit message (`.tmp/STEWARD3A/commit-msg.txt`, also in `git log -1`) states the review folds, the rebase resolutions, the size-budget sum rule and five `Size budget:` lines (libs/steward 3942+155 = 4097, model 11417, servers/steward 1576+202 = 1778, servers/init 2591+33 = 2624, libs/wire 3915+28 = 3943).
- `target/prebuilt` is STALE (built at 04b3be8cc before the folds; its last `make prebuilt` was killed). Run `make -f scripts/jobs.mk prebuilt` before any case.
- Report so far: `.wash/local/STEWARD3A-report.md` (head/gates sections describe f04dc870e and the rebase to 04b3be8cc; the folds and the final gates are NOT yet in it). Questions/rulings: `.wash/local/STEWARD3A-questions.md` (Q1–Q4), `STEWARD3A-Q1-ruling.md`, `STEWARD3A-Q2-ruling.md` (Q3, Q4 appended). QA thread `STEWARD3A-lease-files` (Q4, answered).

## Traps (both implementers')

1. **Never edit the tree while prebuilt or a case runs**: prebuilt cases fail "tree changed". Ignored files (`.tmp/`) are fine.
2. `make -f scripts/jobs.mk set CASES=...` runs BOTH widths; `WIDTHS=` is not a jobs.mk variable. Use `make -f scripts/jobs.mk rv64/<case>` for one width.
3. Run dirs `target/testbench/run-*` are pruned fast: copy a case's `*.log` right after it ends (never grep `*`: the `.img` is 63 MB). The job logs `target/jobs/<w>-<case>.log` hold only the verdict line.
4. The `size-budget` case reports ONE crate over its ceiling at a time; the host run `cargo testbench --exact size-budget` (no prebuilt, ~20 s) iterates quickly; it also demands every raise be named by a `Size budget: <crate>:` line in the commit that raised it (incl. the predecessor's and 3Q-inherited ones).
5. `mix format` in `libs/steward/elixir` has no `.formatter.exs`: running it on a file reformats the whole file with defaults — DON'T; the oracle isn't formatter-governed. `userland/shell` is (`mix format --check-formatted`).
6. `cargo fmt` is `cargo +nightly fmt -p <crate>` (stable rustfmt ignores the repo's options).
7. `beamlet-agent-footprint` stages `tests/data/beamlet-footprint/stage/in` (= `keep hello`) and binds the data volume root at `/project` (`bind=/project=walfsd:data`); its manifest is shared with `beamlet-footprint`. The memory scan fires right after the last `expect` line.
8. `steward-end-lease` expects the `[lease alice/agent-1 ended]` line BEFORE `{:ended, {:ok, %{}}}` (the batch sends the notice before the reply). The bench stamps matched lines only, not the typed line.
9. The sponsored session inherits alice's ACCOUNT (CreateBudget names none): its home session is in alice's walfsd bucket. walfsd/littlefsd `in_flight` is 6 (3Q); the admission derivation host test (`tools/testbench/src/case.rs the_servers_admit_what_a_manifest_lets_one_account_hold`) derives a sponsored principal's cap from the sponsor's budget AND the sponsor's label sets (init's `holds`).
10. `Steward::ended(pid)` (drive.rs): a process whose object the core forgot (end_lease, deadline) keeps its name for the exit line; the bin prints the line and raises no event.
11. A lease's `/project` is minted BELOW the home carve WITH `agent_quota` (1 MiB) in the lease's scope (3R). `walfsd` makes a 0-quota sub-root a 0-byte root (NoSpace): never mint below with quota 0.
12. The model's reach table: `LAST = [(274, 148), (296, 151)]` in `model/tests/steward_reach.rs`, re-recorded with `cargo test --release -p redoubt-model --test steward_reach -- --ignored --nocapture reach_table` (prints "<family>: N reached ... the last at seed S"); model.md's two sentences carry the same numbers. Any new table row moves it.
13. `lease.trace` keeps a `StartAgent badge=302` (a lease's badge) as the taker of the new `!caller_session` row; its other agents start from badge 102. `run-traces` fails on `UNTAKEN by the hand-written traces:` lines; the 59 `never reached by the model's traces` lines are informational and pre-existing.
14. The agent `Echo` has verbs `echo`, `keep TEXT` (echo then idle; the footprint case), `spin`, `sub MS`; `finish` prints `agent: result written` on the console the VM has (a lease's has none).

## Review folds (steward-red-3, Merge verdict OK with notes) — ALL DONE in 2d831801b

- P1 notice forgery: `Redoubt.Shell.Notices` accepts badge 1 (`@notices`) only; `line/4` takes the badge; `test/redoubt/shell/notices_test.exs` (3 tests: steward's badge prints, badge 0 and 2 print nothing, another message prints nothing). Shell `mix test`: 311 passed, 70 skipped.
- P1 rebase: `connect(owner, slot, scope)` merged (3R's scope + my owner); the lease's connections and the /project carve are minted in the lease's scope (`fresh_carved(.., below, below_quota, scope)`); the derivation test follows init's `holds`; steward.md's "until R41's scope lands" clause removed.
- P2 nested lease: guard `caller_session` (guards.rs, oracle guards.ex, gen registry, row `| - | StartAgent | !caller_session | - | refuse |` first in `tables/lease.md`, generated files regenerated with `cargo run -p redoubt-steward-gen`), core test `a_lease_cannot_start_an_agent`, `lease_bounded_and_carve_lease` no longer nests, steward.md "Leases" says a lease's badge is told `unknown`, lease.trace rewritten, reach table re-recorded.

## Host gates at 2d831801b (all green)

core 38+6+18, gen 10, trace 6, steward server 2+4+29, init 66, wire 35+2+2+4, testbench 213, beamlet-redoubt 83 (userland/otp, `--features fake`), shell 311, oracle run-traces rc 0 (22 ok, no untaken), model families steward_policy+noninterference 2 pass (12.5 s), mutations PolicySponsored* caught, reach table reproduced, docs PASS, size-budget PASS (host, committed tree), formatting PASS, rv64/rv32 checks of steward server + init clean, model release build clean.

## The four lease cases on the rebased tree: NOT YET RUN for real

At the pre-rebase tree with a stand-in `in_flight: 4` (not committed) all four passed on rv64: steward-lease-ends 9.7 s, steward-end-lease 13.8 s, steward-sub-agent-dies-with-agent 25.8 s, agent-login 9.7 s (`.tmp/STEWARD3A/verify3/`, `agent-login-standin/`). On the rebased tree (3Q's real in_flight 6) a run at 04b3be8cc was started and killed for the folds; nothing recorded. rv32 of the four never ran.

## Next steps, exactly

1. `export PATH="$HOME/.cargo/bin:$PATH" BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains RUSTSBI_PROTOTYPER=... RUSTSBI_PROTOTYPER_RV32=... REDOUBT_TMP=/home/mcloonan/redoubt/.tmp`; `make -f scripts/jobs.mk prebuilt` (both widths, ~8 min).
2. The four cases for real, both widths: `make -f scripts/jobs.mk rv64/steward-lease-ends` etc. (one at a time; copy `$(ls -td target/testbench/run-*/|head -1)*.log` to `.tmp/STEWARD3A/final/<case>/` right after each). Then `make -k -j4 -f scripts/jobs.mk set CASES="beamlet-agent-footprint steward-three-contexts steward-ssh-two-principals shell-compile init-boot image-disk docs size-budget formatting"`. Expect: lease-ends ~10 s, end-lease ~14 s, sub-agent ~26 s, agent-login ~10 s per width; agent footprint peaks 1993/2083 (size 5248 holds).
3. Report: update `.wash/local/STEWARD3A-report.md` (head 2d831801b, the folds, the four cases' timings both widths, the set's results, logs under `.tmp/STEWARD3A/final/`). `member_update` complete (≤2000 bytes, head and the four timings first); steward-red-3 renews by range-diff (base 794449d21).
4. If a case fails: `agent-login` or the lease cases failing with `(:eio)`/no result means an admission or quota wall again — check walfsd's `limits` (6) and that `agent_quota` reached the steward line (`steward: carved ...`/`sizes ... agent_quota=1048576` in init's args).

## Where every log is

`.tmp/STEWARD3A/`: `cases6-rv64-rv32.log` (first set), `gates-rv64-rv32.log` (head-level gates at 2254d114d), `gates-head2/3.log`, `exp*/` (the Q4 probes: `exp11/probes.txt` the Eio timeline, `exp12/` the NoSpace), `verify/ verify2/ verify3/` (lease cases under the stand-in), `agent-login/` (the Eio failure), `agent-login-standin/` (PASS), `oracle-run2..5.log`, `prebuilt*.log`, `commit-msg.txt`.

## What consumed my context

Finding the lease's file failure (a dozen prebuilt+case cycles with probes, each ~10–25 min; the first hypotheses — OOM, threads as processes, the hub — were wrong; the answers were walfsd's per-account `in_flight` and the 0-quota sub-root), the Q4 ruling's build (core line field, init, steward, image, pages), the re-measure, then the rebase onto 3Q/3R (nine conflicts, the bin's 3R rewrite) and the three review folds with their ripples (generated files, oracle, reach table, trace rewrite, derivation test).
