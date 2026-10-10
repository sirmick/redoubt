Handoff from small-fixes-implementer (2026-10-07).

## BEAM13 (assigned, in progress): branch wp-BEAM13 in /home/mcloonan/redoubt/.worktrees/BEAM13, from main 56a77fcf3. Uncommitted; gates were running (background) at handoff.

Finding: a console reader that exits leaves input queued; the VM's idle returned at once on non-empty cons.input/eof_pending, a hot loop.

Fix (written, uncommitted):
- userland/otp/vm/src/platform.rs: new trait method `console_listening(&mut self, _listening: bool) {}` (default no-op).
- userland/otp/vm/src/vm.rs: `System::set_console_reader(Option<Pid>)` sets console_reader and calls platform.console_listening only when "is there a reader" flips. Used at poll_console's Eof arm and process exit (was `console_reader = None`). poll_console now binds `let read = self.platform.lock().console_read();` BEFORE the match, or the Eof arm deadlocks on the platform lock.
- userland/otp/vm/src/bif/info.rs: console_subscribe uses set_console_reader(Some(pid)).
- userland/otp/redoubt/src/lib.rs, ConsoleIo:
  - `listening: bool`, starting TRUE (existing tests call console_read without the hook; no input arrives before the first read anyway).
  - ConsoleIo::read() sends no read while !listening.
  - idle's early return counts input/eof only while listening.
  - Platform::console_listening sets the flag and re-arms a read if started.
- Tests:
  - redoubt/tests/console.rs: `input_nobody_reads_holds_no_idle_and_waits_for_the_next_reader`. One key "y" (one key cannot split into two completions); the verdict is idle(Some(deadline)) returning >= deadline. Shown to FAIL with the old idle condition.
  - vm.rs tests: `the_platform_hears_the_console_reader_come_and_go`. A System-level setter test; Bundle has a `listening` counter. `spawn` of a BIF (beamlet:console_subscribe or erlang:apply) gives undef in that test VM, so there is no real process.
- docs/userland/beamlet.md: console paragraph sentence (held for the next reader, idle does not wake); trait table row `console_listening`; status list gains both tests, (11) to (13).
- nightly fmt done on beamlet-redoubt and beamlet-vm.

Gates launched: beamlet-redoubt host tests through q --quiet (log /tmp/BEAM13-redoubt.log), beamlet-vm tests (/tmp/BEAM13-vm.log), prebuilt, docs, formatting, size-budget, unsafe-budget, no-cruft, rv64 beamlet-console, beamlet-boot (/tmp/BEAM13-gates.log).

Next:
- Read results.
- Raise a size ceiling if size-budget fails: a `Size budget: <crate>: <reason>` line in the commit, ceiling in tests/size-budget.toml.
- Commit by path: the six files above. Message style `beamlet: ...`, with a Co-Authored-By line.
- Write .wash/local/BEAM13-report.md.
- member_update assignment_results complete (id 3375f38f85ba67411f1150fa48b7c375), for the beamlet red c27d790b. Tier B.

## Worktrees
- .worktrees/B17: a detached worktree at main, recreated because it is the SESSION'S CWD. Removing it breaks the shell; leave it.
- .worktrees/BEAM13: in progress.
- Others (B24, B25, B9, B8, B20, B21, B22) are merged; the orchestrator removes them or asks.

## Lessons
- B25: for Redoubt's loopback sshd, ssh::redoubt now creates sshd.log empty up front; before, it existed only once ssh started the ProxyCommand. A short-deadline case raced ssh's start.
- B24: a traced kernel writes a `C` record last: id = kernel ticks, pass = charged, entry field = audit ticks. The oracle prints 'nobody N of 1000 (kernel K ticks, audits A, charged C)'. A measured bill that contains an audit double-counts; K28 fixed destructions, and B24 fixed bill_irq (irq_audits_open accumulator, debug only).
- A kernel file is Tier A even when feature-gated. Prove the default kernel unchanged with llvm-nm -S --size-sort plus a size diff.
- sched-latency and the other icount cases are deterministic per binary; any added instruction moves the N=16 tails.
- Pages may not carry package IDs (doccheck C4).
- Every package's gates include size-budget, unsafe-budget, no-cruft.
- An edit after prebuilt makes case targets fail "stale index"; rerun prebuilt. Never edit while a gate run reads the tree.
- Build before committing (once committed a shadowed name that did not compile).
- Use q / jobs.mk only; never bare cargo; never push.
