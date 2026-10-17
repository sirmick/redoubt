# STEWARD3Y: the end batch answers at the cutoff; cleanup runs at the steward's turn

Branch `wp-STEWARD3Y` on main 0b3fcfd28, worktree `/home/mcloonan/redoubt/.worktrees/STEWARD3Y`.
One commit, amended as gates fold: see `git log -1` for the head.

## What was delivered

- `servers/steward/src/cleanup.rs` (new): the queue, `TRIES` = 4, `RETRY_US` = 1 s, the bound.
- `servers/steward/src/lib.rs`: `Kernel::release` is the steward's side alone (closes its own
  copy of a scope-minted handle at once); new `Kernel::clean(handle, last) -> Result<(), Error>`;
  `Steward::cleanup` field, `pending()`.
- `servers/steward/src/drive.rs`: `give_back` at every release site (forget's notices and
  connections, Detach's console, a replaced notices right, rebind's dead connection);
  `Steward::cleanup(kernel)` runs the due steps and returns the next due.
- `servers/steward/src/bin/steward.rs`: the Machine's `release`/`clean`, the `disconnect`
  helper, `CLEANUP_US` = `NOTICE_US` (100 ms), the loop's cleanup turn and its wake, attach's
  failure path drops the kept console untold, module doc "Cleanup at the loop's turn";
  `RELEASE_TIMEOUT` leaves the steward.
- `servers/steward/tests/steward.rs`: the Recorder's `clean`, `Call::Clean`, `clean` outcome,
  `clock`; four new tests (named below).
- `tests/steward-server-restart.toml`: `lost` forbidden for every blame (the third no longer
  exempt). `tests/size-budget.toml`: servers/steward 2427 → N (see the commit's paragraph).
- Pages: `docs/servers/steward.md` "Failure and restart" (the rule, status line) and R41;
  `docs/servers/init.md` "Residual risks"; `docs/plan/m3-agents.md` progress.

## Design, as built

The scope's destroy is the cutoff (STEWARD3R, R41). `release` closes the steward's own copy of a
scope-minted handle immediately, because the scope's end frees the handle index for the next
install (the `Held` doc comment's hazard); the server call is `clean`, run from the loop's top
via `steward.cleanup(&mut machine)` before each receive. Each step is bounded by `CLEANUP_US`
(100 ms: the notices' bound), retried no sooner than `RETRY_US` (1 s) later, dropped after
`TRIES` (4) with `steward: disconnect <id> at <server> dropped after 4 attempts: the server did
not take it` (or `ended to sshd's channel ...`). 4 × 0.1 + 3 × 1 = 3.4 s ≥ init's
`SERVER_TIMEOUT` (3 s): a server not serving by then is lost to init too. Only `Timeout` retries;
`Dead` (an `ended` to a channel sshd closed), `Refused`, and a record `reconnected` already
forgot are done at once. The loop caps its receive wait by the next due retry; a cleanup wake
is not the restart probe's. The queue's bound: a connection enters once at its owner's release;
at most SLOTS + 4 per session (badge, slots, relay control, attached console, notices right),
SLOTS + 1 per lease.

Not a design question: the node body asked for a stated cap; 4 × 1 s is stated with its
arithmetic on the page. If the Architect wants another cap, it is two constants.

## Gates

Head `d005c442d` (one commit on wp-STEWARD3Y). `git status --short | wc -l` = 0. Size budget
servers/steward: 2427 → 2522 (the case: `servers/steward: 2522 of 2522 lines`).

- `make -f scripts/jobs.mk prebuilt` → rc 0.
- `make -f scripts/jobs.mk -j3 -k set CASES='steward-blame-lockout steward-server-restart
  steward-session-ends steward-lease-ends steward-end-lease init-boot docs size-budget
  unsafe-budget'` → rc 0, every case PASS: rv64 and rv32 for the six machine cases
  (blame-lockout 10.0/9.1 s, server-restart 15.0/13.5 s, session-ends 55.3/55.1 s, lease-ends
  8.6/8.0 s, end-lease 11.4/10.4 s, init-boot 6.7/6.8 s); docs, size-budget, unsafe-budget PASS
  (rv64 only, host cases). Logs: `target/jobs/<w>-<case>.log`.
- Rerun to capture console logs the bench's KEEP=8 pruning took: `make -f scripts/jobs.mk -j1
  rv64/steward-server-restart rv64/steward-blame-lockout rv64/steward-end-lease
  rv32/steward-end-lease` → all PASS; logs copied to `.tmp/STEWARD3Y/runs2/` (rv32 blame logs
  from the first run in `.tmp/STEWARD3Y/runs/`).

**The stall, before and after.** Before (STEWARD3X's report, both widths): the third blame
printed `init: walfsd:data's fault blamed on account 1002, labels none: lost: the steward did
not answer within 1 s` while the steward audited Blamed and LockedOut. After, every blame on
both widths, the lockout's included: `init: walfsd:data's fault blamed on account 1002, labels
none: the steward took it` (restart rv64: lines 75, 90, 106 of the console log, the LockedOut
audit at 104 and bob's `login bob: refused (LockedOut)` at 108; rv32: 74, 91, 107 with LockedOut
at 105; lockout rv64 line 79, rv32 line 74). `reconnected ... the steward took it` after 590-963
ms on every restart. No `steward: ... dropped after 4 attempts` line in any run: every cleanup
step was taken at its first or a retried attempt. `steward-server-restart` now forbids any
`lost` for a blame.

**end-lease latency.** The case has no send timestamp and each session's `matched N s after
ssh started` is on its own clock, so the call→notice p99 the gate names cannot be derived from
these logs; what one stream shows is notice→answer: 77 ms (rv64), 62 and 100 ms (rv32 twice).
By construction the end_lease path is not slower: its end batch lost its inline server calls
(the lease's disconnects now run after the reply, at the loop's turn) and gained one queue push
per connection. I did not re-measure a p99; if the orchestrator's 125 ms figure came from a
sweep, that sweep is the comparison to run.

- Host: `scripts/q run -- cargo test -p redoubt-steward-server --test steward` → 38 passed, 0
  failed (rc 0), including a_session_s_end_is_answered_before_any_cleanup_call,
  a_cleanup_step_that_times_out_is_retried_then_dropped,
  a_cleanup_step_the_server_refuses_or_cannot_take_is_done_at_once,
  the_cleanup_queue_holds_each_released_connection_once.
- Cross-check both widths: `cargo check -p redoubt-steward-server --bins --target
  riscv64gc-unknown-none-elf` / `riscv32imac-unknown-none-elf` → Finished (rc 0).
- fmt: `cargo +nightly fmt -p redoubt-steward-server -- --check` → rc 0.

## Summaries checked

- `README.md`, `docs/README.md`, `GETTING-STARTED.md`: no claim about an end's cleanup or lost
  blames: no change.
- `docs/plan/m3-agents.md`: the progress paragraph (changed: an ended session's servers told at
  the steward's turn). The crash-blame row's cases are unchanged (both cases still gate it).
- `docs/SECURITY.md` R40 row: the files named still hold the blame path; the residual column
  (a lease's connections at a restarted server stay dead) still true: no change.
- `docs/servers/init.md` "Residual risks" (changed), "Fresh connections per child" (the
  `reconnected` lost case is unchanged: it still depends on the steward taking the hand-over).
- `docs/userland/native.md:546` names `RELEASE_TIMEOUT` for the shell's natives, not the
  steward: no change.

## Files read whole by this implementer

cleanup.rs, lib.rs, drive.rs (whole), bin/steward.rs (whole, in parts), tests/steward.rs
(the Recorder, the helpers, the tests edited and added; the rest by symbol), the two case files'
head and forbid blocks, the page sections edited.

## Folds (steward-red-4's notes, by steward3y-fixer)

- **P2, the bound sentence** (`docs/servers/steward.md` "Failure and restart"): names that a
  `rebind` queues the connection it replaces (one step per call), and that the bound is argued,
  not enforced: the queue has no cap, drains at every turn its servers answer, and a re-bind at
  a server not taking its steps queues nothing more (the mint there fails too). The same in
  `cleanup.rs`'s module doc.
- **P3, a turn's cost: capped.** My call was a cap, not a stated cost: `Queue::run` now stops at
  the first step that times out (the step is re-due `RETRY_US` later, or dropped if last; the
  steps behind it stay due and untouched), so a turn costs the loop one `CLEANUP_US` of waiting
  at most beside the steps that answer. The loop's `due` is then ≤ now and its receive waits
  1 µs: callers are served between two waits, and the next turn runs on. No new constant. Host
  test `a_turn_stops_at_its_first_timed_out_step` (one `clean` in the turn, the rest pending
  and untold; the next turn with answering servers tells every other step);
  `a_cleanup_step_that_times_out_is_retried_then_dropped` now runs each attempt as a round of
  turns (helper `round`: one timed-out step per turn, the rest's due ≤ now). Page, module docs
  (`cleanup.rs`, `drive.rs`, `bin/steward.rs`'s "Cleanup at the loop's turn" and the loop
  comment) and the status line updated; the commit message's cleanup, host-test, pages and Size
  budget paragraphs too.
- **P3, the host-run log:** `/home/mcloonan/redoubt/.tmp/STEWARD3Y/host-tests.log`
  (39 passed, 0 failed); `fmt.log`, `check-rv64.log`, `check-rv32.log`, `prebuilt.log`,
  `cases.log` and `folds-gates.txt` (the rc lines and PASS/FAIL summary) beside it. The first
  host run of the fold (38 passed, 1 failed: the helper assumed a retried step's due equals
  `now`; it keeps its older due) was overwritten by the passing run.
- Size budget: servers/steward 2522 → 2527 (the stop and the append), ceiling raised in
  `tests/size-budget.toml`, the paragraph updated.
- Gates for the fold: see the Folds gates block below.

### Folds gates

Head `74c734e86` (the one commit, amended; message kept, the cleanup, host-test, pages and
Size budget paragraphs updated). `git status --short | wc -l` = 0.

- `cargo +nightly fmt -p redoubt-steward-server -- --check` → rc 0 (`.tmp/STEWARD3Y/fmt.log`).
- `cargo test -p redoubt-steward-server --test steward` → 39 passed, 0 failed, rc 0
  (`.tmp/STEWARD3Y/host-tests.log`).
- `cargo check -p redoubt-steward-server --bins` rv64 / rv32 → rc 0.
- `make -f scripts/jobs.mk prebuilt` → rc 0; `make -f scripts/jobs.mk -k set CASES='size-budget
  steward-server-restart steward-blame-lockout' WIDTHS=rv64`: steward-blame-lockout PASS 9.8 s,
  steward-server-restart PASS 14.4 s (no `dropped after` line, no `lost` in either); size-budget
  FAIL only because the raise was uncommitted; after the amend `set CASES='size-budget'` → PASS,
  rc 0. docs case PASS (1.5 s, run before the amend on the same page text).
- Logs: `target/jobs/rv64-*.log`; summary in `.tmp/STEWARD3Y/folds-gates.txt`.

## Train 46 regression (steward3y-fixer-2): steward-pending-cap

**Cause.** Not the end batch's order: the core withdraws an ended session's requests inside the
EndSession event, before the reply (host test
`an_ended_session_s_pending_request_is_withdrawn_before_the_end_s_reply`, which passed on 3Y's
code as written). An instrumented rv64 run (every call's opcode, every cleanup step) showed no
`end_session` (opcode 12) at all before the approver's login: the case's end step was `bye`,
`Process.sleep(1000)`, `end_session`; the exit step closes ssh's input as soon as `bye` matches;
sshd's `channel_closed` (opcode 2) reached the steward 37 ms later, the context detached, the
Detach's console `ended` was taken at the loop's turn, sshd ended the channel, ssh exited, and
the approver opened her screen 140 ms later with the request still pending in a detached
context. On main that `ended` was sent inside the `channel_closed` call, while sshd's thread was
blocked in that very call, so it was lost after its second, every time; the channel ended only
at the VM's `end_session` a second later, which dropped the request first. 3Y's deferred cleanup
made the detach's `ended` land, as steward.md "A context outlives its channel" says it should.

**Fix.** `tests/steward-pending-cap.toml`: `bye` then `end_session`, no sleep (the marker still
proves the line was read before the close; the VM's call is one IPC, sshd's channel_closed is
the network, ssh's EOF and a call behind it). `servers/steward/tests/steward.rs`: the host test
above. `docs/servers/steward.md` "Failure and restart": the lost `ended` on a client's close
named beside the measured stalls; the status line names the test. Commit 74c734e86 amended; the
account gains a steward-pending-cap paragraph.

**Gates** (logs `.worktrees/STEWARD3Y/.tmp/STEWARD3Y/fix2/`: `cases.log`, `host-tests.log`,
`fmt.log`, `prebuilt-gate.log`, console logs under `runs/`):
`cargo test -p redoubt-steward-server --test steward` 40 passed, rc 0; `cargo +nightly fmt
--check` rc 0; `make -f scripts/jobs.mk prebuilt` rc 0; `make -f scripts/jobs.mk -j3 -k set
CASES='steward-pending-cap steward-approve-box steward-screen-inert steward-blame-lockout
steward-server-restart steward-session-ends steward-lease-ends steward-end-lease init-boot docs
size-budget unsafe-budget'` rc 0: every case PASS on rv64 and rv32 (pending-cap 11.9/11.2 s,
approve-box 14.9/14.0, screen-inert 6.2/5.9, blame-lockout 9.6/8.6, server-restart 14.7/14.1,
session-ends 58.4/58.2, lease-ends 8.5/11.4, end-lease 12.2/16.0, init-boot 7.8/12.0); docs,
size-budget (unchanged ceiling: no steward source changed), unsafe-budget PASS.

**Summaries checked.** steward.md "A context outlives its channel" (already states the
behaviour now realised: no change); m3-agents.md's pending-cap row (unchanged: the case still
gates it); init.md, SECURITY.md R40/R41, README/GETTING-STARTED: nothing on a client's close.
