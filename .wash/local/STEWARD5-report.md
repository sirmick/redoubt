# STEWARD5 report: the steward's loop never waits on a shared server

Branch `wp-STEWARD5`, worktree `.worktrees/STEWARD5`, rebased onto origin/main `5923f7bc8`
(train 69; ATK13 was not published at the time of this report). Head: `60369dbb8`, one commit,
13 files, +898/-453. Reviewer steward-red-4. Ruling: rulings 'F1' and its 'F1 addendum'
(thread STEWARD5-handover: the hand-over shape confirmed; the refused mint ruled (b), fails
now, nothing retries inside the steward). Evidence under `.worktrees/STEWARD5/.tmp/STEWARD5/`
(`notes.md`, `logs/` the make logs, `latency/` the two bench logs) and
`.worktrees/STEWARD5/target/jobs/` (per-case logs; the final set's overwrite the earlier).

## Delivered

- `servers/steward/src/cleanup.rs`: the rule unchanged (TRIES, RETRY_US, the parking line's
  attempt, wake on a step done or a mint answered, forget on a restart); `run`/`run_parked`
  become `take(now) -> (H, attempt)` and `settle(result, now) -> (H, Fate)` so no lock is held
  across a call; `Fate` (Done, Retry, Parked(tries), Failed(e)); `parked_at(server)`; `forget`
  drops the server's due steps as well as its parked ones; `len` counts the step whose call is
  out. Module doc: the thread, and that the queue is shared under a lock neither holds across a
  call.
- `servers/steward/src/lib.rs` (`Kernel`): `clean(handle)` hands over, no result; `parked_at`,
  `wake`, `forget`; `server_of` leaves the trait; `Steward` no longer owns the queue
  (`pending`/`parked` gone: they are the kernel's).
- `servers/steward/src/drive.rs`: `give_back` = release + hand over; `cleanup()` gone;
  `minting`: wake the server's parked steps before the mint when any are parked, mint, and on
  any answer but `Timeout` wake again; a refused mint fails its step as a timed-out one does.
- `servers/steward/src/bin/steward.rs`: the shared queue (`Arc<Mutex<Queue<Job>>>`, the lock
  prepared at start so its endpoint page is charged then); `Job` (key, endpoint handle, id,
  ended) and `Job::tell` within `STEP_US` = `RELEASE_TIMEOUT` (1 s); `cleanup_thread` (take,
  call outside the lock, settle, report every fate but a retry's as a send on badge `CLEANUP`
  = 6, idle in a call on the same badge); the loop: `clean` builds the job from `held` (or
  completes a step nobody is told inline), `ring` answers the held idle call, `cleanup_idle`
  holds it (answering at once if a step is already due), `cleanup_tick` at the loop's top folds
  the thread's next retry into the wait and rings it when due, `cleanup_done` drops a done or
  failed step's record and says the parking attempt's line; `reconnected` retires the old
  handles and `retired_closed` closes them at the thread's next report. `CLEANUP_US` gone. The
  module doc states the single-arbiter invariant, the no-lock-across-a-call rule and that the
  thread cannot die alone. `CLEANUP_STACK_PAGES` = 1 with its arithmetic.
- `servers/steward/tests/steward.rs`: the Recorder owns a `Queue<usize>` and a `thread()` turn
  (the thread's calls recorded as `Call::Clean`, the loop's hand-overs as `Call::Hand`, wakes
  that woke something as `Call::Wake`, `Call::Forget`).
- `image/manifest.json`: steward `budget.pages` 1024 -> 1027. `tests/size-budget.toml`:
  servers/steward 3414 -> 3539 (the `Size budget:` line is in the commit).
  `tests/steward-latency.toml`: the header comment's state of the rows.
- Pages: `docs/servers/steward.md` "Failure and restart" (the cleanup bullet rewritten: the
  thread, the hand-over and why it is no send, the fates, the mint rule, the cost arithmetic,
  the history; the console bullet's order sentence; the status line), `docs/kernel/budgets.md`
  (17,421 -> 17,424 with the steward's 1,027 shown), `docs/testbench.md` (the thread's stack
  sentence beside consrelay's), `docs/kernel/scheduling.md` "With the real steward" (the
  one-hart columns re-measured, N = 1 to 4 on both widths; the paragraph under the table),
  `docs/servers/init.md` "Residual risks" (the blame bullet's clause: the cleanup is the
  thread's).

## Point 4 (two threads on one kept connection)

`libs/rt/src/client.rs`: `Connection` holds the endpoint, an `AtomicU16` tag counter and a
timeout, no buffer and no fids (its header: "several threads call on one connection at once");
`disconnect` encodes its id into the words and lends nothing; `new_connection` uses the
caller's lend. So the thread's `disconnect` and the loop's `new_connection` disturb nothing of
each other's, and the thread has no lend page: the ruling's "one Lend page" is not needed.
`in_flight` at the servers was not touched: the derivation names the steward's share by its
pending mints, not its calls in flight.

## The hand-over's shape

kernel/ipc.md: a send blocks until a receiver takes it, so a step sent to a thread inside a 1 s
`disconnect` would hold the loop that second. The queue is shared under `redoubt_rt::sync::
Mutex`, held for one queue operation at a time and never around a system call; the thread idles
in an open call on the loop's endpoint, which the loop holds and answers by `reply` (never
blocks) when a step is due or handed over; the loop is the one arbiter of the queue, so a
hand-over either precedes its take of the idle call (which then finds a step due and is answered
at once) or follows it (and answers it): no wake-up is lost, and a thread mid-step takes the
next due step itself before idling. Fates come back as sends; the thread blocks on a send only
until the loop's next receive, and the loop never blocks on the thread. A restart retires the
handles the thread may still be calling on until its next report, after `forget` dropped the
server's queued steps under the lock. The first draft's `await_cleanup` (a window receiving
only the thread's events for a refused mint) was refused by the Architect and is gone.

## Host tests (62 pass; `cargo test -p redoubt-steward-server`, rc 0 on the head)

Kept in the new shape: a_cleanup_step_that_times_out_is_retried_then_parked,
a_parked_step_runs_at_the_servers_next_answered_call, reconnected_forgets_a_servers_parked_steps,
a_never_answering_server_costs_no_bound_once_its_steps_are_parked, the_cleanup_queue_holds_
each_released_connection_once, a_session_s_end_is_answered_before_any_cleanup_call,
a_cleanup_step_the_server_refuses_or_cannot_take_is_done_at_once, STEWARD6's three.
Renamed: a_turn_stops_at_its_first_timed_out_step -> a_timed_out_step_does_not_hold_the_steps_
behind_it (the turn rule was the loop's; the thread moves on); a_mint_at_a_server_with_parked_
steps_runs_them_first_and_succeeds_where_it_was_refused -> ..._wakes_them_and_the_login_after_
succeeds (ruled (b)). New: the_loops_turn_makes_no_call_to_a_server_that_never_answers (the
loop's events hand over and call nobody while the server never answers; every cleanup call is
the thread's), a_step_that_never_completes_holds_only_itself (take without settle: the loop's
events go on, a restart forgets the rest, the late answer settles it), a_step_that_completes_
after_the_loop_moved_on_is_settled_then, reconnected_forgets_a_servers_due_steps_too. Five
exact-sequence tests now expect `Hand(h)` after `Release(h)` and `Forget` after `Reconnected`.
The lines' order is the console queue's (3U's tests, unchanged) and the fates arrive in order
on one endpoint; not host-tested beyond that.

## Gates (exit codes; every command under scripts/q or jobs.mk)

The full set, both widths (`make -f scripts/jobs.mk set CASES=...`, -k): the 33 steward-*
cases, steward-host-tests, init-boot, userland-boot, beamlet-footprint, formatting,
size-budget, unsafe-budget, docs: 77 targets, every rc 0, run twice: on d4625319b (the ruled
code before three pages-only rebases; `logs/gates3.log`) and on the head 60369dbb8
(`logs/gates-final.log`, the per-case logs in `target/jobs/`). Between them host tests, docs,
formatting and size-budget were rerun at every rebase (logs/gates4-8.log), rc 0.
Memory scan: `stack steward thread 1` 1,432 B on rv64 and 1,168 B on rv32 on every memory case
(steward-session-ends, init-boot, userland-boot, steward-ssh-two-principals); first-thread
stack and heap peaks unchanged (17,560 B of 9 pages; 16 of 128 pages). Release checks of the
binary on rv64 and rv32 with and without `latency-probe`: rc 0 (cargo check --release; the
cases' builds are the real builds).
Not run: steward-latency at two harts on rv64 (the brief's "the same at N = 3"): the slot went
back to steward7 after my two one-hart runs; the page keeps the two-hart column from before the
thread and says so. The test manifests (latency, confined, idle, six-principals, three-contexts,
vault-launch, many-sessions, boot-profile) keep the steward at 1024 pages: none is
memory-checked, and the thread's three pages fit their headroom as the image's did.

## steward-latency, one hart, seed 3 (`cargo testbench --exact --arch W steward-latency --smp 1 --keep-failed`, logs `latency/rv64.log`, `latency/rv32.log`)

Both PASS (rv64 736 s, rv32 777 s), every target met at N = 1, 2, 3 and 4; net µs p50/p99:

| measure | rv64 N=1 / 2 / 3 / 4 | rv32 N=1 / 2 / 3 / 4 |
| --- | --- | --- |
| timer wake | 2085/3151, 850/1736, 1244/2297, 641/953 | 1047/1460, 1215/1508, 867/1268, 1208/3421 |
| decision wake | 1150/11227, 16547/21849, 672/3999, 900/2165 | 24226/26261, 668/1417, 1198/1421, 718/2990 |
| deadline notice p99 | 12633, 13645, 10460, 11423 | 10864, 11670, 10731, 10751 |
| budget_destroy | 6059/9512, 6078/8375, 5049/10183, 6042/8751 | 7840/10257, 5364/7733, 5872/10020, 7099/10828 |
| R10 p99 (134 destructions) | 9699 | 9657 |
| lease end | 21849 + 9699 = 31548 | 26261 + 9657 = 35918 |
| reply_wait | 6459/6694, 6533/6560, 7664/9191, 6652/9310 | 6761/6792, 6978/9439, 6929/8874, 7128/10085 |

Before: timer wake p99 77617/92789 (rv64) and 68679/92904 (rv32) at N = 3/4; reply_wait p99
124110/65333 and 134595/63756. No 100 ms retry in any sample now. The console logs are not kept
on a pass, so the count of `parked after` lines in these runs is not recorded.

## Documentation check (summaries)

- README.md, GETTING-STARTED.md, docs/README.md: no claim on the steward's cleanup or latency;
  no change.
- docs/servers/steward.md: the owning page, rewritten as above; "Residual risks" (STEWARD6's
  bound) unchanged and now cited from the mint rule.
- docs/servers/serving.md "a disconnect a busy server does not take holds its connection's
  admission": unchanged, still true (the thread's attempt, now).
- docs/servers/init.md "Residual risks": one clause updated (the steps were "at the steward's
  turn, each within its own 100 ms send bound").
- docs/kernel/scheduling.md: the table and paragraph; the earlier paragraph on the parked line
  appearing one to four times a run stays (it describes the parking rule, unchanged).
- docs/kernel/budgets.md: the servers' total; docs/testbench.md: the memory prose (the table's
  steward row is unchanged: first-thread stack and heap).
- docs/SECURITY.md R41 row cites files, not these tests: no change. docs/userland/agents.md,
  sshd.md: no claim affected.

## Open risks and notes

- The two-hart rv64 column is not re-measured (slot); the cause of its one miss is the retry
  this package removes, so it is expected to pass when the two-hart rows next run.
- A slot released to the thread whose disconnect is not yet done is a transient: a mint there
  fails only if the share is at cap and a reuse comes within about a second (the Architect's
  note); no row of this run showed one. If a measured run ever does, it is the case driver's
  bounded retry to add, not a design change (STEWARD7 should expect that).
- A thread of the steward's process cannot die alone (no thread exit notice; a panic ends the
  process): stated in the module doc; no restart path.
- An abandoned idle call (the thread gone) would leave `idle` holding a stale Request, replied
  later as discarded: cosmetic, and unreachable while the thread cannot die alone.
