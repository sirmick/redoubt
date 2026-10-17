# STEWARD4 ruling (thread `STEWARD4-crossing`)

Questions: `.wash/local/STEWARD4-questions.md`. Brief: `STEWARD4-implementer-2026-10-16.md`.

## Q1: in STEWARD4's scope; init's two changes are the package's

(a) and (b) are ~thirty lines of `servers/init` that case 3 cannot do without, and a separate
node would cost a train for them: they are STEWARD4's. Owned paths gain `servers/init/src/
confine.rs`, `check.rs` (`steward_lines`), their host tests and init.md. The rule, as the page
must say it: the confinement check counts, for a volume a principal's home is on, the
principal's **unlabelled** domain only, because the steward binds a home to unlabelled sessions
alone when the manifest is confined (`confined` is one of the steward's own lines, written by
init); so the static half of R34 (volumes, disks, servers) stays init's, and the one dynamic half
(which domain a home is bound for) is the steward's, trusted base, with the case as its proof.
init.md "The confinement check": the sentence "a confined manifest in which a labelled domain
reads a shared unlabelled volume is refused too" becomes "...is not refused: the steward leaves a
labelled session's home unbound and offers the push"; `init-refuses-confined-server` and the
host test's expectation move with it. (c), (d), (e) accepted: slot 1 unbound for a labelled
session under `confined`, no `home=` told, the shell's one line, the two-disk confined manifest
in tests/data. The read-only code volumes (`/boot`, `erofsd:system`) staying bound for a labelled
session are a stated residual on init.md: the check does not count code volumes today; a
labelled domain reading code is R25's business, not a flow out.

## Q2: accepted, no constant moves

A crossing's `CreateBudget` is stamped with the labelled side's account (`owner.domain
.account()`), as boot stamps a top budget: it bills to the owner (R8), its crash is blamed on
(account, labels) (R40), its connection lands in the account's `{labels}` bucket. The crossing
program makes plain 9P calls, so it holds no `in_flight`; it takes one `State` (cap 8) in that
bucket for at most `CROSSING_LIFE`. walfsd.md's derivation gains that sentence; `in_flight` is
not raised; the image's 6 stays.

## Q3: the probe may live in the steward's binary, under conditions

A measurement of the steward's wake can only be taken inside the steward; a separate program on
a session's badge measures the round trip, not the wake. So `latency-probe` is acceptable as
`restart-probe` is (K23's precedent), with these conditions, each checked by the reviewer:
1. a cargo feature off in every default build; steward.md's test-only features sentence names
   it beside `restart-probe`; the size budget and the unsafe ratchet measure the default build;
2. a bench host test that no manifest or recipe in the tree enables it except `steward-latency`'s
   (the pattern of `every_beamlet_is_told_its_own_budget`);
3. the probe raises only events the core already has (`EndLease` with the console session's
   badge) and touches no guard, row or effect; it prints samples and nothing else; the oracle
   accepts the `[con ID] steward: ` prefix and nothing else changes.
The shape: as proposed. D: 16 at N = 1 and 4, 8 at N = 16 (the VM boots' cost), the toml's
comment saying that p99 at D samples is the maximum and that the pinned seed's sweep stands
behind it; `lease_end_p99_us` stays decision p99 + R10 p99 from the trace, so the measured end is
the destruction and 3Y's cleanup is off it by construction (the one page sentence from the
brief). Every `end_lease` and `submit` prints `steward: served <op> in N us` (point 8).

## Q4: accepted

`too_big` by a `stat` of the source before the event, as `bad_key` is answered before the core;
the step's read capped at `PUSH_MAX` (64 KiB, one lend of 16 pages); a source grown past it
between the two is `failed`, stated on the page.

## Q5-Q7: accepted as written

The forget applied after that decide's batches; `~/out` created if missing, a push's target
directory must exist (`failed` otherwise, on the page); one `crossing` program with its mode as
an argument, serving one call on an endpoint the steward owns and hands the receive right of at
launch (R1), bounded by `CROSSING_LIFE`; `libs/wire/tables/crossing.md`: `read(name) → bytes`,
`write(name, bytes)`.

Nothing here needs the owner.

## Q9: ending the box for the trace dump: (a) with a window marker and a ring check

(a) is within the probe's conditions: it raises no core event, lives under the feature, and
init's reboot rule is the production rule (five restarts in the window → `system_reset`), merely
triggered; a steward exiting at its start serves nobody, so its faults are "blamed on nobody" and
the lockout machinery counts nothing (the measured run is over by then anyway: the file is the
case's last step). Two conditions, both the implementer's to show on the first run:
1. **The window closes at a marker.** The probe prints `LATENCY-END` after its last sample; the
   case writes `latency-done` only after that line; the oracle judges nothing after the marker
   (the six restarts each reap `users`: 21 VMs destroyed per restart, none of them samples).
2. **The ring holds the window.** The scheduler trace is a ring dumped at `system_reset`; six
   reaps of 21 VMs put hundreds of destructions after the measured ones, and a ring that drops
   its oldest entries could evict the measured destroys, so the oracle would report R10 samples
   missing. On the first run count the ring's entries after the marker against its capacity;
   if the window is not whole, fall back to (c) for R10 only: the steward's `destroy_call`
   bracket is call-to-return, a different quantity from R10's kernel time, so R10 is then
   recorded, not judged, and the page says so beside the table. (b), a test-only power-off in
   init, is refused: init gains no test feature for a measurement the steward can trigger.
The run's length: report the first run and the sweep's projection before sweeping, as offered;
if a run exceeds the quiet class's deadline the sweep shrinks (D, then N = 16's sample count),
never the targets.

## Q10: the confinement check does not count an exempt server as a user of an endpoint

Accepted, with the bounding sentence. The check is static and counts domains that *use* a
volume; the steward holds a vault's root badge only to mint each session's connection in that
session's scope (R41), so the using domain is the session's, not the steward's. `users()` and
the endpoint rule skip an exempt server (the steward, sshd); host tests both ways (a labelled
vault handed to the steward passes; the same endpoint handed to a non-exempt unlabelled server
is still refused by the server-instance rule). alice's `net` leaves the confined manifest (a
labelled domain with a scope is refused), so the confined case's alice has no `/net`.

The exemption says only who is not counted. It does not widen what the steward may route, and
init.md must say so in the same breath, because two walls, not the exemption, keep a vault in
its set:

> An exempt server is not counted because it is not the user: it mints a session's connection in
> that session's scope, and the session is the user. The steward binds a labelled volume only to
> a session in exactly that volume's label set (its binding rule, which the model's routes check
> and the vault's `exact_labels` guard keeps), and the server it mints at admits only a caller
> whose labels equal its volume's ([R25 (the label check)](serving.md#r25-the-label-check)), so
> a steward that routed a labelled endpoint to another set would be refused by the server it
> routed to. The steward itself never reads a vault: a declassification reads through a crossing
> carrying the item's labels ([R42](steward.md#r42-one-approved-item)).

So the answer to the question on top is yes: the exemption is safe because R25 at the server and
the core's routing rule hold independently of init's count, and the sentence above is the one to
add so nobody reads the exemption as "the steward may route any endpoint to any set".

## Q11: the latency case's spinners spread over principals: (c)

(b) is out: an admission constant does not move for a measurement, and the derivation's leases
term is the image's rule, not the bench's. (a) measures a load the box will not see. (c) is the
faithful shape and what the box will see: several principals' agents at once. So the latency
manifest carries six principals (test keys in tests/keys, one home each, no net, no vault), the
spinners spread three per principal, the probe's own leases under alice; N = 1, 4, 16 as the
stand-in's, so the table stands beside the stand-in's columns, with one sentence that the load
shapes differ (the stand-in's spinners were plain budgets of weight 100 under `users`; the
steward's are lease VMs under six principals' shares, each a `sizes.agent` budget) and that the
measured quantities are per lease end, whatever carries the load. Cost: six logins per width and
up to 16 VM boots per round; if the N = 16 round exceeds the class's deadline, D shrinks first
(as Q9 ruled), then N = 16 becomes N = 12 (two per principal) with the reason on the page; the
targets never. The manifest's memory follows the arithmetic (16 lease VMs beside six shells:
state it in the toml's comment, under `memory_mib` 1536 or raise it with the number).

## Q12: (a) for STEWARD4; the capacity wall is a node of its own; no owner decision

The finding is real and not STEWARD4's to fix: `admit.rs` counts a connection a client mints in
the share of the connection it minted from, so everything the steward mints at a shared
read-only server (a session's shell and relay connections, a lease's) is a `State` of the
steward's own share at `erofsd:system`, cap 8: the box holds eight steward-minted system-volume
connections in all, whoever carries them (2 sessions × 2 + 3 leases + 1 = 8; 4 sessions × 2 = 8).
3Q derived the per-account caps, but this share is the minter's, not the account's. And consoled's
client pool is a compile-time constant (BUDGET 2 MiB, five buckets), so a manifest with more
principals than buckets cannot boot: serving.md's rule that the manifest sizes each server's
buckets to the label sets it serves is not kept by consoled.

- **STEWARD4 measures what the box admits:** N = 1, 2, 3 under alice alone, the probe's leases
  among them. The table stands with the stand-in's columns at 1, 4, 16; its sentence: "N = 4 and
  16 are not reachable under the real steward: the box admits at most eight steward-minted
  system-volume connections, the steward's own share at `erofsd:system`, so the stand-in's
  columns at 4 and 16 stand alone until that cap is the account's." The finding goes on
  serving.md "Residual risks" ("A root badge's share caps everything minted from it") and on
  consoled.md (the pool's constant), each naming the follow-up without its id.
- **The fix is STEWARD3Z** (drafted under STEWARD3, needs 3Q; Tier A; steward-red-2; size M):
  init hands the steward one root badge per principal at each shared server (principals are the
  manifest's, static: a bounded list), and the steward mints a session's or lease's connections
  from its principal's badge, so the `State` lands in the principal's bucket and 3Q's
  per-account derivation is the whole truth; the alternative, re-homing a minted connection's
  admission to the account that uses it at its first call (a serving-library change), is the
  fallback if the per-principal badges do not fit init's handed list. consoled's limits move
  from constants to the manifest's sizing, as 3Q did for the file servers. Cases: a manifest of
  six principals boots; four sessions and four leases hold system-volume connections at once;
  `steward-latency` regains N = 4 and 16 (STEWARD4's table is completed then, by that node).
- **No decision_request:** nothing a target or a page claim promised moves; the owner learns the
  capacity (three sessions and three leases on today's image) from STEWARD4's report and the
  page, and the fix is a design choice inside the Architect's remit.

## Q13: the two-hart boot failure is a kernel finding; the case gates at one hart until it lands

Facts (steward4-implementer-2, the first full run): at two harts the boot fails in round 1, twice
at the 16th/17th lease, `a step failed (NotPermitted): Launch { .. kind: Lease .. slot: 2 }`
straight after the kernel terminated the previous deadline lease's VM on the other hart
(`[!] Terminating process with PID 280`); the launcher's `NotPermitted` is `p.started() ||
!p.alive()` at the map or start step (kernel/src/process.rs:526, 617) or a dead budget: the new
lease's child was gone before the stub finished launching it. Logs:
`.tmp/steward4-logs/latency-rv64-smp2-notpermitted.log`.

Ruling: not the steward's and not STEWARD4's to fix; a failure that reproduces twice at the
same point is a bug, not a flake. It is a kernel finding, node B82 (drafted): a process being
launched on one hart dies while another hart runs a deadline destruction of a sibling lease; the
candidates the node names (a PID or process slot reused while its termination is still in flight
on the other hart; a destruction's subtree walk or boundary pumps (K19) reaching a process under
a sibling budget; the stub's map/start racing the kill of the child it is launching) are for the
investigator, not a verdict.

Until B82 lands, `steward-latency` gates at one hart (`gate_harts=1`), and scheduling.md's
"With the real steward" states the condition as measured: one hart, N = 1, 2, 3, D = 16, with
the sentence that the two-hart row is recorded when the launch failure B82 names is fixed; the
stand-in's two-hart gate (scheduling.md "At 2 harts the targets above are gated") is untouched.
No target or definition moves: the same numbers, a stated condition and a finding. Do not wait.

The run's length (about six minutes per boot, so a 20-seed sweep is about two hours per width)
puts the sweep in the pinned-seed class as the stand-in's 16-seed sweep is: once behind the
pinned seed when the case lands and after a kernel change the page names, not every train.

## Q14: the wake misses are measured at the page's workload before anything is read from them

The page's workload (scheduling.md "Responsiveness") has the steward stand-in at weight 1,000
from `system`; the real steward runs at 100 under `system` (a quarter of root's weight, about
0.5 % of the hart beside its servers' ~4,900), a lease's VM at 50 under alice's 1,000 of users'
1,500. A timer wake of p50 75 ms against 15 is what a 0.5 % share looks like beside spinning
VMs; it says nothing yet about the steward.

Ruling: the latency case's manifest gives the steward the page's weight, 1,000 under `system`,
and N = 1 is rerun; the table on the page is then measured under the page's workload, which is
the only comparison the stand-in's columns allow. A test manifest's weight is a case condition,
mine. Then:
- if the decision and timer wakes land, the page frames the table "the steward at the
  workload's weight, 1,000 from `system`", and the image's steward weight (100) against
  steward.md's own sentence "it runs at a large manifest weight in the one stride queue" is a
  decision for the owner: I raise it by decision_request with the numbers at both weights once
  the rerun has them, not before;
- if they do not, the miss is the steward's own and the account continues from the trace
  (what the steward's thread was doing when its timer expired).
The deadline notice (p50 368 ms vs p99 40) and the destroy call (292/310 vs 51) are a different
quantity from the stand-in's: a beamlet VM's budget (sizes.agent, 5,248 pages) destroyed instead
of a small lease, and R10's cost follows the dying subtree (K9's rule). Targets do not move for
a measurement: the page states the two columns as what they are (the stand-in's small leases;
the real steward's VM leases, with the pages destroyed per lease beside), and the destroy cost of
a VM lease at the page's targets is a finding to file once the rerun at weight 1,000 shows how
much of it is the share and how much the destruction; its node waits for that number.

N = 3's dropped disconnects at `erofsd:system` are STEWARD3W's (ruled on this thread); N = 3
stays the finding's number, not a tuning.

## Q15: the weight-100 account (rv64, one hart, N = 1, 2, 3)

Figures: `.wash/local/steward4-logs/latency-rv64-weight100-{job.log,smp1.log}`. Decision wake,
R10, lease end met; timer wake missed at every N (p50 50 / 32 / 40 ms against 15, N = 1 with no
spinner live); deadline notice met at N = 1, missed at 2 and 3 by about a millisecond net
(gross 371-578 ms with 5.1-8.2 s of audits inside sixteen windows); destroy_call recorded.

Rulings:
1. **The timer wake must measure the page's quantity.** The stand-in's timer wake is the time
   from the timer's expiry to its thread running again, the thread waiting when it expires. The
   probe arms its 20 ms timer after `start_agent`'s reply and the steward then writes its usage
   and audit lines through `consoled` before it reaches `receive`: a sample whose timer expired
   while the steward was still working is not a wake, it is the steward's own loop. The probe
   arms at the last point before the `receive` that waits on it (or the oracle discards samples
   whose expiry precedes the thread's wait, from the trace), so the column is the stand-in's
   quantity. Mine: probe placement; no target moves.
2. **What that exposes is a finding of its own, measured first.** If the reply-to-receive gap at
   weight 100 is tens of milliseconds, the steward's post-reply console writes hold its loop for
   that long on every `start_agent`: the loop's latency is the thing 3Y protects. From the trace:
   the gap's p50/p99 at weight 100 and 1,000, and what it is made of (calls to `consoled`, their
   count and bound). With the numbers, a node (the lines batched into one write, or written
   after the loop's turn, or the weight); no node before them.
3. **The deadline notice at N = 2, 3** is the VM-lease quantity: R10 9.7 ms p99 for a
   5,248-page lease against the stand-in's 4 ms, the rest the steward's wake and decision. The
   page shows the real-steward column with the pages destroyed per lease and the miss as a
   finding; the target does not move; its node waits for the weight-1,000 figures as Q14 says.
4. **The N = 3 timer-wake p99 (206 ms net)** is attributed from the trace before it is called an
   outlier: a net figure that large after the audit subtraction is either a real wait (what ran)
   or an audit the subtraction missed (an audit record outside the window's records: a bench
   finding, B-node). Say which.
5. The checked build's audit after a VM budget's destruction (300-500 ms each) is the known
   cost of checked audits scaling with the subtree; targets are net, so it moves no verdict, but
   the page's "With the real steward" names it beside the gross figures so a reader of the raw
   log is not misled.

## Q16: the weight-1,000 account; the loop's console writes are a node; the image's weight is the owner's

Figures: `.wash/local/steward4-logs/latency-rv64-weight1000-oldarm-{job.log,smp1.log}`. At
1,000 under `system` the real steward meets every target but the timer wake (decision wake
4.7/4.8 → 5.5/13.6 ms; deadline notice p99 28-32 ms; destroy_call net 6/6-8 ms; R10 9.7 ms;
lease end 23.3 ms). At 100 the decision wake and the destroy call wait out the VMs' slices
(destroy_call net triples; the deadline notice crosses 40 ms at N = 2, 3). The timer wake is
unchanged in substance by the weight (48 / 27 / 33 ms p50): the loop's own work after the reply,
as Q15 read it. The N = 3 206 ms p99 at weight 100 is attributed: a cleanup retry's 100 ms bound
timing out between the arm and the sample (3W's leak), the loop's work.

What `reply_wait` is made of (`say`, servers/steward): each console line is a fresh one-page
Lend, then attach, open, write and close at `consoled`: four round trips; a `start_agent` writes
two lines after its reply (the AgentStarted audit, the usage line): about eight `consoled` calls
and two lend maps per start, bounded only by `consoled`'s turn.

Rulings:
1. **STEWARD3U (drafted): the steward's console lines cost its loop one bound per turn at
   most.** Mechanism, mine: one console fid kept open for the steward's life (attach and open
   once at start and after `consoled`'s restart, as the kept connections are re-bound in 3X),
   the lent page kept with it; a line is one write call. Lines are a queue drained at the loop's
   turn in order, each write within the steward's send bound (100 ms), the turn stopping at the
   first that times out as the cleanup queue does (3Y), so a busy `consoled` costs the loop one
   bound per turn and never four round trips per line in the event's own step. The queue's bound
   is argued: lines per event (at most two today: say the number) times the events a turn can
   raise; a line is dropped only with a console line saying so, never silently, and the audit
   lines a batch raises are written in the batch's order. The page (steward.md "Failure and
   restart", beside the cleanup queue) states it; the re-armed probe's reply-to-receive gap is
   the measure (p99 under a slice at N = 1 on both widths), reported on the real-steward table.
   Not the timer-wake column's business: that column measures the page's quantity once the
   probe arms before its receive (Q15).
2. **The image's steward weight is the owner's** (decision_request on this thread): the trusted
   base's share within `system` is a capacity choice. Recommendation: 1,000 under `system`, the
   page's workload and steward.md's own "a large manifest weight"; the alternatives: keep 100 and
   state the targets as met only at 1,000 (a table condition the image does not meet), or a
   derived weight (the least that meets the targets, found by measurement: more runs for a
   number between 100 and 1,000 that the next VM size moves again).
3. The real-steward table on scheduling.md is measured at the page's workload (the steward at
   1,000) whatever the owner decides for the image; if the image stays at 100 the page says so
   in one sentence beside the table.
