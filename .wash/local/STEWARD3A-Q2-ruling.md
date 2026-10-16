# STEWARD3A Q2 ruling: (A), a sponsored principal in the core; the core, oracle and model move together in this package

Thread `STEWARD3A-sponsored-login`. Question: `.wash/local/STEWARD3A-questions.md` Q2 and "My
picks for start_agent".

## (A), and why

agents.md already says what a sponsored principal is: its own principal, never an impersonation
(audit under its own domain), its budget under its sponsor's so it bills to the sponsor's account
(R8), the sponsor can destroy it at any time, and a crash loop is stopped at the sponsor (R40 keys
by the kernel's account, which is the sponsor's). (A) is that page made mechanism, with the
session machine kept: the harness (AGT2's mode, AGT3's compile, agent-login) sees a session.
(B) moves the agent's audit and routes into alice's domain, against "never an impersonation".
(C) leaves AGENT0's goal in name only when the owner's order is STEWARD3 now.

## The rules (A) keeps, as the core must state them

1. `principal` line: `sponsor=NAME` (absent or `-` for none); init writes it from the manifest's
   `sponsor` field and refuses a sponsor that is not another principal, the principal itself, or
   itself sponsored (no chains in this package: agents.md's chain is M3's later work; say so).
2. Boot carves a sponsored principal no top budget. `Login` runs the session machine as today;
   `carve_session` parents a sponsored principal's session at the sponsor's **unlabelled**
   sub-budget with `deadline = now + MAX_LEASE`. A sponsored principal's login in a labelled set
   is refused (`labelled`): an agent's vault access goes through `submit` and an approval
   (brief point 2), never a login.
3. The session stays in its own domain: audit `<id>/{}`, R37 routes, R36 ids from the one id
   space (a lease id and a session id never collide; `EndLease { lease }` names the session's id).
4. New session rows: `Running`/`Detached` × `EndLease` (ahead of admission), guard `sponsor_of`
   (the caller is an unlabelled session of the domain's principal's sponsor) → `Ending`:
   `unroute, drop_requests, audit_lease_ended, notify_sponsor, reply_ok, destroy_budget`. Every
   row of a sponsored session that leaves `Running`/`Detached` for `Ending` (exit, idle, deadline,
   blame, end_session) adds `notify_sponsor`, aimed at the sponsor's unlabelled domain.
5. Names: the login's reply stays `session-N` (sshd and the harness unchanged).
   `lease_ended.name` is `<principal>/<name>` for every lease: `agent/session-7` for a sponsored
   session, `alice/agent-3` for a `start_agent` lease. One rule, the shell prints it as is.
6. The deadline's end is the kernel's; the steward's `Exited` from the notice runs the row as
   for any session's end, plus `notify_sponsor`.

## Ownership in this package

- `libs/steward` (manifest.rs, store.rs, guards.rs, effects.rs, tables/session.md, the
  generator's outputs) is yours here: the brief's "not yours" was written for a server-only
  package; the plan body overrode it. STEWARD3R step 2 touches the server's `Connect` and the
  serving library, not the core's tables; it rebases onto your branch if both are open.
- The Elixir oracle (guards.ex, effects.ex, regenerated tables, hand-written traces so no row is
  untaken) and the model's steward families are yours too: the core, its oracle and the model
  move together (STEWARD0's rule: one state machine shared by server and model). The reach table
  is re-recorded only for rows the generator reaches through the fixed manifest: the model's
  manifest gains one sponsored principal; no new random draw (model-generator rule: a new
  variant picks from values already drawn, or seeds shift and the mutations' deadlines break).
  `the_reach_table_is_reproduced` must then pass with the new rows reached, and the three
  families' seed times stay within MODEL1's bounds (quote them).
- init: the manifest field, the check, init.md's table row; SECURITY.md R39's row; agents.md
  "An agent is a principal with a sponsor" status; image/README.md's arithmetic.

## The image's arithmetic

Fold `agent`'s top budget into alice's (a sponsored principal has none). The constraint the
image must meet, stated in image/README.md: alice's `{}` share holds at once her console
session, one SSH session, the agent's sponsored session and one `sizes.agent` lease; bob's halves
unchanged. `sizes.agent` is not 4,096: a beamlet VM's idle peak is ~5,542 heap pages with the
shell loaded; the agent VM (its start module, `schedulers=1`, no shell app) is measured in
`beamlet-footprint`'s manner with `report_memory` (a `beamlet-agent-footprint` run, both widths)
and `sizes.agent` is twice its idle peak plus what it holds outside the heap, rounded up to 128,
by the page's rule. If the sum exceeds alice's top after the fold, the vault set's share may give
way only while it still holds two sessions (its present need); if it cannot, bring the numbers to
this thread before changing any top budget. `contexts` cap for alice follows init's default from
the share (3 if the arithmetic gives 3). agent-login's `users/agent/{}` expectations move to
`users/alice/{}` with the sponsored session named.

## Your picks for `start_agent`: accepted, with three notes

- The VM: as you say, with `sizes.agent` from the measurement above, not the placeholder.
- The namespace: slot 1 the sponsor's home's `project` at `/project` through the home's kept
  carve (quota shared), slot 5 `erofsd:system`, nothing else. The writable read root is a stated
  residual on agents.md "Leases" ("read `~/project`" waits on a read-only connection no server
  takes yet).
- `DestroyBudget` after the deadline: `BadHandle` counts as done only when the core has already
  taken that budget's `Exited` (the kernel's notice came first); a `BadHandle` on a budget with
  no exit taken still fails the batch. Bind it to the event, not to the handle's state.

## Checkpoint

After the core rows, the oracle traces and the model's families are green on the host (before
the machine cases): one progress line with the branch, the row count added, the three families'
seed times, and the agent VM's measured peak.

## Q3 (the lease's console): (a), the platform tolerates no console

beamlet's platform opens `/dev/cons` at its start (userland/otp/redoubt/src/lib.rs:291) and exits
1 without one; a lease's namespace has no console by the page's word. The platform must not
require what the page denies: `Console::open` becomes optional in `Redoubt::new`; with none,
console writes are dropped, reads answer end of input, the size is the default 80x24 (the shell
never runs there; the Erlang `io` of an agent goes nowhere but `/project/out`, which agents.md
says). The `Arc<Console>` threaded through the hub, the io and the size query becomes an
`Option`, one path; a host test runs a VM with no `/dev/cons` through its module to exit. Built
in this package (Tier A platform boundary, steward-red-2 reviews it); beamlet.md's platform
table gains the sentence that `/dev/cons` is optional and what its absence means.

(b) is refused: an agent on the UART is a channel to the box's console the page denies it, and
the residual would be a lie about containment. (c) is (a) with a server for nothing.

The steward's line when a process it launched ends stays, as the lease's end audit while audit
records are console lines (STEWARD2's residual): `steward: lease NAME of ACCOUNT/{} exited CODE`
(`faulted`, `was killed`), with NAME the notice's name (`alice/agent-1`) so the two lines
correlate; init.md/steward.md's console-line table gains the row.

## Q4 (the lease's files): (1a) in a sibling node, (2a) here

Wall 1, admission: (1a). A bucket is keyed by account by design (serving.md "Buckets", R26), so
the cap per bucket must hold what the manifest lets one account hold at once: the console
session, the contexts cap and the leases per principal, plus one spare for a context detaching
while another attaches (its connection is live until the relay ends). For the image: 1 + 3 + 1 +
1 = 6. The constant is not a guess any more: walfsd's and littlefsd's `in_flight` are stated as
that derivation on the page, and a bench host test holds each server's constant to the largest
such sum over the manifests in the tree (as every_beamlet_is_told_its_own_budget did for pages).
`BUDGET` and the servers' heap caps follow `Limits::fits` with the new count, the arithmetic on
walfsd.md/littlefsd.md and image/README.md. The latent third-context defect (alice's third live
context refused its home) gets its case there: three contexts live, each reading its home. This
is a sibling node, STEWARD3Q (Tier A, steward-red-2, size S), launchable now; 3A's lease cases
run on its branch until it merges. (1b) is refused (against serving.md's keying and R26); (1c)
impossible as the hub stands.

Wall 2, quota: (2a), in 3A. R48 stands as written (a sub-root is a quota of its own): the lease's
connection gets a quota carved from the home's at `start_agent` and returned by the ledger at
the lease's end (the disconnect in STEWARD3R's cleanup). `sizes.agent` gains `quota` (bytes); the
image gives 1 MiB of alice's 8 MiB `home_quota`; init refuses a quota not below every
principal's `home_quota`; the steward passes it to the `below` mint. A `~/project` holding more
than the quota refuses the mint, the step fails, `start_agent` answers `failed`: stated on
steward.md "Leases" (the sharing sentence becomes the carve) and agents.md "Leases" (the
task-scoped line names the quota among the lease's sizes); init.md's sizes row. (2b) rewrites
R48 for one caller; (2c) breaks the scope.

`sizes.agent` (2,560 pages) was measured on a failing path: re-measure on the echo path once the
files work, with `beamlet-agent-footprint` staging the input it reads, by the twice-the-idle-peak
rule as before.
