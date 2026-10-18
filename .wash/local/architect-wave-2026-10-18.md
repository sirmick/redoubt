# Architect brief, 2026-10-18: the next wave

You replace architect-2, which sat sealed for 20 hours in a `decision_request` to the owner (the
image's steward weight: 100 today, 1,000 is what the responsiveness targets were stated under).
The owner has not answered; the question stays the owner's. Do not raise a `decision_request`
until your drafts are shipped: it blocks you and nothing reaches you while it is open. Write owner
questions, batched, into `.wash/local/owner-questions-2026-10-18.md` with your recommended
answer each; the orchestrator flashes the owner and relays the answers.

## State you inherit (read in this order, only what you need)

- `.wash/local/handoffs/architect.md` (2026-10-16): the lineage's standing handoff: process, the
  rulings' index, the open QA. Everything after it is in the rulings below and the QA threads.
- Rulings since: `.wash/local/STEWARD4-ruling.md` (Q1–Q17), `STEWARD3Z-ruling.md`,
  `STEWARD3W-ruling.md`, `STEWARD3X-ruling.md`; the reports `.wash/local/STEWARD3V-report.md`
  (part 2: the N = 4 table, Q4 on N = 16), `STEWARD3W-report.md`, `STEWARD3U-report.md`,
  `B88-report.md`, `B87-report.md`. QA threads under `.wash/qa/` (STEWARD3V-budgets open).
- The plan: `.wash/plan.toml`. STEWARD3, STEWARD4, AGENT0 and B79–B88 are done; main is
  f28960f34 (train 62). No leaf package is open. Todo nodes are steps: `sshd` (needs steward +
  SSH1, both done), `agent-suite` (needs sshd), `model-replay`, `BEAM5` (gen_tcp over /net; needs
  BEAM3, done), `smp` (SMP5 parked by the owner), `shell-host`; milestones M3 (AGENTS) and later.
- The orchestrator's log: `.wash/local/orchestrator-handoff-2026-10-15.md`, the sections from
  "~18:40Z" on 2026-10-17 to the end (the findings below with their evidence lines).

## Provisional rulings the orchestrator made in your absence (confirm or overturn, briefly)

STEWARD3V Q1–Q3 (budgets from the manifest, option (a)); STEWARD4 Q17; B82's scope (a kernel
case pins a deadline between create and start, not a kernel change); B83 option A; STEWARD3U
option (b) (one kept console fid, a line one write, a queue drained at the loop's turn); B88's
scope widened to verityd, the fix landed as cache-from-manifest + read-ahead with the rv32
two-hart N = 3 and 4 rows stated missed (compute, not I/O). All are merged; overturning one means
a new node, not a revert.

## Findings awaiting nodes (draft each as a plan node: title, body ≤2000 bytes, tier, reviewer,
size, needs; a brief per Tier A node)

1. **The loop's bound inside a timer-wake window.** 3U and 3W bounded every wait on the steward's
   loop at 100 ms; the targets are 50 ms-class; one bounded attempt landing in a window breaks a
   p99 by construction (STEWARD3V's table: timer-wake p99 missed at N = 3 and 4 at one hart on
   both widths, 69–93 ms, one sample each beside a `disconnect ... parked after 4 attempts` line;
   `.tmp/STEWARD3V/latency4-rv32-console.log` 716–720, 730–743). Options: (a) a bound derived from
   the targets; (b) cleanup steps on a second thread of the steward's, the loop never waiting on
   a shared server (a weight and a State the manifest must afford); (c) non-blocking attempts,
   completion as an event. Your ruling; then the node.
2. **N = 16 measured.** A session with N leases holds 2 + N connections at erofsd:system and 1 + N
   at its home and /boot; the per-account caps (11, 8) admit N ≤ 7 (3V's Q4 on STEWARD3V-budgets,
   built on (a)). A manifest that sizes leases, or the caps raised by the rule: your ruling.
3. **The steward's mints at shared servers wait within FOREVER** (bounded disconnects, unbounded
   mints; max observed 1.43 s; B88's residual). A clean refusal of start_agent, never a retry loop.
4. **Session vs lease under the scheduler.** B88: beside two spinner VMs the shell's launch takes
   5.3–5.7 s against 3.9 s alone on rv32 at two harts (rv64 2.2 → 3.3 s). Whether a session's
   load outranks a lease's is a weight question adjacent to the owner's open one; draft under the
   weights as the image has them today, so the owner's answer changes a number, not a design.
5. **The `sshd` step's first packages** (docs/servers/sshd.md: the box's platform, approve@box;
   docs/userland/sessions.md) and **BEAM5**'s (gen_tcp over /net), if their scope is clear from the
   book; else the owner questions, batched, in the file above.

## Deliverables

- `.wash/local/plan-2026-10-18-wave.toml`: the nodes as `[[node]]` tables (the orchestrator
  applies it with plan_set; never plan_set yourself on started nodes).
- A brief per Tier A node, `.wash/local/<ID>-implementer.md`, in the house style (context rules
  first: `scripts/q run` / `make -f scripts/jobs.mk` for every build and boot; fmt gate is
  `make -f scripts/jobs.mk set CASES=formatting`, docs gate `set CASES=docs`; no edits while cases
  run; messages ≤2000 bytes; pages carry no dates or ids; a miss is a finding, not a tuning).
- One message to the orchestrator: the list, the order you recommend, which can run in parallel
  (target five in flight), and the owner questions file if any.

Never implement, test, push or edit pages yourself; design and plan only. Set waiting and end
your turn after reporting; never poll.
