# B61 report (b48-implementer, 2026-10-15)

Worktree /home/mcloonan/redoubt/.worktrees/B61, branch wp-B61 from main 146db8897. Tier A.

## Design

**The limit.** `Io::connect` (beamlet-redoubt/io.rs) gives each namespace connection one hub
session and one waiter thread, at most `MAX_WAITERS` = 6. At the bound it takes the wake-ups
already waiting and releases every connection whose session has ended *and* whose waiter has
handed its end over; if none has, it refuses `TooManyThreads`, which files.rs maps to `eio`. A
session's `piped` is a fresh server per pipeline, so after six pipelines every place can be an
ended piped whose waiter is still in flight: its completion call fails with the server's death,
it sends its last hand-over to the VM's wake endpoint, and only when the VM receives that does the
hub mark the waiter gone. Between the destroy and that receive, a seventh fresh piped's first
request is refused (pipe-sixty-four on rv32 under load: `{:refused, {:mkdir, :eio}}`).

**Choice: (b) first, (a) as the floor.**
- (b) At `Io::forget` (called by `budget_destroy` once the kernel has destroyed the server's budget,
  so the server is dead and its waiter's call fails at once), after `Hub::disconnect`, the VM waits
  for that connection's waiter to hand over: it receives on its wake endpoint, handing each wake-up
  to the hub as `take` does (others go to `others` for `dispatch`, as today), until the hub says the
  waiter is gone (`Hub::waiting(conn)` false), then releases the session and drops the `Io.conns`
  entry. Bound: `HAND_US` (1 s), the VM's existing bound for a thread of its own to take or hand over
  work; the common case is a thread switch. Past it the entry stays and (a) applies. Cost on the
  common path: nothing per request; per pipeline end, one short wait where the session is already
  tearing down. Invariants kept: a session is released only after its waiter's last hand-over
  (B47); the handle's close stays with the term (B48); nothing is sent on an ended session.
- (a) At `connect`'s bound, after the non-blocking take and release, the VM waits the same way for
  any ended connection's waiter to hand over, up to `HAND_US`, releasing as they do; a place freed
  is taken. Past the bound: `TooManyThreads` still, now mapped by files.rs to `Emfile` ("too many
  open files": the process's bound on what it holds open), so a pipeline reads
  `{:refused, {:mkdir, :emfile}}` and not a device error. (a) covers servers ended by their own
  death with no destroy (none today; the startup servers never end) and the pathological case.
- Not chosen: raising `MAX_WAITERS` (the bound is threads, stated by design); parking the fresh
  request in the files layer (its retries are for `busy`, a server's word, not the VM's own bound).

**Tests.** Host: six servers bound and used, then all stopped and a seventh bound and used at
once, before any waiter has handed over: served (on main: `eio`). Six live servers, a seventh:
`Emfile` after the bound, the six still served. Machine: pipe-sixty-four leaves the quiet class;
run beside load on rv32.

## Build

- `libs/client/src/aio.rs`: `Hub::waiting(conn)`, one line; the crate's ceiling 1108 -> 1109
  (`tests/size-budget.toml`, the commit's `Size budget:` line).
- `userland/otp/redoubt/src/io.rs`: `hand_over(done)` receives on the VM's wake endpoint, handing
  each wake-up over as `take` does, until `done` holds or `HAND_US` (1 s) has passed; `reap`
  releases ended connections whose waiters are gone. `forget` disconnects, then hand_over(the
  waiter gone), then reap. `connect` at the bound: take_waiting + reap as before, then hand_over
  (reap; a place free), then `TooManyThreads` if still full.
- `userland/otp/redoubt/src/files.rs`: the connect refusal maps `TooManyThreads` to `Emfile`,
  anything else to `Eio` (B48's P3-2).
- Tests (`tests/system.rs`): `a_fresh_connection_at_the_bound_waits_for_an_ended_waiters_hand_over`
  (every place used by a live server; a thread stops them all 100 ms after the fresh request is
  made; the request is served after waiting) and
  `a_fresh_connection_past_the_bound_is_refused_emfile_and_the_live_ones_still_serve` (every place
  live; the fresh request is `Emfile` after >= 900 ms; a live server still serves). B48's thousand
  test stops the server before the destroy (on the fake nothing kills the child) and asserts, with
  no idle, that the destroy left the waiter gone: the forget-side wait, directly.
- **Mutation** (io.rs's waits reverted, `.tmp/B61/fix-io.patch`; the files.rs name kept): the
  bound test fails `left: Err(Emfile), right: Ok(())` at tests/system.rs:356 and the thousand test
  `left: 2, right: 1` at :449, its waiter not yet handed over (`.tmp/B61/host-mutation2.log`); a
  first version of the bound test, stopping the servers before the request, passed on main too:
  on the fake the stopped servers' waiters hand over within microseconds, so the non-blocking
  take found them; hence the delayed stop.
- Pages: beamlet.md Threads (the wait, where, the bound; the two tests), residuals (only the
  seven-live-servers bound stays); native.md Bounds (what a fresh piped waits for; `emfile`).
  `scripts/jobs.mk`: pipe-sixty-four stays in the quiet class (first taken out, put back after
  the gate below); its toml comment says why: a hand-over past the 1 s wait under heavy load.

## Summaries checked

Grep for MAX_WAITERS, waiter place(s), TooManyThreads, eio, emfile, quiet class across README.md,
GETTING-STARTED.md, docs/userland/README.md, docs/servers/README.md, docs/servers/piped.md,
docs/userland/shell.md, docs/userland/files.md, docs/testbench.md: one hit, testbench.md:1346,
which is launch-idle's quiet class, not this case. piped.md's "Started by a session" says piped
is started per pipeline and its budget destroyed at the end (unchanged, true). native.md's Bounds
and beamlet.md's Threads and residuals are the pages this changes (above). No other summary
names the bound.

## Branch

wp-B61 rebased onto main 86f51e6fa (train 26, B52): one commit, **3efd81acf**, not pushed, clean.
The rebase conflicted in tests/system.rs, B52's event waits against B61's order in the thousand
test: resolved so both hold. The count is taken before the server is stopped, when nothing is in
flight; after the destroy (the hub has let the waiter go, asserted with no idle) and the drops,
`wait_held(before - 5)` waits for the waiter's own close: five handles leave (the budget's with
the destroy, the job's exit endpoint and process handle with its exit, the send right with its
last term, the waiter's). Gates at 3efd81acf through q: the suite 24 passed (24.8 s); ./test-shell
every stage, the Rust stage forced by removing its stamp; docs PASS. Before the rebase it was
d2a14e050 on 146db8897, which differed from d3c2143e0,
where the gates below ran, only in scripts/jobs.mk and tests/pipe-sixty-four.toml (the case kept
in the quiet class, with the reason), the message, and (steward-red-2's P2-1 on e4aa7e199)
docs/userland/beamlet.md and native.md: past the wait an ended server's place is not yet free
either, the gate's pipeline 14 said so, and the gap (a wait bounded by the request's own
deadline, which the files layer does not carry) is on beamlet.md's residuals as "open", B62 by
this report (the docs checker's C4 keeps IDs off the pages). P3-1: the Build section above now
says the case stays quiet. The code is the same. `make docs` at d2a14e050: PASS, rc 0. Review:
Tier A, steward-red-2.

## Gates at d3c2143e0 (through q, beside SCHED4's timing step, then train-20's bench)

- `cargo test -p beamlet-redoubt --features fake --test system`: 24 passed (host1.log before the
  thousand test's reorder; host2.log after), rc 0.
- `./test-shell`: every stage passed, rc 0 (beamlet 361 passed 1 skipped; BEAM 292 passed 70
  skipped; beamlet-redoubt 24).
- `make docs`: PASS, rc 0 (docs1.log, on these pages).
- prebuilt: rc 0. The 13-case set both widths: 23 of 23 PASS, pipe-sixty-four rv64 22.1 s and
  rv32 16.5 s among them (`.tmp/B61/set.log`).
- pipe-sixty-four rv32 three more times beside load (`q ls`: train-20's bench, ten jobs,
  rv64/kernel-containment and rv64/model-host-tests on four cores among them): PASS 18.1 s, PASS
  18.7 s, and one **FAIL at 44.0 s**: pipeline 14 `{:refused, {:mkdir, :emfile}}`
  (`.tmp/B61/sixty-1.log`). The run's length is the waits: under that load an ended piped's
  waiter took longer than the 1 s bound to hand its end over, at pipeline ends and at the bound,
  until a fresh piped was refused by the bound's name. In the passing runs the waits returned at
  once (their times match the quiet-class runs of B48). So the window is closed where the waiter
  runs within a second of the server's death, and named where it does not; the case stays a
  host-clock case that load can fail, and **stays in the quiet class** (jobs.mk, the toml's comment
  say why), against the assignment's hope: a failure beside other work is no verdict
  (testbench.md), and this one needs the machine starved. What would close it: a bound tied to
  the request's own deadline, which the files layer does not have (its retries are a server's
  `busy`), or the pipeline's start carrying one; the orchestrator's call.
- `rustfmt +nightly --check` on each changed Rust file, `git diff --check`: clean.
- Not run: the whole bench.
