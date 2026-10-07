# BEAM10 report: beamlet-redoubt's system tests intermittent under load

Branch wp-BEAM10, worktree /home/mcloonan/redoubt/.worktrees/BEAM10, head 7f58d8b5e on main
226245507 (rebased from f11e8c2e9 after BEAM9's merge; BEAM9's retry lines in idle merged without
conflict). One commit: `beamlet-redoubt: an idle that finds something already arrived returns with it`
(userland/otp/redoubt/src/lib.rs, src/files.rs, src/system.rs, tests/system.rs,
docs/userland/beamlet.md).

## Cause

Not a lost request: a request taken and not looked at. `Redoubt::idle(deadline)` began with
`take_completed()`, which drains the wake-ups already queued on the VM's endpoint and dispatches
them: a serve thread's request or a pool reply into `sys.events`, a file result into the files
table. It then returned early only if console input had arrived, and otherwise entered
`io.wait(deadline or FOREVER)`. So when a wake-up arrived just before the VM idled (the caller's
thread winning the race, which load decides), its event was taken and the VM slept on it until
some other wake-up came, if one ever did.

The three failures BEAM4's gate recorded fit:
- `a_request_never_answered_is_answered_by_the_serve_thread_at_its_deadline`: the serve thread's
  `wake.send` returned (the VM took it), the event sat in `events`, nothing else ever woke the VM:
  the fake kernel's 60 s stuck guard, or a hang with the guard's patience on.
- `an_endpoint_served_stays_open_when_its_term_is_dropped`: the VM idled with the request already
  taken, `idle(Some(deadline))` slept to its 3 s deadline, the caller's 3 s bound fired first:
  "caller got 0".
- `a_typed_call_goes_out_on_a_pool_thread_and_its_reply_is_an_event`: both replies taken by one
  `take_completed`, then a wait FOREVER.

Shown deterministically by the new test: the caller runs first and the test sleeps 300 ms so the
request is parked and its wake-up queued before the VM idles; `idle(Some(now + 10 s))` then
returned only at the deadline, and `poll()` afterwards yielded the request (probe run: "waited 0
us left, open calls 0, poll Some(7)": the serve thread had expired and refused the request at
5 s meanwhile). The HAND_US hand-off path (pool.rs, jobs.rs, serve.rs) is not involved and is
unchanged.

Real on the machine too: the shell's scheduler idling right after an event would stall until the
next I/O wake-up.

## Fix

`idle` returns after `take_completed()` whenever anything surfaced for the VM: console input, a
file operation finished and not yet told (`files::Table::has_finished`), or a system event
(`Sys::has_events`). The wait is entered only with nothing to hand over. The early return cannot
spin: the VM polls `poll()`/`finished()` and reads the console before it idles again (vm.rs's
scheduler loop), and `idle`'s contract already allows an early return.

## Tests

- tests/system.rs `a_request_already_waiting_ends_the_idle_that_takes_it` (new; host, fake kernel):
  a request parked and its wake-up queued before the VM idles; the idle returns with it well before
  its 10 s deadline and the caller is answered. Before the fix: FAILED (the idle waited its whole
  deadline). After: passes. The verdict is the idle's own return time and the caller's reply.
- The 2 x 30 bounded tallies (`timeout 600`, `cargo test --manifest-path
  userland/otp/redoubt/Cargo.toml --features fake --test system`), before on a tree without the
  fix (.worktrees/BEAM9-main: main + BEAM9's two commits, which touch none of this) and after on
  wp-BEAM10:

A run is `timeout 600 q run ... cargo test ... --test system`; the bound wraps q's queue wait for
the quiet cores too, so a timed-out run counts as a hang only if its output shows the test binary
running with tests left unreported; one that never started is a queue wait and no verdict (another
tenant held the quiet cores for long stretches during these series).

| series | tree | runs | passed | failed | hung (600 s) | no verdict |
| --- | --- | --- | --- | --- | --- | --- |
| before, quiet cores (`--quiet --cores 4`) | BEAM9-main | 30 | 27 | 1: `an_endpoint_served_stays_open_when_its_term_is_dropped` (line 318: the caller got 0) | 1: `requests_arrive_with_badge_account_and_labels_and_an_answer_reaches_the_caller` | 1 (queue wait) |
| before, general cores, `RUST_TEST_THREADS=8`, beside the other series | BEAM9-main | 10 (stopped: each hang costs 600 s) | 8 | 0 | 2: `a_request_never_answered_is_answered_by_the_serve_thread_at_its_deadline`, `requests_arrive_with_badge_...` | 0 |
| after, quiet cores | wp-BEAM10 d3c847331 (pre-rebase) | 31 | 30 | 0 | 0 | 1 (queue wait) |
| after, general cores, 8 threads, beside the other series | wp-BEAM10 d3c847331 | 30 | 30 | 0 | 0 | 0 |
| after, general cores, 8 threads, beside the other series | rebased head (tree of 7f58d8b5e) | 30 | 30 | 0 | 0 | 0 |

The hung tests are the ones BEAM4's gate recorded, and both idle with a request or reply that can
arrive before the idle; the failed one idles with a 3 s deadline on the same.

## Gates (the rebased tree, identical to 7f58d8b5e's; run in a temporary worktree of the same commit before its message-only amend)

- beamlet-redoubt host tests (`cargo test --manifest-path userland/otp/redoubt/Cargo.toml
  --features fake`, q, 4 cores): console 11, files 16, limits 7, lookup 2, pack 2, system 11,
  userland 2; rc 0.
- prebuilt rc 0; docs PASS; formatting PASS; size-budget PASS; unsafe-budget PASS; no-cruft PASS.

## Documentation check

- docs/userland/beamlet.md: the "Asynchronous underneath" idle bullet gains the sentence; the
  natives' status `tested (26)` lists the new test. The console section's statement that `idle`
  "still sleeps until its deadline" after the console's end is unchanged and still true (nothing
  arrives then).
- README.md, GETTING-STARTED.md, plan M1: searched for "idle", "system.rs", "flake"; no claim
  affected.
- userland/otp/redoubt/src/lib.rs: idle's doc comment amended.
