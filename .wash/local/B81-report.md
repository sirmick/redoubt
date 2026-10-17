# B81 report (b81-implementer)

Branch `wp-B81` on main 2b0c897c9, worktree `.worktrees/B81`. One commit, head 498750f01;
`git status --short | wc -l` = 0. Reviewer: kernel-red-3.

## What was found

The case `beamlet-lookup-host` failed in trains 44, 46 and 49 as `beamlet-redoubt --test system`
(train 46's and 49's transcripts name the file; the kept log held only Cargo's deprecation
warnings). Reproduced once in 40 runs under load (four concurrent copies of the case's 21 test
binaries, each on a 4-core lease; `.tmp/B81/load1/run-10-1.log`): two tests of `system.rs`,

- `a_served_servers_binding_and_handle_go_with_its_budget`: `bind` → `Refused("not_a_connection")`,
  the attach's `ATTACH_US` (1 s on the host's clock) ran out before the fixture `littlefsd`
  answered;
- `what_a_served_server_returned_goes_with_its_budget_and_is_not_closed_again`: `p.call(...,
  1_000_000)` → `Refused("timeout")` to a server thread just spawned.

Both are the platform's bounds, read on the fake kernel's clock, which was the host's: a race
against the host under load. Pure CPU starvation inside the lease does not reproduce it (8 busy
loops on a 4-core lease, 9/9 green, `.tmp/B81/starved8/`), so the quiet class would not have made
it a verdict: quiet jobs run beside the rest of the machine.

The race also hid in passing runs: the tests waited 10 s of host time three times (`FILE_WAIT_US`)
and 1 s in `a_forgotten_connections…` (`HAND_US` in `Io::forget`, the server outliving its budget
because the fake runs no child); the `system` binary took 46 s.

## The fix (events, not the host's clock)

`system.rs` holds the fake's clock (`Fake::hold_clock`, the mechanism `libs/rt/tests/held_clock.rs`
and `parked.rs` use): every deadline is a step the test takes once the state it waits on is reached.

- `turn()`: the clock is the binary's and a tick is a step of every test's deadlines, taken only
  when every fake thread is in a wait, so the tests run one at a time (a static mutex); the three
  tests that bypass `with_session` take the turn too.
- `park_and_expire`: a call parks (`Later`), one `tick(FILE_WAIT_US)`, then its answer (`emfile`,
  `etimedout`).
- `step_until` / `ask_stepping`: `RETRY_US` steps between non-blocking looks, for what waits on the
  clock: a `busy` retry, a hand-over's `RETRY_US`. Not for a server at work: each of the server's
  own calls is a quiescent instant, so stepping through its block I/O ran the clock out
  (`a_forgotten…`'s open hit `Etimedout`); plain `ask` waits on changes.
- `ATTACH_US`, `HAND_US`, `REQUEST_WAIT_US`: one tick from a helper thread (or the test thread,
  for the serve thread's deadline) once the platform waits on the bound; the silent bind asserts
  the refusal came at exactly the bound.
- `a_fresh_connection_at_the_bound…`: the ender is gated on a channel the test signals after
  `Later`, not a 100 ms sleep; the `< HAND_US`/`>= 100 ms` host asserts are gone (the `Later` with
  every place still held is the proof).
- `a_request_already_waiting…`: the `left > 5 s` assert is gone; on the held clock an idle that
  slept would be called stuck.

The `system` binary runs in 24-27 s (was 46), 27/27.

Two bugs met on the way, both the fake's rules: `finished()` pops the answer (my first helper
consumed it and spun); a tick needs quiescence of every fake thread, so a shared held clock needs
the tests serialized.

## Part 2: the kept log

`build.rs::test_failure` keeps the harness's failures block (from `failures:`: each failed test's
panic and name, the `test result` line; past 400 lines its head and its names list) and Cargo's
last `error` line on (which test file, how the binary ended), never the warnings; `cargo_test` and
`fanout`'s job failures use it. Unit test
`a_host_tests_failure_keeps_the_failures_block_and_not_cargos_warnings` (three shapes: a block, a
crash without one, a block past the bound).

## Pages

- `docs/testbench.md` "On a shared host": the held clock for `beamlet-lookup-host`'s system tests
  (a platform bound on the fake's clock is a wall-clock bound too); what a failing host-tests
  case's job log keeps.
- `docs/userland/beamlet.md`: checked (status line 481 says the tests run on the host on the fake
  kernel; no clock claim); no change needed. README.md, GETTING-STARTED.md: no mention of the case.
- `userland/otp/redoubt/src/lib.rs`: `REQUEST_WAIT_US` re-exported for the test.

## Gates

- `cargo test -p beamlet-redoubt --features fake --test system`: 27/27 (`.tmp/B81/held-12.log`).
- Alone ×5 (`cargo test -p beamlet-vm -p beamlet-redoubt --features beamlet-redoubt/fake`, the
  case's command): 5/5 exit 0, 23 binaries ok each, `system` 24.4-27.2 s (`.tmp/B81/alone-final/`).
- Under load ×12 (four concurrent copies of the 21 binaries on 4-core leases × 3 iterations):
  12/12 green (`.tmp/B81/load3/`; `.tmp/B81/loop.sh`, `run-binaries.sh`).
- Bench unit tests (`cargo test -p testbench --bin testbench -- build::tests fanout`): 17 passed.
- jobs.mk: `docs` PASS, `formatting` PASS, `size-budget` PASS, `unsafe-budget` PASS,
  `beamlet-lookup-cli-host` PASS.

## Residuals

- The other test files of the case (`files.rs`, `console.rs`, …) still bind on the host's clock;
  none has failed in a train. The same conversion applies if one does.
- `wait_held(…, 5 s)` host waits remain in two tests (a handle's close by a thread): a wall-clock
  bound only in the direction that fails on a 5 s stall, never seen.
- Pre-existing warning `field harts is never read` in the bench's test build (not this package).
