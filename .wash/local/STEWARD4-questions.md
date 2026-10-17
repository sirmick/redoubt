# STEWARD4 design questions (thread `STEWARD4-crossing`), implementer, 2026-10-16

Brief: `.wash/local/STEWARD4-implementer-2026-10-16.md`. Each question states my default; I build
on the defaults for everything but Q1 and Q3's parts that touch files outside my owned paths.

## Q1: case 3 (confined push) needs init, not only the steward

Facts. `servers/init/src/confine.rs:104-117`: the volume rule counts, for a volume, "every domain
of a principal whose home is on it", so the image's manifest with `confined: true` is refused at
`volumes[0]` (alice's `{7}` domain beside her unlabelled home on `data`). That rule is the page's
"a confined manifest in which a labelled domain reads a shared unlabelled volume is refused too".
The brief's point 4 moves that from init's refusal to the steward's namespace build. Also the
steward has no way to know the manifest is confined: its own lines (`label`, `home`, `vault`,
`console`, `net`) are written by `init` (`check.rs:1079`). And R34's disk rule means the vault
needs its own disk and `blkd` in a confined manifest (the image puts both volumes on `disk0`).

Default: (a) `confine.rs`: for a volume a principal's home is on, count the principal's
**unlabelled** domain only (the steward binds the home to unlabelled sessions alone when
confined), with the host test's expectation moved and the page's sentence rewritten ("the steward
leaves a labelled session's home unbound and offers the push"); (b) `init` appends one own line,
`confined`, to the steward's lines when the manifest sets it (`check.rs steward_lines`, host test);
(c) the steward's `binding` leaves slot 1 unbound for `Child::Session { labelled: true }` under
`confined`, so the VM is told no `home=`; (d) the shell prints one line at its start when it is
labelled and was told no home: `[no shared home in this label set: a push is the way in]` (no new
argument, no platform change); (e) `tests/data/steward/confined.manifest.json` + a disk recipe:
alice alone (no bob, no agent, no net), `data` on disk0 with its `blkd`, `alice-secrets` on a
second disk with `blkd:vault`, the system volume as the image's. `/boot` (bootfsd) and
`erofsd:system` stay bound for the labelled session: read-only code volumes the confinement check
does not count for a labelled domain today; stated on init.md as a residual, not changed here.
(a) and (b) are in `servers/init`, outside my owned paths: your word.

## Q2: the crossing budget's account and the admission arithmetic

Facts. `carve_crossing` parents the budget at `Parent::Users`; the server's `spec(.., account 0,
..)` would inherit `users`' account, which is 0 (init.rs:438), so a crossing's vault connection
would land in an account-0, per-badge bucket (serving.md "Buckets"), its crash blame counting
nowhere, and a new bucket per crossing against `buckets=5` at `walfsd:alice-secrets`.

Default: the server stamps a crossing's `CreateBudget` with the labelled side's account
(`owner.domain.account()`), as boot stamps a top budget: it bills to the owner (R8), its crash is
blamed on (account, labels) (R40), and its connection falls in the account's `{labels}` bucket.
The crossing program makes plain (unmultiplexed) 9P calls, so it holds no `in_flight` (that cap
counts multiplexed connections' completion calls: walfsd `server.rs:22-31`); it takes one `State`
(cap 8) in that bucket for at most `CROSSING_LIFE`. So the derivation gains the sentence, and no
constant moves (the image's 6 stays). Say if you want `in_flight` raised regardless.

## Q3: `steward-latency`'s shape

Facts. The real steward arms no timeout but the idle one (`next_idle`); it never destroys a
lease by its own decision; a lease's VM is beamlet (`sizes.agent`, 5,248 pages), so each
destruction costs a VM boot; the steward's console lines reach the log as `[con ID] steward: ...`.

Default: a test-only feature `latency-probe` (off in every default build, as `restart-probe`):
(1) after each `start_agent`, the steward arms a timeout (`DECIDE_US`) and at its wake ends the
newest lease itself (the core's `EndLease` with the console session's badge, reply 0),
printing `decision_wake` and `timer_wake` samples (`time_now` less the deadline armed), and a
`destroy_call` sample around `DestroyBudget` (recorded, not bounded); (2) each lease exit notice
prints `deadline_notice` (arrival less the lease's deadline) for a lease its deadline took;
(3) the group is the count of the domain's other live leases at the sample (`N=1`, `N=4`,
`N=16` when the case keeps one probe lease beside N spinners); (4) `sched_oracle` accepts the
`[con ID] steward: ` prefix before `LATENCY-SAMPLE`/`LATENCY-COUNT`, nothing else changes;
`lease_end_p99_us` stays decision p99 + R10 p99 from the trace, so the measured end is the
destruction (3Y's cleanup is off it by construction). The case: a manifest of its own with alice's
top at 131,072 pages (16 leases), `memory_mib` 1536, `icount shift=3`, checked build with
`sched-trace`; per N, N spinning agents, then D leases ended by decision and D by deadline, D = 8
(p99 is the max at that count; 69 VM boots in all). Every end_lease and submit always prints
`steward: served <op> in N us` (point 8). Say if D or the probe's shape should differ.

## Q4: `too_big` for a push over `PUSH_MAX`

A failed `Read` step answers the core's `failed`, not `too_big`. Default: the server stats the
source before the event and answers `too_big` itself (as a login's unknown label is `bad_key`
before the core); the step's read is capped at `PUSH_MAX`, so a source grown past it between the
two is `failed`. `PUSH_MAX` 64 KiB, one lend of 16 pages.

## Q5-Q7 (FYI, server-internal)

- Q5: the core emits `Forget(request)` in the same `decide` as the copy-out's or write's batch,
  before the drive runs it; the server applies a request's forget after that decide's batches, so
  the crossing still finds its item's path.
- Q6: `~/out` is created if missing; a push's target directory must exist (the writer's
  connection is rooted there; a missing one fails the mint, `failed`): both on the page.
- Q7: one program, `crossing`, with its mode as an argument; it serves one call on an endpoint
  the steward creates and hands it the receive right of at launch (R1: the endpoint's owner is the
  steward, so the labelled program never starts a flow), bounded by `CROSSING_LIFE`; wire table
  `libs/wire/tables/crossing.md`: `read(name) -> bytes`, `write(name, bytes)`.
