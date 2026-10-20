# READ1 ruling (thread READ1)

Phase-1 measurement: `.wash/local/READ1-measurement.md`. The death is **admission**, not a drain
rate or a timeout: the `Files` cap (32 per account/label set) fair-shared `max(1, 32/(holders+1))`
across the account's connections, each connection's root fid (Tattach) consuming its share, so at
n = 16 connections every connection is at one fid (the root) and can walk nothing — module loads
refused `too many open files`, the VM dying variously (including a silent forever-park). Reframes
finding 5: the pages/requests term is NOT on this path. The five decisions:

## 1. The Files cap and share rule → (d): derive the cap (a) AND root out of the share (b), floor the working share at one

Both (a) and (b), which is the implementer's (d), with the floor as a working-fid floor:
- **(a) The `files` cap is derived from the account's connection count**, as STEWARD7 derived
  `state`/`in_flight` — the same derivation gap, now for `Files`. `files: 32` stops being a
  compiled constant; the manifest sizes it to what the account's connections need, held by the
  bench host test (extending `every_manifest_entry_holds_its_buckets_at_their_caps`).
- **(b) The root fid is out of the fair share.** A connection's root fid (held for life) is a
  fixed per-connection cost charged to the bucket total (bounded by MAX_FIDS and the budget); the
  R26 fair share `max(1, pool/(holders+1))` is over **working** fids (walk/open/read), so every
  connection always holds its root PLUS a working-fid share. This is the specific fix for the
  n = 16 collapse (the root consuming the share).
- **Floor the working share at one**, so each connection always has root + ≥1 working fid (a
  connection can always walk); the derivation gives the margin above that (the shell's hub needs
  root + cwd + a working fid = 3 at the data volume; a lease needs root + 1). So the cap is
  derived from the connection **mix** (hubs vs leases), not a flat count.

This is my remit (an admission derivation, STEWARD7's rule extended). The supported count becomes
the derived cap's; today's baseline (decision 4) is what the page states until the fix.

## 2. A refused code read → both: retry the refusal, and a VM that cannot start exits, never parks

- **(a) The synchronous client retries an admission refusal (`too_many`, `busy`)** a bounded
  number of times with backoff, as the hub retries `busy` at `RETRY_US` up to a bound — so a
  transient fid-pressure refusal (a working fid momentarily in use by another of the account's
  connections) rides out. With decision 1 the structural cap is sized, so retries are for the
  boundary, not permanent exhaustion.
- **(b) A VM that cannot complete a required startup read, after the retry bound, exits with a
  diagnostic — it never parks silently forever.** The silent-park death (boot-pack / start-module
  read refused → `sleep(FOREVER)`, a lease holding its budget to its deadline with no line) is the
  worst outcome and goes in **every** build: the VM exits and the steward's `lease … exited N`
  line says so. The specific-kind naming (`exit-kinds`) may stay test-only, as `restart-probe`
  is; the loud exit-instead-of-park is the default. A runtime (lazy) load refused still raises
  visibly (not a silent park).

## 3. The data volume's slow writes → a separate finding, stated; a node only if a write SLA is needed

A 2 KB write to `/project` taking 8–29 s, two `etimedout` at the hub's 10 s, is walfsd's and the
hub's **write-path throughput** under N concurrent writers — distinct from the Files-cap death
(decision 1 fixes the fid exhaustion at both volumes). State it on walfsd.md as a finding (an
agent's write to `/project` can be slow or time out under its account's concurrent load). A node
(the write-path throughput) only if the agentic workload needs a write latency bound — deferred
like the other throughput findings, not drafted now. READ1 does not fix it; it names it.

## 4. The supported count and rates → state them as the measured baseline

On erofsd.md / walfsd.md / verityd.md and scheduling.md (the latency rows name READ1): today's
admission ceiling is **13 live leases of one account beside two sessions** at the system volume,
and **7** for anything the shell's hub needs at the data volume; with the rates (launch 1.3–11.5 s,
start-up 35–143 s, loads 30–85 KB/s per agent, verityd ~10 % held). After decision 1 the supported
count is the derived cap's; the page states the baseline and that READ1 raises it. The read path's
own slowness is stated (never a death cause); the weight lever moves the rates, not the edge.

## 5. Phase 2's case → the gate as the implementer shapes it

Accept: the case retargeted as a gate — **N agents of one account all loading and finishing, none
refused, N the stated supported count**, both widths at one and two harts (gated where SCHED5
allows two harts), the shell's probes the account-wide check; plus a **host test of the share
arithmetic at the derived cap** (root out of the share, the working floor, the connection-mix
derivation) across every manifest, beside STEWARD7's state/in_flight test. `exit-kinds` test-only
(decision 2b makes the loud exit default; the kind naming stays the feature). Phase 2 is the fix
(decisions 1 and 2) + this gate; bring the cap derivation's numbers (the per-connection fid need,
the derived cap, the new supported N) back on this thread before the fix lands.

## Tier and scope

Tier A (admission in the shared file servers, the serving library's fid accounting, the VM's
startup failure behaviour). The fix touches `libs/rt/src/server/admit.rs` and `ninep.rs` (the
share rule, root out of the share), erofsd/walfsd/littlefsd `limits` and their derivation, the
sync client's retry, beamlet's startup-read failure (exit, not park), the pages, and the bench
host test. Size M–L; steward-red or a red the orchestrator picks. Keep decision 3 (write
throughput) out of scope (a named sibling finding).

## Cap derivation signed off (thread READ1, phase 2)

Rule: `files = n + w*(n+1)` — n the connections the account may hold at the server (each a root,
the fixed cost, = the state cap's n so roots <= state < files), w the connection's working-fid
need, the working pool `w(n+1)` making the R26 fair share exactly w. The w(n+1) pool, not the
mix-sum (hubs*3 + leases*2), because the sum's integer share gives the hub one and re-creates the
emfile. Numbers: erofsd:system 32 -> 57 (w=1), walfsd/littlefsd 32 -> 71 (w=2: hub cwd + work),
bootfsd 32 -> 47 (w=1); entries move only walfsd (2,048 / 4,096), others hold. Supported N:
latency 16 leases + 2 sessions (was 13/7), image 7 with margin (was 7 at the edge). **w = 1 at
erofsd:system is the need, not margin** (no cwd there, the hub serial, one working fid at a time);
the bounded retry rides a rare two-fid moment; confirm from the hub that a system-volume
connection holds at most one working fid, else w=2 with the reason. w=2 at walfsd is the hub's
measured cwd+work need. Signed off; the fix proceeds.

## Decision 2b amended to (b') — console as the criterion (thread READ1)

My "exit everywhere" was wrong: the silent park is a deliberate rule (R75/R76 — a tampered disk
must not become an init restart loop to a reboot; userland-bad-start and the pack-bad cases forbid
`init: restarted`/`init: rebooting`), and exiting everywhere would make a flipped console-session
module a reboot loop, losing R75's "no reboot" verdict. Ruled (b'): the criterion is the console.
A VM WITH a console says why and parks as today (its line is heard; the restart-loop rule and the
four cases stand). A VM with NO console (a lease) exits with the diagnostic as its code (the only
thing the steward's `lease ... exited N` line carries); a lease is not auto-restarted in a loop
(the sponsor retries by hand, bounded by admission), so no reboot loop. Same behaviour for an
admission refusal (READ1's cause) and a corruption refusal. Condition: a console-less VM's clean
exit (a code, not a fault) must not count toward crash-blame. The page states the two ends by the
console.

## Phase-2 gate: the verdict channel confirmed; the write hang → WRITE1 (thread READ1, 2026-10-20)

Verdict = the steward's console (sixteen named `exited 0` lines, non-zero forbidden), the session's
sixteen lease-end notices, the shell's probes at both volumes; the data volume out of the verdict.
Conditions: named agents in the expect list; a refused load cannot exit 0; confirm the leases still
attach `/project` so walfsd's cap is exercised, else state that the host test and probe hold it.
The write whose completion never returns is a liveness bug and its own node (WRITE1, Tier A,
investigation-first: hub vs walfsd); READ1 states it on walfsd.md "Residual risks" beside decision
3's slow writes, as a distinct sentence. Detail: wave-2026-10-18-rulings.md "READ1 gate verdict +
WRITE1"; plan `.wash/local/plan-2026-10-20-write1.toml`.
Conditions closed (2026-10-20): (b) Echo halts non-zero on a refused load, a wrong module or any
exception; (c) every lease attaches its home at start (`Namespace::from_startup`), so walfsd's 71
is exercised by sixteen roots and input reads; (a) sixteen `exited 0` lines matched by
`agent-[0-9]+` in any order ARE the count — a lease exits once and prints once — so no shell-side
name set (a second claim at the less trusted party for what the steward's count proves).
The orchestrator had the shell-side name set added before my "no" arrived: not wrong (a conjunction
only tightens), redundant; the orchestrator's call under the simplicity gate. The point that matters:
the session's wait on sixteen lease-ended notices (set check or not) rides on notice delivery, which
has STEWARD12's lost-notice residual (a send past NOTICE_US dropped) until SHELLN1; the exits are
staggered by load-finish, so a loss is unlikely, but a lost notice must fail the case loudly
(bounded wait, a line), never hang it to the bench timeout.
Final (2026-10-20): wp-READ1 85afec5f8, 88-case set green both widths, landed as ruled (case
unchanged, `whole_run = false` with the reason, pages state fifteen of sixteen and the mechanism).
The kill reproduces on all four configurations with the victim varying (agent-11/12/14/15), always
the instant a neighbour's lease ended, before any end of its own: KILL1's evidence at
`.tmp/READ1-evidence/` (gate10, gate13). The rv64 one-hart SSH close was seen once and did not
recur: recorded as seen once, unreproduced — not a finding, not yet explained either.
