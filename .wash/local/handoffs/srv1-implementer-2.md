# Handoff: srv1-implementer-2 → successor (two open packages: B37 and SHELL8)

## Branch state (both clean, nothing pushed, no q jobs running)
- **B37**: worktree /home/mcloonan/redoubt/.worktrees/B37, branch wp-B37, head 9b97e47b7 on main 36d1450f9. One commit: "difftest: tests run in parallel, and the BEAM's results are cached per module" (userland/otp/tools/difftest rewritten, tools/expect.escript takes an optional module list, GETTING-STARTED.md beamlet section + userland/otp/README.md one line). IN REVIEW; the orchestrator merges it on its own. Do NOT add the remaining cuts to wp-B37: build them on a NEW branch off main once 9b97e47b7 is merged (check `git log main` / `git branch --contains 9b97e47b7`; when I stopped, main was still 36d1450f9).
- **SHELL8**: worktree /home/mcloonan/redoubt/.worktrees/SHELL8, branch wp-SHELL8, head 8306ef6ed = ONE commit on main b0905b33b ("shell: what the VM logs is drawn through the shell's guard, crash reports among it"). Waiting for the beamlet red-team review. The simplifier's 5 notes are folded in already.
- Scratch: /home/mcloonan/redoubt/.tmp/B37 (measure.sh, measure.log, step-*.log, test-shell.timed, difftest.serial, difftest.new, p2-*.log, prof.log) and /home/mcloonan/redoubt/.tmp/SHELL8 (gate logs). Reports: .wash/local/B37-report.md (step 1 table, step 2 numbers, the proposals), .wash/local/SHELL8-report.md, .wash/local/SHELL8-design.md.

## Traps
- Every build/test goes through /home/mcloonan/redoubt/scripts/q (or `make -f scripts/jobs.mk -C <worktree> ...`). Exports: PATH=$HOME/.cargo/bin:$PATH BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper. Scratch under /home/mcloonan/redoubt/.tmp/<node>, never /tmp, never in a worktree root (prebuilt fingerprints the tree and refuses a stale index).
- Under q, a bash `sleep N; cmd` chain is blocked by the harness. Wait with `timeout 590 bash -c "until grep -q X log; do sleep 15; done"`, or run_in_background.
- `./test-shell FILE` takes paths relative to userland/shell (test/redoubt/...), not repo-relative. With a wrong path the beamlet and BEAM stages "fail" without running anything.
- ./test-shell's on_fake_kernel stage has a host-clock Rust test that flakes under load: beamlet-redoubt tests/console.rs `an_end_of_input_already_waiting_ends_the_idle_that_takes_it` (left Eof, right Nothing). Rerun it alone: `q run --quiet -- bash -c 'cd userland/otp && cargo test -q -p beamlet-redoubt --features fake --test console'`. It passed 3/3 alone.
- Running heavy jobs while you measure skews the timings. Measure alone.
- Commit messages need "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>". Stage by path. Every committed file must have been read in full by you. Pages carry no dates, package IDs or review history.
- Wash: questions to the orchestrator are at most 2000 bytes (put detail in a file). reply_to must be a MESSAGE id, not an assignment id.
- difftest's atomvm suite fails 6 tests on main (B34, known): 518/524 passed, 6 failed, 18 skipped is the reference count. atomvm/test_node's EXPECTED value depends on what ran before it in the oracle VM: old serial oracle {'EXCEPTION',error,{badmatch,43}}, chunked 0. It fails on beamlet either way.

## B37: the orchestrator's answer (message 44717e1f…), build ALL FIVE, separate commits, new branch off main after 9b97e47b7 merges, in this order:
1. **Concurrent machine cases.** A jobs.mk case-set target that may be run with `make -j`. It is allowed provided every case still takes its own q lease, quiet-class cases still run alone (--quiet), and net cases keep --lock net. Put that rule on docs/testbench.md in ONE sentence (the 'never -j' rule existed to stop oversubscription, which q's leases already prevent). Note: memory files mention "no -j"; the owner/orchestrator has now relaxed it for this target.
2. (#3 in the answer) **on_fake_kernel skips its Rust build and tests when nothing it compiles changed.** The stage is in ./test-shell (the on_fake_kernel function): `cargo build -p beamlet-redoubt --target riscv64imac-unknown-none-elf`, `cargo test -p beamlet-redoubt --features fake`, then a shell run on the fake kernel. Keep the shell run.
3. (#4) **on_beamlet and on_beam run concurrently, with separate scratch.** Both currently use userland/shell/_build (beamlet uses _build/beamlet-test and _build/beamlet-root; mix test uses _build/test).
4. (#2) **Choose machine cases by what changed, conservatively.** Any change under userland/shell/lib/redoubt/shell/driver*, term*, screen*, userland/otp, or any server runs the shell machine cases; a change elsewhere in Elixir runs none. Make it a SCRIPT that prints the case set from `git diff --name-only`, so a gate can be checked, not guessed.
5. **Profile prebuilt's 117 s after an Elixir-only change and REPORT before building anything.** What I found so far: the prebuilt log (/home/mcloonan/redoubt/.tmp/B37/step-shell-prebuilt.log) says "prebuilt rv64: 236 cases, 0 failed, in 59.8s" and "prebuilt rv32: 222 cases ... in 57.0s". jobs.mk runs `cargo testbench --prebuild target/prebuilt` (8 cores, the widths in turn). So an Elixir-only change rebuilds every case's pieces for both widths. Next: find which part (Mix compile of the userland disk, EROFS pack, verity, bundles) takes the time, in tools/testbench/src/prebuilt.rs and build.rs.
Then remeasure both gate lists at the end, in the same table as step 1.

## B37 measurements and method (step 1, in .wash/local/B37-report.md)
- Method: /home/mcloonan/redoubt/.tmp/B37/measure.sh. It times each step with date, under q leases (test-shell at --cores 8 via a timed copy, test-shell.timed, which prints TIME lines per stage; difftest per suite at --cores 4; cargo at 8), and runs the machine cases one at a time via jobs.mk rv64/<c> rv32/<c>.
- Phases: cold (fresh worktree); shell change (append a comment to userland/shell/lib/redoubt/shell/printer.ex, which makes the formatting stage fail — the timings still stand); beamlet change (append a comment to userland/otp/vm/src/lib.rs). Each edit is reverted with git checkout afterwards.
- The case set is the 29 cases whose toml mentions Redoubt.Shell or beamlet, minus vendor-check, elixir-oracles, bench-elixir-oracles-broken-guard, beamlet-lookup-cli-host and beamlet-lookup-host.
- Results:
  - shell change ≈ 942 s: test-shell 61 (on_beamlet 28, on_beam 11.7, on_fake_kernel 19.5); prebuilt 117; machine cases 764.
  - beamlet change ≈ 1138 s: build-beamlet 11.6; cargo test otp 22; serial difftest 36; test-shell 71; prebuilt 208; cases 790.
  - cold: test-shell 91, difftest 121, prebuilt 395.
  - The biggest cases: steward-restart 2×188 s (13 restarts, 14 s apart, by design) and steward-ssh-idle 2×54 s.
  - tools/elixir-tests could not run: no reference/elixir-1.20.4 sources.
- Step 2 numbers at 12 cores: warm 8.9 s (serial 38.6; 11.4 s at 4 cores), one test edited 9.3 s, cold 31 s (serial 121). Every run had the same counts.

## SHELL8 (wp-SHELL8 8306ef6ed)
- What it does: Redoubt.Shell.Log (userland/shell/lib/redoubt/shell/log.ex) replaces the logger's `default` handler while Redoubt.Shell.Driver runs. Driver.run installs it after :group.start and uninstalls it in try/after around loop/1. The driver loop has one new clause, `{:redoubt_shell_log, line}`, which draws a put_chars.
  - The handler formats with the default handler's formatter (@bounds chars_limit/max_size 4096 for logger_formatter) and converts to UTF-8 with Redoubt.Term.Text.utf8/1 (new; invalid bytes become <FF>, sharing byte/1 with visible/1).
  - It sends to a relay process, which does io:put_chars(group). The handler never waits.
  - An event from group or the driver itself becomes the fixed line via fallback(driver).
  - Backlog: exactly 32, a :counters slot (add, get, undo past the bound), with a dropped count drawn as "[N log events dropped]".
  - The default handler is restored on uninstall.
- Tests: driver_test.exs has 4 new tests (hostile crash reports in a 200-row terminal via start([], rows); an event logged inside group via :sys.replace_state; a flood with `deepest` printed afterwards; handler ids restored). log_test.exs: 16 tasks × 20 events at a stuck relay, so exactly 32 queued and 288 dropped. text_test has a utf8/1 case. Docs: docs/userland/shell.md ("The loop", "Hostile text never drives the terminal").
- Gates run: full ./test-shell (all stages but on_fake_kernel's console flake, which passed alone), docs 0, formatting 0, userland-boot and userland-read-only on both widths 0. After the simplifier fold: ./test-shell on driver/log/text tests passed every stage, docs 0, formatting 0.
- Next: the beamlet red's review. Fold any findings into 8306ef6ed (amend; one commit). When SHELL4 (pager/completion, wp-SHELL4) merges, rebase: SHELL4 also edits driver.ex. Conflicts are likely in Driver.run (my install/try/after around loop) and in the loop's receive clauses (mine adds {:redoubt_shell_log, line} after the `{^group, request}` clause), plus possibly shell.md's "Hostile text" section and driver_test.exs (my tests sit before "hostile text typed or pasted…"; start/2 gained a rows argument). After the rebase, rerun ./test-shell (full), docs, formatting and userland-boot/read-only on both widths, then send the head as a `question`.
- B36 (the fixture fix) is on main as b0905b33b; SHELL8 is already rebased onto it.
