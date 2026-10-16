# STEWARD3X ruling (thread `STEWARD3X-rebind`)

Question: `.wash/local/STEWARD3X-questions.md`. All five defaults stand, with the words below.

## The wire
18 `reconnected(server, connection: handle[0])` on init's class; 19 `server_restarted(server,
slot: u16)` a send on a session's `notices` endpoint (malformed as a call on every badge, as
`lease_ended`); 20 `rebind(slot: u16) → at: string, connection: handle[0]` from a session's badge.

## Q1: accepted
The notice carries the slot and the reply names the path: the steward owns the binding table,
the session knows only `home=`. Only path-bound slots are announced. Slot 5 (`erofsd:system`,
the named handle beamlet loads code through) is a stated residual on steward.md "Failure and
restart": an `erofsd:system` restart leaves every session dead until it ends; no `bind` can
replace a code source. Slot 4 is never announced and `rebind(4)` is `unknown`: consoled is no
shared server, and a session must not mint a UART console.

## Q2: accepted, with the invariant stated and tested
`rebind` and `reconnected` are the server's, no core row: a rebind mints the new connection by
the same route the login batch used (`Kernel::connect(owner, slot, scope)`): the same scope (the
session's), below the same kind of root (the home carve, whose quota is the home's, shared), in
the same account bucket; the `made` token for the slot is replaced and the dead one's record
dropped (nothing to disconnect at the new instance). It grants nothing the login did not. A
host test asserts it: the re-bound connection's stamp, root and quota equal the login's, and a
`rebind` from a badge whose slot is not the session's own answers `unknown`. `reconnected`
likewise swaps the handle, drops the attached, carved and held records at that server, and
notifies every live session whose table names it.

## Q3: (a) now, (c) the shape later
An agent's connections at a restarted server stay dead until its deadline or the sponsor's
`end_lease`; the sponsor's sessions are re-bound, the agent is not: agents.md "Leases" and
steward.md say it plainly, and that the sponsor's way out is `end_lease` and a new
`start_agent`. Not (b): ending a lease for a server's fault punishes the sponsor's work, and the
sponsor already holds the choice. (c), a `notices` endpoint for leases and a harness that serves
it, is the right shape when agents are expected to outlive a server restart; a follow-up node
when the owner asks for it, not now (MAX_LEASE is 24 h, so "near" is not the argument; the
argument is that the sponsor decides).

## Q4: accepted, with two states said
After `start(i)`, before the blame: per handed item at the restarted server's endpoints, init
mints the badge (`users`), `new_connection`, closes the badge, calls `reconnected` within
`RELEASE_TIMEOUT`, and replaces its fresh-list entry only when the steward took it, so the
release at the steward's exit frees the new one. A failed call disconnects the new connection
and is said in blame's words (`took it | lost: did not answer within 1 s | lost: ended | lost:
refused it`); after `lost`, the steward stays stranded at that server until its own restart: say
so in the line's doc and on init.md as the residual it is. A login that reaches the steward
between the server's restart and `reconnected` fails its home step and is refused: accepted and
stated (the person retries).

## Q5: accepted
`[walfsd:data restarted: /home/alice re-bound]` and `[walfsd:data restarted: slot 1 not
re-bound (reason)]`, through the shell's printer as `[lease NAME ended]` is, outside any
evaluation's marks in the machine mode.

## Pages
steward.md "Failure and restart": the bullet from 3C's ruling becomes the rule (the three
operations, the order, the residuals: slot 5, `lost`, the window before `reconnected`);
init.md "Fresh connections per child" and "Restarts and reboots"; agents.md "Leases" (Q3);
sessions.md the shell's line; libs/wire/tables/steward.md the three rows; m3-agents.md's
crash-blame row to its case; SECURITY.md R40 to tested when `steward-server-restart` holds.

## Q4 amended: blame first, reconnect after; the 10 s measured, not guessed; the blocking disconnect is a node

- The order change is sound: blame while the steward is idle, reconnect when the server serves.
  Stated on init.md "Restarts and reboots" in that order.
- The wait for the restarted server's first answer (`reconnect::SERVER_TIMEOUT`) is bounded by
  the server's recovery, which for walfsd is its log replay and volume check: a function of the
  log's length and the volume's size, not a constant of init's. The bound is measured once on
  both widths with the image's data volume (walfsd-power-loss's recovery is the measurement:
  quote it) and set at twice the measured recovery, rounded up to a second, with the reason and
  the measurement on init.md; 10 s stands only if that is what the arithmetic gives. A server
  that does not answer within it is `lost` as ruled, and the line says the bound.
- The steward blocking in an end batch's disconnect at a dead or recovering server is not 3X's
  to fix but is 3X's to state: a residual on steward.md "Failure and restart" ("a session's or
  lease's end waits on each of its servers' disconnects, each bounded by the steward's call
  timeout; a recovering server costs that bound per connection, during which the steward
  serves nobody"), with the measured number from the lockout case. The fix is STEWARD3Y
  (drafted): per STEWARD3R the disconnect is cleanup, never the cutoff, so an end batch replies
  after `DestroyBudget` and the disconnects become deferred cleanup steps the Machine runs at
  its turn with a short bound and a retry, never blocking the loop; R28/B62's rule that a wait
  is bounded by its own deadline then holds for the steward as a client too.
