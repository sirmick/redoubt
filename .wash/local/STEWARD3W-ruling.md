# STEWARD3W ruling: a cleanup step a busy server did not take is parked, not dropped

Thread `STEWARD4-crossing` (the orchestrator's FYI of 2026-10-16 from steward4-implementer-2's
latency run, round 3). Node: STEWARD3W (drafted with this ruling). Architect's territory: a
mechanism inside the steward's serving loop; no wire, kernel or target changes.

## The observation

Two spinner VMs reading at `erofsd:system`; a lease's end queued its `disconnect` there; 3Y's
cleanup timed out four times (100 ms each, a second apart) and dropped the step with the console
line steward.md names; the lease's `State` stayed in the steward's share at `erofsd:system`
(eight in all, Q12) and the eighth lease connect was refused. This is 3Y's stated residual ("the
server's admission for that connection is then freed only by its own restart"), now seen under
ordinary load, not after a restart.

## Why it happened

`TRIES` = 4 and `RETRY_US` = 1 s (servers/steward/src/cleanup.rs) were calibrated for a
*recovering* server: four tries span init's 3 s `SERVER_TIMEOUT`, so a server that took none is
lost to init and restarts, and the restart frees the record. A *busy* server is a different
thing: it answers other callers, init sees it alive, nothing restarts it, and the dropped step's
record is never freed. The count was a proxy for "lost"; the proxy fails for "busy".

## (1) What frees a dropped disconnect's State

Alternatives weighed:

- **The server reaps a connection whose scope is gone.** 3R's rule (STEWARD3R-ruling.md §3): the
  server learns nothing of the destruction; a notice is an abandoned-call pattern the book
  rejected, and a kernel query "is this badge live" is a new ABI call: the owner's, and not
  needed.
- **Keep the residual.** Under 3Z's re-homing the leaked slot sits in the account's bucket, so one
  account pays for its own lease's leak, not every principal. Bounded, but under a steady read
  load an account could lose a slot per lease it ends; a bound that shrinks with uptime is not a
  bound the page can state.
- **Retry at the server's next answer (chosen).** The fourth timeout *parks* the step instead
  of dropping it. A parked step runs again at the loop's turn after any call to that server
  completed within its bound (a mint taken or refused, a step done), and before a mint the
  steward makes at that server while it has parked steps there (the mint needs the slot the
  step frees; this costs the mint at most one bound, a turn's rule as today). A server that
  init reports restarted (`reconnected`) forgets its parked steps with its records. A server
  that never answers again costs nothing while its steps are parked: no timer, no bound per
  turn. Memory: a parked step is a queued step that has not left; the queue's argued bound (a
  connection enters once) covers it unchanged.

The console line changes from `dropped after 4 attempts` to `parked after 4 attempts: retried
when the server answers`. steward.md's residual sentence becomes: "a step a server did not take
in four attempts waits for that server's next answer or its restart". scheduling.md's "With the
real steward" keeps the finding beside the table if the leak is seen again in the latency run
(the implementer's reading is right: N = 3 is the finding's number, not a tuning).

## (2) The 100 ms bound stays

The send bound is the loop's latency promise: what a notice gets, what the blame and lockout
targets on scheduling.md are measured against. Widening it to the client library's one-second
call timeout (or the server's admission timeout) re-creates the stall 3Y measured: a busy server
is exactly the case where the loop would wait the full bound. A busy server is answered by
persistence (the step waits for the server, not the loop for the server), not by width. A
constant moves only with a measurement that shows the bound itself too short for an idle
server's answer; none does.

## 3Z's brief

3Z does not take this: its scope is admission (admit.rs, the pending pool, consoled's limits);
cleanup.rs and drive.rs are STEWARD4's hotspot. One sentence added to the 3Z brief: the re-homing
moves the leaked slot into the account's bucket, and serving.md's residual says so in 3Y's words
("until the server's restart") until STEWARD3W replaces it.

## STEWARD3W, the node

Scope: servers/steward/src/cleanup.rs (a step knows its server; `parked`; the answer signal),
drive.rs/lib.rs (the call sites that complete a call to a server; `reconnected` forgetting parked
steps; a mint running the server's parked steps first), the page sentences above, SECURITY.md's
R41 row if its tests change. Host tests: a step parked after four timeouts runs at the server's
next answered call; a mint at a server with parked steps runs them first and succeeds where it
was refused; `reconnected` forgets that server's parked steps; a server that never answers costs
the loop no bound once its steps are parked; the queue holds each connection once, parked or
due. Bench: `steward-latency` round 3 forbids the parked line outliving the round (the run's
`disconnect` lands before the next lease connects) on both widths; `steward-server-restart`
unchanged in verdict. Size S; Tier A (the steward's loop); steward-red reviews. Sequenced after
STEWARD4's drive.rs lands (the orchestrator says when); `needs: []` so the plan does not block
it on an open package.
