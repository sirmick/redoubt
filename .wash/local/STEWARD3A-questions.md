# STEWARD3A: two design questions for the Architect (steward3a-implementer, 2026-10-15)

Base: wp-STEWARD3A at e19e31528 (train 36's head). Read: steward.md "Leases", "The steward's
protocol", "Authentication and sessions", "From the session", R39, R41; agents.md "Leases", "When a
lease ends"; the brief point 1; libs/steward tables/lease.md and session.md; effects.rs
(`carve_session`, `carve_lease`, `notify_sponsor`), guards.rs (`sponsor_session`), machines.rs
(`StartAgent`); servers/steward/src/*.rs and bin/steward.rs; docs/kernel/ipc.md "Authority".

## Q1: the transport of the sponsor's notice

The core emits `Output::Notice { to: Session(badge), LeaseEnded { lease } }`. The brief says "a
`send` on the session's steward connection, which beamlet surfaces as a message". But what the
steward holds per session is the badge it minted on **its own** endpoint (`Made::Connection` of
the steward slot, `Kernel::mint`), and ipc.md "Authority": only the receive right receives, any
other handle is the right to call or send *to* that endpoint. So nothing the steward holds today
reaches a session's VM. The session has no receive right of the steward's either. Options:

(a) **A registration the session makes.** A session-class operation in the wire table,
`notices` (fields: `endpoint: handle[0] endpoint`; no reply fields): the shell's start makes an
endpoint, `serve/1`s it in a listener process, mints a send right and calls `notices` with it;
the steward keeps it under the session (released at `Forget`) and `send`s each notice on it,
one way, within `SEND_TIMEOUT`-like bound; the VM surfaces the send to the listener as a message
and it prints `[lease agent-N ended]` through the shell's printer. A session that never
registers, or never reads: the send is dropped or sits in the VM's queue; the steward is not
held. Costs: a table row, protocol.rs, `Kernel::notify`, a listener in the shell (a consumer of
BEAM4's serve/1 and the generated client), the page line.
(b) **A held call**, as `sshd`'s `watch`: `notices` with no handle; the steward parks the
request and replies it with the next notice (`lease: u64`, `name: string`); the shell re-calls.
Costs as (a) plus per-session parked requests and a bounded queue of notices that arrive between
calls (or they are dropped).
(c) **The kept console**: the steward writes the line on the channel console it keeps for the
session. No VM part, but it is not "beamlet surfaces as a message", it bypasses the shell's
screen and machine mode (an unmarked line in a driver's stream), and a detached context's kept
console is a closed channel.

I recommend (a). Needed from you: the choice; the row's name and fields for
libs/wire/tables/steward.md; the sentence for steward.md "Leases" (and "From the session").

## Q2: the AGENT0 conversion needs the core

The plan body: "a login as agent is the sponsor's lease, its session budget carved under alice's
sub-budget with deadline MAX_LEASE, so alice's sessions can end_lease it and receive its end".
The server cannot do this: the core decides the parent (`carve_session`: `Parent::Sub(cx.domain)`,
the principal's own domain), the deadline (none for a session), the kind (`EndLease` routes to
`Kind::Lease` objects, made only by `StartAgent`/`Granted`) and the guard (`sponsor_session`
compares the caller's account to the lease's domain's). The brief's "Not yours: the core's
guards, rows and constants" and the plan body conflict here; the orchestrator's launch allows
"the steward core's tests and the Elixir oracle unchanged **or extended**".

Also arithmetic (image/README.md): alice's `{}` share is 23,811 pages, two sessions (console +
one SSH login). An agent session (11,392 + its relay) under it needs the image rebalanced:
agent's 24,576 pages and 20 processes folded into alice's top (72,200 pages, 60 processes:
36,100 per set, three sessions each), and alice's `contexts` cap follows (init's default would
be 3). The agent-login case's `users/agent/{} holds ...` expectations move to `users/alice/{}`.

Options:
(A) **Core: a sponsored principal.** The `principal` line gains `sponsor=NAME` (init writes it
from a manifest field `sponsor`; `sponsor=-` or absent for none; init checks it names another
principal, not itself, and no chain for now). Boot carves a sponsored principal no top budget.
`Login` as it runs the session machine as today (context, relay, console, `sshd`'s reply), but
`carve_session` parents at the sponsor's **unlabelled** sub-budget with `deadline = now +
MAX_LEASE`; the session stays in its own domain (audit `1003/{}`, R37 routes), its kernel
account is the sponsor's (R8: inherited). New rows, session machine: `Running`/`Detached` ×
`EndLease` (ahead), guard `sponsor_of` (the caller is an unlabelled session of the domain's
principal's sponsor) → `Ending`: `unroute, drop_requests, audit_lease_ended, notify_sponsor,
reply_ok, destroy_budget`; and every row of a sponsored session that leaves `Running`/`Detached`
for `Ending` adds `notify_sponsor` (which, for a sponsored session, targets the sponsor's
unlabelled domain, not `cx.domain.unlabelled()`). `EndLease { lease }` names the session's id as
the lease id; the reply to its login names it `session-N` as today (or `agent-N`? your call).
(B) **Core: a sponsored login is a `Lease` object** in the sponsor's unlabelled domain, the
lease machine growing the session's context rows (relay, attach, detach, idle, channel_closed).
Much larger; the agent's audit and routes move into alice's domain.
(C) **Defer the conversion** to a follow-up node: STEWARD3A delivers `start_agent`,
`end_lease`, the deadline and the notice; `agent` stays "in name only" and the pages say so.

I recommend (A). Its costs I see: manifest.rs (line grammar, `lines()` writer, `Fixed::new`
checks), store.rs boot carve, guards.rs (+1 guard), effects.rs (`carve_session` parent and
deadline; `notify_sponsor`'s target), tables/session.md (+rows; the generator regenerates
dispatch and diagrams), the Elixir oracle (`guards.ex`, `effects.ex`, regenerated tables; new
hand-written traces so no row is left untaken: run-traces fails on an untaken row), the model's
steward families and `the_reach_table_is_reproduced` (new rows must be reachable, or the reach
table re-recorded: not mine to judge), init's manifest field and check (init.md "The boot
manifest" table), SECURITY.md R39's row, agents.md "An agent is a principal with a sponsor".
If (A), say whether the oracle traces and the model's reach table are in this package or a
sibling's, and who holds libs/steward (a hotspot? the brief says the core is not mine).

## My picks for `start_agent` unless you object (point 1 of the brief)

- The lease's VM: beamlet from `/boot`, start module `Elixir.Redoubt.Agents.Echo` (packed in the
  userland disk with the shell's app), `schedulers=1`, told `principal=<sponsor>` and
  `home=/project`; budget `sizes.agent` (the image's 4,096 pages / 1 process / weight 50; raised
  in image/manifest.json if beamlet's start needs more: `beamlet-footprint` says).
- Its namespace (the binding table for a `Kind::Lease` owner): slot 1 = the sponsor's home's
  `project` directory at `/project`, minted through the home's kept carve (`new_connection`
  rooted at `project`, sharing the home quota); slot 5 = `erofsd:system`; no `/boot`, no vault,
  no `/net`, no console. A read root that is writable is a residual (no server takes a
  read-only connection; the brief's "read ~/project, write ~/project/out" waits on one).
- `Redoubt.Agents.Echo` reads `/project/in` and does what it says: `echo` copies it to
  `/project/out/result` and exits; `spin` loops; `sub <ms>` carves a sub-budget with that
  deadline, starts a thread spinning in it and loops. Cases 1-3 stage `in` through alice's
  session before `start_agent`.
- `DestroyBudget` after the kernel's deadline (the handle revoked, R10): the step treats
  `BadHandle` on a budget whose process already exited as done, so a deadline's end prints no
  "a step failed" line; a `BadHandle` on a live one still fails the batch.

## Q4 (steward3a-implementer-2, 2026-10-15): the lease's `/project` cannot be served as designed

Found with Q3 built (the platform runs without a console; its host tests pass): the agent's VM
starts, boots OTP and runs `Echo.start` in 0.23 s, and its file operations on `/project` fail.
Two walls, both the pages' rules, neither the steward's code; measured on the machine with
temporary probes (evidence: `.tmp/STEWARD3A/exp11/probes.txt`, `exp12/`, in the worktree; the
transcripts of every experiment are under `.tmp/STEWARD3A/exp*/`).

**Wall 1: admission.** `walfsd` admits `in_flight: 2` multiplexed sessions per bucket
(servers/walfsd/src/server.rs `limits`: "one per session, so two lets a share hold one"), and a
bucket is keyed by **account** (serving.md "Buckets": `AdmitKey::of`). Alice's console session
and her SSH session hold the two; the lease is alice's account by design (agents.md: "An agent
shares its sponsor's account"), so the hub's session-opening `collect` on the lease's connection
is answered `REFUSED` in 3 ms and every file operation is `Eio`. With `in_flight: 4` (an
experiment, reverted) the read of `/project/in` succeeds. The same cap refuses alice's **third
live context** its home (the manifest allows her three per set, steward.md "Contexts"): a latent
defect independent of leases, which no case exercises.

**Wall 2: quota.** The lease's connection is minted *below* the home's kept carve
(`fresh_carved`, `below = "project"`, quota 0; steward.md "Leases": "shares the home's quota").
walfsd's ledger (libs/fileserver quota.rs `mint`; walfsd.md "Quotas", R48) makes a mint at a
directory other than the granter's root a **new live root with the quota asked**: 0 bytes. So
`File.write("/project/out/result")` is `Rerror(NoSpace)`; a session's home connection
(`below = ""`, the granter's own root) carves nothing and shares, as the page says. The steward's
design assumed the sharing rule applies below the root; walfsd's rule says a sub-root is a quota
of its own.

Options, wall 1:
(1a) Raise walfsd's (and littlefsd's: same limits) `in_flight` to cover the manifest: the console
session + the contexts cap + leases per account; alice: 1 + 3 + 1 = 5, so 6. Cost per bucket
+4 × 64 KiB; `Limits::fits` against `BUDGET` (3 MiB) with `buckets=6`: 6 × (364,544 + 262,144)
= 3.76 MB > 3 MiB, so `BUDGET` and `heap_pages`/budget of `walfsd:data` rise too (the entry has
1,024 pages = 4 MiB; `BUDGET` is "the bytes its clients may use between them"). The pages: the
`limits` doc comment, walfsd.md/littlefsd.md's admission lines, image/README arithmetic.
(1b) Key a lease's bucket apart (a lease-specific label or badge key): contradicts serving.md
"Accounts, not badges or budgets, because both are cheap to create" and R26's point that an agent
flooding its sponsor's bucket gets at most half. Not recommended.
(1c) Have the lease's home connection be the sponsor's own session's (no new session): the VM's
hub needs a session per connection; impossible as the hub stands.

Options, wall 2:
(2a) Give the lease's connection a quota of its own, carved from the home's at `start_agent` and
given back at the lease's end (the ledger returns a minted root's quota on `disconnect`):
`sizes.agent` gains `quota` (bytes; the image: e.g. 1 MiB of alice's 8 MiB `home_quota`), init
checks it is below every principal's `home_quota`, the steward passes it to `fresh_carved`
(`quota` for a `below` mint; the `Carved` row's `quota` is the home's). `~/project` itself must
have room: the ledger counts what the directory holds (`count`) against the new quota, so a
`project` holding more than `quota` refuses the mint, and the step fails: the shell prints
the steward's "a step failed" line; the agent's `start_agent` answers `failed`. Pages: steward.md
"Leases" (the sharing sentence becomes the carve), init.md's sizes row, agents.md "Leases"
("task-scoped ... 256 MB" already imagines a size), image/README.
(2b) Change the ledger so a 0-quota mint below a live root is a pass-through charged to the root
above (R48 rewritten; littlefsd shares the ledger): touches a Tier A page outside this package.
(2c) Root the lease at the home itself and bind only `/project`: the VM's namespace binds a
connection at a prefix with no sub-path, so the agent would see the whole home: violates the
lease's scope. Not an option.

My pick: (1a) with 6 and the budget raise, and (2a) with `quota` in `sizes.agent`; the question
is whether walfsd/littlefsd and init/image changes are mine in this package (hotspots: none
listed for them) or a sibling's, and the numbers. Until ruled, the three lease cases fail (the
agent runs, its files do not); everything else of STEWARD3A is built and green.

Also measured: with the files failing, the agent's error path (`inspect/1` loads `Inspect`,
`Macro`, `Code.Identifier`...) exhausted the 2,560-page lease budget in 0.8 s (exit 101, the
allocator's panic); `sizes.agent` was measured on `beamlet-agent-footprint`, whose VM also fails
its `/project` read (no binding there) and sits on the same path. The size wants re-measuring on
the echo path once the files work; the footprint case should stage the input it reads.
