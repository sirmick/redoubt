# STEWARD3X questions (steward3x-implementer, 2026-10-16), QA thread `STEWARD3X-rebind`

Non-blocking: the defaults below are built unless the Architect says otherwise.

## The wire: three opcodes in `libs/wire/tables/steward.md`

- 18 `reconnected(server: string, connection: handle[0] endpoint)`, on `init`'s class (badge 3).
- 19 `server_restarted(server: string, slot: u16)`: the steward's send on a session's `notices`
  endpoint; as a call it is malformed on every badge (`Class::None`, as `lease_ended`).
- 20 `rebind(slot: u16)` from a minted badge; reply `at: string, connection: handle[0] endpoint`.

## Q1: the notice carries the slot; `rebind`'s reply names the path

The ruling's `server_restarted(server)` gains the slot, and the reply to `rebind` names the path
the session binds the connection at. A session knows its home by `home=` alone, not which slot
or server binds `/boot`, `/vault` or `/net`; the steward derives both from the binding table. Only
slots bound at a path are announced: the system volume's named handle (slot 5) is not, since
beamlet loads code through it and no `bind` can replace it (a residual: an `erofsd:system`
restart leaves the session dead). Slot 4 (the console) is never announced and `rebind(4)` answers
`unknown`: `consoled` is no shared server, and a session must not mint a UART console.
Default: as above.

## Q2: no core row

`rebind` is the server's, not a core event: the badge's route names the object, its scope is the
one the login batch made, the connection is `Kernel::connect(owner, slot, scope)` as the batch
made it, and the `made` token for that slot is replaced (the dead one's record is dropped: there
is nothing to disconnect at the new instance). It grants nothing the login did not grant; the core
decides nothing new. `reconnected` likewise: the machine swaps the handle it reaches the server
by, drops its attached, carved and held records at that server, and the steward sends the notice
to every live session whose binding table names the server. Default: no row, no oracle or model
change; the host tests are the server's and `init`'s.

## Q3: leases

An agent's VM has no `notices` endpoint (3A) and no shell to re-bind.
- (a) its connections at the restarted server stay dead; the lease runs to its deadline or the
  sponsor's `end_lease`; agents.md and steward.md state it: the sponsor's sessions are re-bound,
  the agent is not.
- (b) the steward ends the lease (a new core event; `lease_ended` tells the sponsor).
- (c) leases get a `notices` endpoint too, and the harness serves it.
Default: (a), with (b)/(c) a follow-up.

## Q4: `init`'s path

After `start(i)` of a restarted server, before the blame: for each of the steward's `handed`
items at that server's endpoints, `init` mints the badge (stamped `users`, as at the steward's
start), makes a connection with `new_connection`, closes the badge, calls `reconnected` within
`RELEASE_TIMEOUT` (1 s), then its fresh list replaces the entry for that endpoint, so its
release at the steward's exit frees the new one. A call that fails disconnects the new connection
and is said: `init: walfsd:data reconnected for the steward: the steward took it | lost: the
steward did not answer within 1 s | lost: the steward ended | lost: the steward refused it`
(blame's words). A server handed as a badge at boot (it made no connection then) is tried the
same way: the steward keeps its badge copy, and the new connection replaces it in its table.
Default: as above.

## Q5: the shell's line

`[walfsd:data restarted: /home/alice re-bound]`, or
`[walfsd:data restarted: slot 1 not re-bound (reason)]`. Default: as above.
