# STEWARD3C Q1 ruling: the restart gap is real and gets its own node; case 7 proves on the machine what the machine can show

Thread `STEWARD3C-shared-restart`. Question: `.wash/local/STEWARD3C-questions.md` Q1.

## The gap, named

A shared server restarts with empty tables (R27, serving.md "The server restarts"); every
connection it served is dead, the steward's kept fresh connection (init.md "Fresh connections per
child") included, so every later login fails its home step and every live session's home is
gone, and `init` has no way to hand a running server a new connection (init.md's consoled
residual, now the file servers' and ipd's too). No page says so; it is a design gap, not 3C's,
and m3-agents.md's attack row ("Alice is unaffected, also when walfsd panics") assumes a
mechanism that does not exist. Options (a) and (b) each lose a rule the pages keep: (a) ends
every session and forgets the counts; (b) loses "disconnect at the steward's exit frees them
all" and leaks admission at every server after a steward crash. (c) is a bench-only route through
the core for one case. So:

## Ruling

1. **Owner: STEWARD3X** (drafted under STEWARD3, launchable after 3C and 3Q; Tier A,
   steward-red-2, size M): `init` re-makes the steward's fresh connection at a restarted server
   and tells the steward with a root-class call `reconnected(server, connection: handle[0])`
   (the wire carries handles, as `login` does); the steward replaces its kept connection and
   tells every session the server served (a `server_restarted` send on its `notices` endpoint,
   STEWARD3A's channel); a session re-binds its home through the steward (`rebind(slot)`: a new
   mint below the new kept connection, in the session's scope), and the shell re-opens its
   binding; the dead connection's records are gone with the instance, so nothing leaks. Until
   it lands the pages carry the residual below.
2. **Case 7 on the machine** proves what the machine can show today: bob's session crashes
   `walfsd:data` once (cut-after-write); `init`'s blame line names bob's account and `{}`; the
   steward's audit line counts 1 for (bob, {}); alice's session's VM is alive after (her home
   is dead: stated, not hidden); walfsd restarted by `init`. Nothing about three on the machine.
   The lockout at three (DestroyBudget of bob's sessions and lease, `LockedOut` until the
   window passes, the audit lines, the vault-session arm of R40) is the host test, as in (d),
   with the oracle traces so no row is untaken.
3. **Blame timeout: `RELEASE_TIMEOUT` (1 s)**, as taken; a blame the steward cannot take in time
   is init's console line, not a restart (the brief's words).

## Page words

steward.md "Failure and restart", a new bullet:

> **A shared server restarts.** Its tables are empty (R27), so every connection it served is
> gone: the steward's kept connection, through which it mints each session's home, and every
> live session's. Until the steward is told of the restart and re-mints (planned: `init` hands
> it a new connection; sessions re-bind through the steward), a login fails its home step and a
> session's files answer `enotconn`; the blame counts are kept, so a crasher's lockout still
> comes at the third blame once the mechanism lands.

init.md "Fresh connections per child", a sentence beside the consoled residual: the file
servers' and ipd's restart strands the steward's fresh connection the same way; the planned
`reconnected` call is the way out for every server.

m3-agents.md's crash-blame row: "Alice is unaffected" → "not yet (the restart gap)", the lockout
"host only", with the follow-up named without its id.

SECURITY.md R40's row: built · partly tested: the lockout at three is a host test; the machine
shows one blame attributed.
