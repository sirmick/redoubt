# STEWARD13 report: a launch does not hold the steward's loop

Branch `wp-STEWARD13`, worktree `/home/mcloonan/redoubt/.worktrees/STEWARD13`, on origin/main
`ddafcad7d` (train 80). Head: see the final section (the WIP commits are rebuilt into logical
commits before acceptance). Tier A (the steward's loop and launch), reviewer steward-red-5.
Rulings: `.wash/local/wave-2026-10-18-rulings.md` 'STEWARD13' and 'STEWARD13 design'; brief
`.wash/local/STEWARD13-implementer.md`. Evidence under `.worktrees/STEWARD13/.tmp/STEWARD13/`:
`before/` (the overlap case on the unchanged loop: `make.log`, the job logs, the run dirs with
console and ssh logs), `after1/` (the first boots on the new steward), `gates/` (the whole gate
set, `make.log` and `set.txt`), `gates1.log` (the host gates before the ceiling moved), and
`target/jobs/` for the per-case logs.

## The cause, as found

`Machine::launch` (`bin/steward.rs`) ran `Launch::streamed(..).start()` on the loop: the VM's
image streamed from the system volume into the child's pages, a batch of 64 pages at a time,
then `process_start`, 1.6 to 6.6 s measured. A lease's deadline firing meanwhile destroyed the
budget in the kernel at once (R10), but the exit notice's `Exited` event, the `lease_ended`
send to the sponsor and everything else the loop serves waited until the launch returned.
Measured by the new case `steward-launch-overlap` (lease A, 8 s, spinning; lease B's
`start_agent` one second before A's deadline, so B's launch of 1.64 to 3.1 s spans it; five
pairs a boot) on the unchanged loop: A's `deadline_notice`, the sponsor told less the deadline,
net p99 375 / 568 / 582 ms (gross 957 / 1,497 / 1,386 ms) on rv64 at one hart, rv64 at two and
rv32 at one, against the 40 ms target; every deadline inside B's launch window
(`before/rv64-steward-launch-overlap.log`, `before/rv32-steward-launch-overlap.log`).

## What is built

- **The launch thread** (`bin/steward.rs`: `launch_thread`, `start_job`, `read_from`,
  `Launches`, `LaunchJob`, `Launching`, badge `LAUNCH` = 7, `LAUNCH_STACK_PAGES` = 3). The loop's
  `Kernel::launch` prepares the launch (the exit endpoint, a session's notices endpoint and send
  right, the child's handles and names resolved, the arguments) and queues a self-contained job;
  the thread pops jobs in order, runs `Launch::streamed(..).start()` as the loop did, with its
  own 16-page lend and the loop's attached `bootfsd` connection (clones share it across
  threads), leaves the process handle or the error in the shared map and reports the job by a
  send on `LAUNCH`; it idles in a call on `LAUNCH` the loop answers when a job is queued
  (STEWARD5's hand-over by reply: `ring_launch`, `launch_idle`). `Kernel::launched(job)` on the
  loop closes the steward's copies (the process handle, the notices receive right, the held
  connections but a channel console), watches the exit, keeps the send right; a failed launch
  closes the exit endpoint and the send right and returns the error.
- **The pending batch** (`drive.rs`): `Kernel::launch` returns `Launched::{Started, Pending}`;
  `run`/`run_from` stop at a pending `Launch` with `Ran::Pending(Pending {batch, step, produced,
  job})`, kept in `Steward.pending` (FIFO); `Steward::launched(kernel, job)` finishes the step
  (`started`: the process under its token and among `pids`, the notices send right under the
  session), runs the batch's remaining steps (every table's batch ends at `launch`, so none),
  pushes `Done {object, result}` through `event`, then replays the held events of that object in
  arrival order. A thread failure fails the step (`failed`, `StepFailed`), and the core's
  rollback runs as for any failed step.
- **The held events** (`drive.rs` `names`/`holds`, `Steward.held`): `Exited` of the launching
  object, `EndLease` naming it, `ChannelClosed` naming a launching session, and `SshdGone` while
  any session launch pends. These are the rows the tables mark `unreachable` in `Starting`
  (`libs/steward/tables/lease.md`, `session.md`); `SshdGone` because `attach_relay` indexes the
  attachment at decide time, so its `Detach` would reach the starting session. The core's own
  rows take `LockedOut`, `Idle`, `Attach` (`refuse_in_use`) and a session's `EndLease` (`refuse`)
  in `Starting`, so those are served between a batch and its `Done`; nothing in `Starting`
  emits a second batch, so no batch is stacked on a pending one. The core (`libs/steward`) is
  untouched.
- **Parked calls** (`protocol.rs`, `bin/steward.rs`): every call gets its own reply token
  (`Serving::with_reply`, drawn above `CONSOLE`, now `pub`); a call whose reply is absent while
  the core still owes one (`Steward::awaits`: the pending owner's stored reply, or a held call's
  token) is `parked`, its `Request` kept in `Machine::parked`; the `Done`'s or a replay's
  `Output::Reply{to}` answers it through `answer_late` (`Late`, a typed server over the same
  `replied` tail, the reply encoded into the call's own lend, the count of handles it came with
  passed for the decode), after the event's lines (`settle`). The `served start_agent` line and
  the probe's `LATENCY-LAUNCH` window and `reply_wait` arming move to the late answer. An
  abandoned parked call is dropped on its `Abandoned` notice.
- **The install lock** (`Kernel::without_installs`; `Machine::install`): held by the thread
  across `start()` except inside the read closure (`start_job`: the guard dropped on entry to
  `read_from`, taken again on return) and by the loop across the clock read and destroy of
  `Steward::destroy`, the only change to the destroy path (the wrapper; the rule inside is
  STEWARD10's and STEWARD11's, unchanged). The thread creates no budget.
- **The case** `tests/steward-launch-overlap.toml` (both widths, smp rv64 [1, 2], rv32 [1],
  the latency manifest, five pairs in one round); `tools/testbench/src/sched_oracle.rs`: the
  `LATENCY-LAUNCH` windows parsed and `deadline_notice_in_launch=1` requiring every deadline
  notice's deadline inside one, else a failing verdict (no overlap, or no window, is never a
  pass), with a unit test; `tools/testbench/src/case.rs`: the probe's builders are six.
- **Host tests** (`servers/steward/tests/steward.rs`, 74 pass): the Recorder's `launch_pends`
  (jobs, `launched`, `launch_fails`), `without_installs` recorded as `Locked`/`Unlocked`, `make`
  refusing an install under the lock and `destroy` requiring it; new:
  `a_launch_runs_on_the_thread_and_the_lease_is_answered_when_it_reports`,
  `a_deadline_exit_of_another_lease_is_decided_and_its_notice_sent_while_a_launch_waits` (the
  idle event and another lease's `end_lease` served too),
  `the_launching_leases_own_end_lease_is_held_and_served_after_the_report`,
  `a_launch_the_thread_fails_answers_failed_and_rolls_the_batch_back`,
  `a_second_start_agent_during_a_launch_queues_and_both_are_answered_in_order`,
  `sshd_s_end_waits_for_a_logins_launch_and_detaches_the_context_after`,
  `the_destroy_pair_runs_under_the_install_lock_and_nothing_installs_under_it`. STEWARD10's and
  STEWARD11's guard tests pass unchanged (now under the lock bracket). `Steward::run` returns
  `Ran` and takes the batch by value (the tests' `ran` helper).
- **Memory**: the launch thread's stack 4,840 B on rv64 / 3,808 on rv32 (`stack steward
  thread 2`, `steward-session-ends`) → 3 pages; the loop's first-thread peak 18,536 B (was
  17,560) → `stack_pages` 9 → 10; the entry 1,027 → 1,051 in `image/manifest.json` and
  `tests/data/boot-profile/manifest-unverified.json` (3 + 1 thread page + 16 lend + 2 lock
  endpoint pages + 1 notice page beside a batch + 1 stack page). The test manifests keep the
  steward at 1,024 as STEWARD5 left them (none memory-checked; the cases pass on them).
- **Size**: `tests/size-budget.toml` servers/steward 3580 → 4024 (the sum rule; the thread, the
  prepare/finish split, the pending batch, the parked calls and their docs).
- **Pages**: `docs/servers/steward.md` "Leases" (a new bullet: the thread, prepare/finish,
  serial launches and what `served` covers, the held events and the protocol's re-statement,
  the failure path, the measurement; the status line names the case and the six tests), the
  deadline bullet's premise sentence (the launch thread installs only under the lock),
  "Failure and restart" (a new bullet: the lock, the measured wait, defence in depth, the
  hand-over, the cost arithmetic 1,027 → 1,051, the launch-time shift; the status line names
  the case and the lock test), "Residual risks" (`served` includes the queue wait; the mints
  still on the loop within `SERVER_US`, the follow-up the lock makes possible);
  `docs/kernel/scheduling.md` "With the real steward" (the residual sentence replaced by the
  overlap measurement, before and after; the Responsiveness status 25 → 26);
  `docs/testbench.md` (the two painted threads); `docs/kernel/budgets.md` (17,424 → 17,448,
  the steward's 1,051).

## Measurements (seed 3, one run per boot)

| boot | before: deadline_notice net p99 (gross) | after: net p50 / p99 (gross p99) | B's launch, before → after (min–max) | lock wait, worst |
| --- | --- | --- | --- | --- |
| rv64, 1 hart | 375 ms (957 ms) | 16.1 / 20.3 ms (259 ms) | 1.64–2.03 s → 1.66–2.52 s | 33 µs |
| rv64, 2 harts | 568 ms (1,497 ms) | 32.2 / 35.8 ms (261 ms) | 2.32–2.63 s → 2.41–2.91 s | 5.2 ms |
| rv32, 1 hart | 582 ms (1,386 ms) | 15.2 / 18.5 ms (279 ms) | 1.84–2.58 s → 1.86–3.08 s | 50 µs |

The gross figures carry the checked build's audits (about 1 s per sample); the oracle judges
net. B's timer and decision wakes right after its launch are recorded (no target): timer_wake
net p99 0.6 / 4.6 / 0.5 ms, decision_wake 8.3 / 13.9 / 35.5 ms. The longest launch grew by 0.3
to 0.5 s where it overlaps A's exit: the exit's work (its destroy, notice and lines) now shares
the harts with the launch instead of waiting behind it. The 5.2 ms lock wait at two harts is the
loop waiting for the thread's batch install on the other hart.

## Premises KILL1 touches (for the reviewer)

The install-lock argument rests on STEWARD10's premise that a record's handle names its budget
until the deadline the steward minted: (1) the destroy pair's guard (`Steward::destroy`) is
unchanged by this package, only wrapped in the lock; (2) the lock's claim is narrower, that no
handle of the launch thread's is installed between the loop's clock read and its destroy, and
holds whatever KILL1 finds about the pair itself; (3) the thread installs process and page
handles only, never a budget, so a recycled index it takes can never be a budget's. If KILL1
moves the destroy path, the `without_installs` wrapper moves with it, nothing else here.

## Gates (exit codes)

Filled in when the whole set ends: see the final section.

## Documentation check (summaries)

- `README.md`, `GETTING-STARTED.md`, `docs/README.md`, `docs/plan/m3-agents.md`: no claim on
  how the steward runs a launch; no change.
- `docs/userland/agents.md` "A launch takes seconds under load, and its caller waits for it":
  still true (the caller's contract and bound are unchanged); no change.
- `docs/userland/sessions.md`: the launcher writes the startup block (unchanged); no change.
- `docs/servers/erofsd.md`, `walfsd.md`, `littlefsd.md`, `serving.md`: the servers' derivations
  name the steward's share by its connections and parked calls, not its calls in flight (as
  STEWARD5 found); the thread's reads go through the same attached connection; no change.
- `docs/SECURITY.md`: R10/R26/R39 cite no test this package changed; the new case and tests
  are on the Leases, Failure-and-restart and Responsiveness status lines; the docs gate passes.
- `docs/servers/init.md` "Residual risks": the cleanup clause is unchanged.

## Open risks and notes

- A `start_agent`'s `served` time now includes a launch's wait in the thread's queue behind an
  earlier launch (stated on the page). The client's launch bound (14 s) stands: a second
  caller waited at least as long before, since its call was not received until the first
  launch returned.
- A launch that overlaps another lease's exit is 0.3 to 0.5 s longer on these boots (the work
  moved onto the shared harts); the launch bound's derivation (twice the worst launch, 6.64 s)
  is not approached.
- A lease batch's mints still run on the loop within `SERVER_US`; a deadline inside a mint
  waits for it (the stated residual, the Architect's named follow-up).
- The thread's read closure discipline (the lock dropped inside every read) is shown by
  construction in `start_job` and by the bench, not by a host test: the closure runs the real
  `Launch::start`, which makes kernel calls.
