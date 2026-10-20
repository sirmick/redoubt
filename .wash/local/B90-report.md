# B90 report (b90-implementer, 2026-10-20)

Worktree `/home/mcloonan/redoubt/.worktrees/B90`, branch `wp-B90` on origin/main 24507bec1, one
commit, head `10f27228f`, clean, not pushed. Re-tiered to A by the orchestrator (the fake kernel
is shared test infrastructure); reviewer kernel-red-4. Scratch: `.worktrees/B90/.tmp/B90/`
(logs, gdb dumps, scripts; `FINDING.md` is the long form of the finding).

## What hung, and why

The hang is in `./test-shell`'s `on_fake_kernel` stage: `cargo test -p beamlet-redoubt
--features fake`, the `console` test binary (userland/otp/redoubt/tests/console.rs), on the
fake kernel's held clock. Reproduced on main 24507bec1:
- `q run --cores 4 -- cargo test -q -p beamlet-redoubt --features fake --test console` x20:
  1 stall (`main-console-2.log`: four tests "running for over 60 seconds", then the fake's stuck
  panic, the held clock at 10,140,005 us, and an abort since the panic crosses the runtime's
  `extern "C"` trampoline). WAKE1 saw the same message 1 in 10.
- The console binary alone (4 test threads) on a 1- or 2-core lease: runs 1, 2 and 3 of three
  loops stalled (`dump{d,e,f}-summary.txt`); on 4 cores 0 of 240.
- gdb thread dumps of stalled runs (`dump*-stacks-*.txt`, `varsg-stacks-2.txt` with the console
  state printed): every stall is the same. The test on the turn is
  `a_write_answered_busy_goes_again_after_the_retry_interval`; its VM thread is in `Redoubt`'s
  `Drop` (lib.rs:469), `io.wait()` for a pending console write, on a clock nobody ticks; the
  fixture server idle in `receive`, the hub waiter parked in a fresh completion call. State:
  `writing: None`, `output` 18 bytes (the whole line), `retry_at 10,150,004` with the clock held
  at 10,140,004, the server's remaining `busy` 1: only one of the three writes reached the
  server in the loop's ten ticks, its busy answer was handed over only in the last idle, so its
  retry fell one `RETRY_US` past the last tick and the drop waited for it. (The panicking thread
  in the logs is an earlier test's pool worker in a FOREVER receive: the oldest wait, not the
  cause.)

**Cause: the fake kernel's tick, not the test and not the shell.** `Fake::tick` waits for
`waiting == others`, and `Fake::wait` counted a thread from `waiting += 1` until it reacquired
the state lock after its condvar returned, which includes the window after a change has
*notified* it. A notified thread is runnable, not waiting. The VM thread never blocks in that
test's loop (`receive(0)`, then a tick that saw everyone "waiting"), so on a loaded or one-core
host it took all ten ticks before the waiter was ever scheduled to hand the answer over. B38's
rule ("a tick always ends the waits the test wrote it to end") was not what the code delivered.
The one quick failure also caught (`dumpb-console-21.log`,
`a_vm_busy_past_the_session_bound_keeps_its_console`, "the console ended while the VM was
busy") has the same shape. Neither the test's wait shape (it ticks and looks between ticks, as
B38 asks) nor the shell is involved; the shell is untouched.

## The fix (libs/rt/fake/src/lib.rs)

Every change goes through `Fake::change`: it counts the change (`State::changes`) and sets
`waiting = 0`; a thread leaving `Fake::wait` decrements `waiting` only if no change woke it
(`changes` unchanged: its deadline or a spurious wake). A woken thread counts as waiting again
only when it waits again, so a tick waits until every thread a change woke has resumed and
re-waited or ended. Bounded by construction (the Architect's condition): a woken thread either
takes the lock back or ends (`run` returns, `thread_exit`, `process_exit`, a panic), and an end
counts it out (`uncount`), so no tick waits for a thread that is not coming back. The module and
`tick` doc comments state the rule; no shipped code changes.

Tests, `libs/rt/tests/held_clock.rs` (now run one at a time on the binary's one clock, as
console.rs's `turn()` does; the two existing-and-new tests interfered when run in parallel on
main's fake):
- `a_tick_waits_for_the_thread_a_change_woke`: a server's reply wakes a client in a call and the
  test ticks at once; the client's call must return at the reply's time.
- `a_tick_waits_for_the_thread_the_last_tick_woke`: a tick ends a receive and a second follows at
  once; the thread the first woke must read the first tick's time (the orchestrator's shape).
- `a_tick_does_not_wait_for_a_woken_thread_that_ends`: the endpoint under a FOREVER receive is
  destroyed and the thread ends; the tick after it returns (the Architect's shape).
On main's fake (`.tmp/B90/main-demo`, origin/main plus the test file): the first fails 5 of 5
natively and under Miri ("the client saw the tick before its reply"), the second fails natively
(`main-demo-native-4tests.log`); the third passes there too, as a guard. On the fix: 4 of 4
natively and under Miri (`cargo +nightly miri test -p redoubt-rt --test held_clock`).

## Runs and gates (every command through scripts/q or scripts/jobs.mk)

- Console binary on the new fake: `fixed1-summary.txt` 20 of 20 on a 1-core lease (the shape
  that stalled on runs 1-3 of main's loops), `fixed4-summary.txt` 20 of 20 on 4 cores, both
  beside WRITE1's and STEWARD13's boots and my own loops; `final2-summary.txt` on the final
  head: 20 of 20 on a 2-core lease (`final2-console-*.log`).
- `./test-shell` whole (`q run --cores 4`, the fake-kernel stamp removed so the stage reran;
  `gates/test-shell.log`): native ok, on_beamlet ok (385 tests, 0 failures), on_beam ok,
  entry_point ok, terminal ok, on_fake_kernel ok; **formatting FAILED on main's own
  `userland/shell/lib/redoubt/agents/echo.ex`** (BENCH2's finding, not touched here).
- `make -k -j8 -f scripts/jobs.mk set CASES="formatting docs rt-miri rt-host-tests host-tests
  beamlet-lookup-host beamlet-lookup-cli-host client-host-tests fileserver-host-tests
  littlefsd-host-tests sshd-host-tests erofsd-host-tests ipd-host-tests piped-host-tests
  consrelay-host-tests walfsd-host-tests"` at be4519c2e (the head's code but for the two added
  tests and the doc sentence): 16 of 16 PASS (`gates/jobs.log`; rt-miri 123 s). Rerun at the
  head 10f27228f of `formatting docs rt-miri rt-host-tests`: 4 of 4 PASS (`gates/jobs2.log`).
- Consumer sweep (`sweep/summary.txt`, every crate whose Cargo.toml names the fake kernel under
  libs/, servers/, userland/, tools/, kernel/; `cargo test -q -p <crate>` each through `q run
  --cores 4`): redoubt-client 44, redoubt-fileserver 12, redoubt-rt 174, redoubt-fake-kernel 0
  (no tests of its own), redoubt-bootfsd 18, redoubt-consoled 22, redoubt-consrelay 26,
  redoubt-erofsd 16, redoubt-ipd 54, redoubt-keyd 38, redoubt-littlefsd 62, redoubt-piped 18,
  redoubt-sshd 39, redoubt-walfsd 36, beamlet-redoubt (--features fake) 89: all passed, 0
  failed, none at the 60 s stuck guard. (`.tick(` in libs/ipclist and servers/blkd is their own,
  not the fake's.)
- `rustfmt +nightly --check` on both Rust files: clean. Not run: the whole bench (the train's).

## Pages (the documentation check)

- `docs/testbench.md` "On a shared host", the held-clock paragraph: states the rule (a tick is
  taken once every other thread of the fake is in a wait, a woken thread counting only when it
  waits again) and the hang it ends. No dates or package ids.
- Checked, unchanged: `docs/userland/shell.md` status lines naming `./test-shell` (the shell and
  its suite are unchanged); `GETTING-STARTED.md:238-258` (`./test-shell`'s stages, no claim on
  flakiness); `docs/userland/beamlet.md` (console tests named as what they prove, unchanged);
  `README.md`, `docs/README.md` (nothing on the fake's clock); `docs/SECURITY.md` (no rule's tests
  changed). The fake's own doc comments (module, `State`, `tick`, `change`, `wait`) carry the rule.

## Residuals and findings beyond the node

1. A freshly spawned fake thread counts itself (`Counted::new`) on its own thread, after
   `std::thread::spawn` returns: a tick taken before that does not wait for it. The existing
   held_clock test and the new ones observe a started flag first; console.rs's tests reach their
   threads through the platform's calls before any tick. Pre-existing, not changed; worth a line
   if a future test ticks straight after spawning.
2. `./test-shell`'s formatting stage fails on main's `echo.ex` (BENCH2).
3. `Redoubt`'s `Drop` waits up to `FLUSH_US` for pending writes on the kernel's clock; on a held
   clock with no ticker that is a wait only a change can end. Correct for the machine; a test
   that leaves a write pending at its end relies on the fixture answering it. Not changed.

## Next step

Review by kernel-red-4; then the train. Remove `.tmp/B90/main-demo` (a detached scratch
worktree of origin/main) with `git worktree remove --force .tmp/B90/main-demo` at cleanup.
