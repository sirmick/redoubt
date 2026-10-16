# STEWARD3A Q1 ruling: the sponsor's notice rides a send right the session registers

Thread `STEWARD3A-notice`. Question: `.wash/local/STEWARD3A-questions.md` Q1.

## Ruling: (a)

The brief's sentence assumed the steward could send into a session; ipc.md "Authority" says it
cannot (only the receive right receives; what the steward holds per session is its own
endpoint's badge). So the session hands the steward the way in: a send right to an endpoint the
session's VM serves. The authority flows the right way (the session widens what the steward may
do to it, by one send right into its own VM; the steward, trusted base, gains nothing it could
misuse: a send carries the steward's badge, and the listener prints and nothing else).

(b) parks a call per session and needs a queue the core does not have; (c) is not a message, is
unmarked in a driver's stream (AGT2's machine mode), and is closed for a detached context.

## The wire

`libs/wire/tables/steward.md`, two rows:

| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 17 | `notices` | `endpoint: handle[0] endpoint` | - |
| 18 | `lease_ended` | `lease: u64`, `name: string` | - (a send from the steward on the registered endpoint, never answered) |

- `notices` is session class, from a session's badge only; one per session; a later call
  replaces the earlier (the earlier right closed), so a shell that restarts its listener needs
  no `forget`; the right is released with the session (at `Forget`, with the connection it rode
  in on). A handle that is not a send right to an endpoint is `wrong_object` by the kernel's
  name.
- `lease_ended` is sent, never called: the steward's `send` with its badge, bounded as every
  send of the steward's is (`SEND_TIMEOUT`'s kind of bound, no parked call, no retry). A send
  that does not go (the VM's queue full, the session gone) is dropped and the core forgets the
  notice, as the brief says: the steward is never held by a session that does not read.
- The notice names the lease and never why it ended (the lease-supervision edge, unchanged).

`Kernel::notify(session, LeaseEnded)` in the server binds to: look up the session's registered
right; none → drop; else `send(lease_ended)`.

## The shell

At its start the shell makes an endpoint, `serve/1`s it in a listener process (a consumer of
BEAM4's `serve/1`, nothing new in the natives), mints a send right and calls `notices` on the
steward connection. The listener prints `[lease NAME ended]` through the shell's printer, so it is
escaped as any output is and marked in the machine mode as any evaluation's output is (AGT2's
marks: a notice is output outside an evaluation; say on shell.md where such a line lands in the
stream, before the next prompt). A labelled session registers too (one code path) and receives
nothing: notices go to the sponsor's unlabelled sessions only.

## Page lines

steward.md "Leases", after the paths by which a lease ends:

> Each path tells the sponsor: every unlabelled session of the sponsor that registered a
> `notices` endpoint at its start receives one `lease_ended` send naming the lease, never why.
> A session that registered none, or whose VM does not take the send, misses it, and the steward
> is not held: the core forgets a notice no session can take.

steward.md "From the session" (the session-class operations):

> `notices(endpoint)`: a send right to an endpoint the session serves, kept under the session
> and released with it, one per session, a later call replacing the earlier; the steward's
> `lease_ended` notices arrive there as messages, which the shell prints as `[lease NAME ended]`.

sessions.md (the shell's start): one sentence that the shell registers its notices endpoint at
start and prints lease endings. shell.md: where the line lands (before the next prompt; in the
machine mode, outside any evaluation's marks, as a line of its own kind if AGT2's mode has one:
read AGT2's page first and keep its words).

## Residual (state on steward.md Leases)

A sponsor whose sessions all missed the notice has no way to ask which leases ended: a `leases`
listing is not in this package (STEWARD4 or later; the agent's budget is still visible as gone
in the sponsor's shell by its own means until then).

## Amendment (Q1 follow-up): the launcher makes the endpoint, (a2)

The VM has no `endpoint`/`mint` native (bif/system.rs; the only receive right a VM makes is
`launch`'s `serve =>`, which goes to the child). So the notices endpoint comes from the launcher,
as every handle a session starts with does:

- The steward makes the endpoint pair when it launches the session's VM, hands the VM the
  **receive right** as the named startup handle `notices` (one more entry; `ns_lookup("notices")`
  finds it; the shell `serve/1`s it in its listener before its first prompt), and keeps the
  **send right** under the session.
- Stamps, corrected: an endpoint the steward creates is stamped with the steward's own budget
  (under `system`); a session's scope under `users` is not below it (the STEWARD3R correction),
  so the send right is **not** minted in the session's scope. It needs no scope: the steward
  holds the only send right and closes it at `Forget`; the receive right dies with the session's
  process. The page says "released with the session", not "stamped with its scope".
- No `notices` row: opcode 17 is `lease_ended` (`lease: u64`, `name: string`), a send from the
  steward on that endpoint, never a call, bounded as the steward's sends are. A send nobody
  receives in the bound is dropped and the core forgets the notice. A lease can end only after a
  session started one, by which time the shell serves `notices`; a labelled session gets the
  endpoint too and receives nothing.
- No new native: BEAM4's fixed set stays.

Page lines, replacing the earlier ones:

steward.md "Leases": "Each path tells the sponsor: every unlabelled session of the sponsor
receives one `lease_ended` send on the `notices` endpoint it was launched with, naming the lease
and never why; a session whose VM does not take the send in the steward's bound misses it, and
the steward is not held: the core forgets a notice no session can take."

sessions.md (the session's startup handles) and beamlet.md's platform table: `notices`, a receive
right the shell serves; the steward's `lease_ended` sends arrive there as messages the shell prints
as `[lease NAME ended]`. The "From the session" sentence about a `notices` operation is withdrawn.
