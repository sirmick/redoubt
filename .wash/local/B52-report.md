# B52 report (b48-implementer, 2026-10-14)

Worktree /home/mcloonan/redoubt/.worktrees/B52, branch wp-B52 from main 9d680b781. Tier B,
tests only.

## Instance 2: the two tests wait on the event they mean

**The race.** B47's `servers_bound_and_ended_one_after_another_never_run_out_of_waiters` ended
each server, idled until the hub had let its waiter go (`p.waiters()` back down), then read the
VM's handle count once as that iteration's figure and compared the nine. The waiter's last act,
after its final hand-over is taken, is to close the handle it woke the VM through, on its own
thread: a read between the hand-over and that close sees one handle more. The first read set the
expectation for the rest, so an early first read failed the test (`[17, 16, 16, ...]` under
./test-shell's full suite beside B48's thousand launches), and an early later read would too.
B48's window test ended the same way (a sampled count, a 1 s poll).

**The event.** Two events end an iteration: the hub lets the waiter go (the VM has taken its last
hand-over: `p.waiters()` is back to its start, which the VM reaches only by idling, so the test
idles until it is, bounded at 5 s of guest time, and asserts it), and the waiter closes its wake
handle (a `handle_close` in the fake). The fake gains `Fake::wait_held(pid, handles, timeout)`:
it waits on the fake's `changed` condvar until the process holds exactly `handles`, and
`HandleClose` now notifies that condvar. The expected count is not sampled: it is what the VM
holds before the first bind, when nothing is in flight; each server adds exactly the waiter's
handle and ends with it closed, so the count returns there (asserted each time, with the count
held on failure). The test also asserts each server's waiter started (`waiters + 1`).

**B48's two tests**, folded in after train 25 (main 146db8897 carries B48; wp-B52 rebased onto
it, one conflict in tests/system.rs where B48's settle loop met this rewrite, resolved to the
event-wait). `a_served_servers_binding_and_handle_go_with_its_budget`: the terms are dropped
before the server is stopped and the count read then, when nothing of the VM's is in flight (the
budget went with the destroy, the send right with its last term, the job's handles with its exit);
after the stop the test idles until the hub has let the waiter go, asserts it, then
`wait_held(settled - 1)`: the waiter's one handle closed. The figure every later pipeline must
match is that first `settled - 1`, derived, not sampled (B48 had sampled it once the count held
still for 50 ms). `a_forgotten_connections_requests_reach_no_later_holder_of_its_index`: the same
two waits at its end replace a 5 s poll.

## Instance 1: the 2-in-30 failure on main

**The hunt.** `./test-shell` thirty times through `q run --cores 2`, on pristine main 9d680b781 in
a scratch worktree under `$REDOUBT_TMP/B52/main` (a first loop in the B52 worktree was stopped:
its tree had my edits from its second run on). Each run's failures grepped into
`.tmp/B52/main-summary.txt`; logs `main-N.log`, never read whole. Beside it: SCHED4's timing
step, and for the first runs my ten beamlet-redoubt suites.

**What test-shell reruns.** Its `on_fake_kernel` stage (beamlet-redoubt's tests) is skipped when
the binaries it runs are byte-identical to the last pass (`target/test-shell-fake-kernel` stamp),
so after run 2 only the Elixir stages ran again: the 30 runs are 2 of the Rust stage and 30 of
the ExUnit stages. 2 failures in 30 runs, as the node says:
- run 1, `on_fake_kernel`: `servers_bound_and_ended_one_after_another_never_run_out_of_waiters`,
  the early-sample race of instance 2 (fixed there).
- run 6, `on_beamlet` (the ExUnit suite on beamlet): two `Redoubt.Shell.DriverTest` cases,
  "Tab completes" (`assert text.(terminal, ~r/alpha\.txt +alps\.txt/)` false) and "a crash report
  of a process a line spawned reaches the terminal as visible text" (`assert text =~ "raw
  ^[]52;c;aGk=^G<U+202E><9B><<FF>>"` failed; the frame held `raw ` and the three earlier reports).

**The race.** The tests' `screen/2` feeds the driver's drawn bytes until the given pattern holds,
then returns once 300 ms pass with nothing more drawn (10 s if the pattern never holds). Both
tests waited for a part of what they then asserted: "alps.txt" and then asserted the whole
listing `alpha.txt  alps.txt`; "raw " and then asserted the escaped controls after it. A report
or a listing is drawn in more than one write, and under load (beamlet's logger formatting a
report with a non-UTF-8 reason; the completion listing) the rest came more than 300 ms after the
part: the assertion read a frame mid-draw. The same shape as instance 2: a sample where an event
was meant.

**The fix.** Each wait is for the pattern the test asserts: `screen(terminal,
&text.(&1, ~r/alpha\.txt +alps\.txt/))`; `shows(terminal, ~r/pwned.*evil/)`,
`shows(terminal, ~r/Task #PID<[\d.]+> .*terminating/)`, `shows(terminal, "boom")`,
`shows(terminal, "raw ^[]52;c;aGk=^G<U+202E><9B><<FF>>")`. The asserts stay, now on frames that
hold what they assert; a frame that never does fails at the 10 s bound as before.

## Summaries checked

The tests keep their names, so no status line moves. Pages that name them:
docs/userland/files.md:155,181 and docs/userland/beamlet.md:921-922 (B48's two, in status lines
and in the handles passage, as what they prove: the binding and the handle go with the budget;
a stale request reaches no later holder), docs/userland/beamlet.md:1156 (the waiters test, as
what it proves: an ended server's waiter gives its place up and the ninth is served as the
first). What each proves is unchanged; how it waits is the test's own. GETTING-STARTED.md:258's
"Tab completes" is the feature, not the test. README.md, docs/userland/README.md,
docs/servers/README.md: nothing names these tests. The docs checker holds the status lines.

## Branch

wp-B52 on main 146db8897, three commits, not pushed, clean (head f1d59bb4e):
- 9ba9356cb beamlet-redoubt, fake: the waiters test waits for the handle close it means, not for a
  count sampled around it (`Fake::wait_held`; `HandleClose` notifies; the three tests)
- 0e03c7f2d shell: two driver tests wait for the frame they assert, not for a part of it
- f1d59bb4e test-shell: its header says which stage it skips (the fake-kernel stage, when its
  binaries and fixtures are byte-identical to the last pass), and its usage how to run it again
  (`rm userland/otp/target/test-shell-fake-kernel`); steward-red-2's P3. GETTING-STARTED:238
  lists the stages without the claim; unchanged. docs PASS at f1d59bb4e.

## Gates (through q, beside SCHED4's timing step)

- `cargo test -p beamlet-redoubt --features fake --test system` on the rebased tree (9ba9356cb's
  code): 10 of 10 runs, 22 passed each (30.7-36.0 s), `.tmp/B52/suite10b.txt`; before the rebase,
  on 9d680b781 + the waiters fix: 10 of 10, 20 passed each.
- `mix test test/redoubt/shell/driver_test.exs` on the BEAM at 0e03c7f2d: 31 passed, rc 0.
- `make docs` at 0e03c7f2d: PASS, rc 0.
- `./test-shell` ten times at 0e03c7f2d, the beamlet-redoubt stamp removed before each run so
  every stage reruns: 10 of 10 "every stage passed", rc 0 (`.tmp/B52/shell10.txt`, logs
  `shell-N.log`; no run printed the Rust stage's "skipped" line; each run beamlet 361 passed,
  BEAM 292 passed 70 skipped, beamlet-redoubt 22 passed).
- `rustfmt +nightly --check` (fake, tests), `mix format --check-formatted` (driver_test.exs),
  `git diff --check`: clean.
- Not run: the whole bench (tests only, no case or page changed).
