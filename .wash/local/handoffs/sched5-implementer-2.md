SCHED5 handoff (sched5-implementer-2, 2026-10-21). Worktree /home/mcloonan/redoubt/.worktrees/SCHED5, branch wp-SCHED5, base origin/main 820d5e5ff. Head ea9666366, tree clean. Never push.

## Branch state
- ae1acdd6f rt: range calls go a range at a time, and a long run is placed in one pass
- 5de5048a9 stub: a segment is mapped, protected and unmapped a range at a time
- 89b380da4 model: a range over MAX_RANGE_PAGES is TooLarge, after its shape and before any walk
- 8106fb191 kernel: no range call holds the kernel longer than about a slice (reviewed head after round 2: red OK, simplifier OK, editor OK with notes, folded)
- ea9666366 WIP rt, client: the launch tests count a batch's range calls. FOLD INTO ae1acdd6f before any review. It touches libs/rt/src/lib.rs (abi re-exports range_chunks) and libs/client/tests/launch.rs. Only commit 1 touches those files, so folding = checkout --detach ae1acdd6f, apply the WIP's diff, amend, cherry-pick 5de5048a9 89b380da4 8106fb191, then git branch -f wp-SCHED5 HEAD; git checkout wp-SCHED5. No interactive rebase. Commit 1's message should then also say the launcher's tests count a batch's range calls.
- Local branches sched5-r0 (edab94595, reviewed round-1 head) and sched5-pre-rebuild: keep until acceptance, never push.

## TRAPS FIRST
- STALE index.lock: bench runs (the prebuilt fingerprint's `git diff HEAD`) leave an empty /home/mcloonan/redoubt/.git/worktrees/SCHED5/index.lock. Before every commit: `pgrep -x git || rm -f /home/mcloonan/redoubt/.git/worktrees/SCHED5/index.lock`.
- Env every command: PATH=$HOME/.cargo/bin:$PATH BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper REDOUBT_TMP=/home/mcloonan/redoubt/.tmp
- Never edit or move HEAD while prebuilt cases run ("tree changed"). Re-run `make -f scripts/jobs.mk prebuilt` after any commit, before cases.
- Long runs: launch with `setsid nohup script > out 2>&1 < /dev/null &` and watch with a run_in_background until-loop. A backgrounded `&` inside a Bash call gives no notification.
- Load runs (>4 jobs, quiet slots, -mttcg host-clock cases) need the orchestrator's go. Quiet slots: start nothing on q.
- Size budget: every commit that raises a ceiling needs a `Size budget: <crate>: <reason>` line. Current ceilings: kernel 10856, stub 364, libs/sys 1048, libs/rt 4189, model 11415. Per-commit values: commit 1 libs/rt 4189 + libs/sys 1045; commit 2 stub 364; commit 3 model 11415; commit 4 kernel 10856 + libs/sys 1048. Helper: /home/mcloonan/redoubt/.tmp/SCHED5/setsize.py (base origin/main).
- Docs gate is the bench case (`make -f scripts/jobs.mk rv64/docs`). C5: a first citation in a passage reads `R22 (range cost)`.
- Formatting: `cargo +nightly fmt` at the ROOT (running it in libs/rt misses the fake crate).
- Sweep lesson: grep for raw over-cap range calls missed five test programs in round 1. Search by evaluating constants, not by pattern.

## Train 91 failures (orchestrator, to fix now)
1. **client-host-tests** (log /home/mcloonan/redoubt/.tmp/train-91/fail-client-host-tests.log): libs/client/tests/launch.rs asserted 64-page process_map batches; rt now splits each into 16-page calls. FIXED in WIP ea9666366, keeping the claims: a helper `moved_in(dst, &[batch lens])` builds the expected range calls with range_chunks; the image / stack / streamed / largest-stack tests compare whole call lists; the refusal test refuses the third batch's first range call (refuse_after 1 + two_batches.len()) and checks the child holds stub + two batches and the launcher's pages are as they were; the failed-read test expects 1 + first-batch calls. `cargo test -p redoubt-client --test launch`: 11/11 pass.
   STILL TO DO: the full host-test sweep. The 31 crates linking redoubt-rt or redoubt-fake-kernel (cargo metadata): redoubt-beamlet-programs blkd bootfsd client consoled consrelay crossing erofsd erofsd-programs fake-kernel fileserver init init-programs ipd keyd littlefsd littlefsd-programs net-client net-tests netd piped rt sshd sshd-host steward-server verityd walfsd walfsd-programs stub test-programs testbench. Run the bench host-test cases covering them: blkd-host-tests client-host-tests consrelay-host-tests erofsd-host-tests fileserver-host-tests host-tests init-host-tests ipd-host-tests littlefsd-host-tests net-host-tests netd-host-tests piped-host-tests rt-host-tests sshd-host-tests steward-host-tests walfsd-host-tests (rv64/<case> via jobs.mk; >4 jobs = ask the go, or run in groups of 4), plus rt-miri, and report which ran.
2. **rv32 pipe-sixty-four**: reproduces alone, 277 s and EMFILE at pipe 39 vs ~17 s before (logs /home/mcloonan/redoubt/.tmp/train-91/). NOT YET INVESTIGATED. Suspect a slower range call per piped launch (each launch's batches now 4x the process_map calls; the stub's segments a range at a time), or rt map_anon's filler search under fragmentation (each Buffer > 16 pages now does map_anon + map_fixed extension, and a too-short gap retries). Start: read the case (tests/pipe-sixty-four.toml) and the console from .tmp/train-91; compare per-launch time; check whether piped/its clients allocate Buffers > 16 pages; a trace boot (hold-trace) of rv32 pipe-sixty-four could show the long sections or the call counts. EMFILE at the 42nd/39th pipe could be a timeout cascade from slowness, not a real fd limit.

## Done (both rounds folded; see .wash/local/SCHED5-report.md, sections through "Fold round 1")
- Round 1: red P2-1/P2-2/P3-1..3, simplifier range_chunks, editor history, Architect rulings (device-owner exception; process_map untouched overrun 1.4-1.8 slices).
- Round 2: editor P2/P3s; head 8106fb191; docs rc 0, formatting rc 0; diff --stat in .wash/local/SCHED5-diff-r2.txt.
- Boots at b49e2477c/1986b1764: the 21-case set 40/42 then the 12 affected cases 24/24, all green.

## Next
1. Run the host-test sweep (above) at ea9666366, fix anything else.
2. Investigate rv32 pipe-sixty-four (ask the orchestrator for slots for timing boots).
3. Fold the WIP into commit 1 (see above), prebuild, docs/formatting/size gates, report the new head with `git range-diff` against 8106fb191, and append a train-91 section to .wash/local/SCHED5-report.md.
