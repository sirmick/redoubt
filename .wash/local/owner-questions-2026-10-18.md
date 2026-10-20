# Owner questions, 2026-10-18 (architect-5)

Batched; each with the Architect's recommendation. None blocks the wave's nodes: they are
drafted under today's image, and an answer changes a number or adds a node, not a design.
Context: `.wash/local/wave-2026-10-18-rulings.md`. Please answer on this file's thread through
the orchestrator; only an actual human answer authorizes a change.

## Q1. The image's steward weight (re-raised: architect-2's request died unanswered)

`image/manifest.json` runs the steward at weight 100 under `system`. scheduling.md's
responsiveness targets were stated with the steward stand-in at 1,000, and the real steward
meets every target but the timer wake's one-sample p99 only at 1,000 (STEWARD4 Q15/Q16: at 100
the decision wake and the destroy call wait out the VMs' slices; the deadline notice crosses
40 ms at N = 2, 3). `steward-latency` measures at 1,000 whatever the image says; today the page
says in one sentence that the image runs it at 100.

- **Recommendation: 1,000 under `system`.** steward.md's own sentence is "it runs at a large
  manifest weight in the one stride queue"; the trusted base's responsiveness is what the
  targets protect, and 1,000 is a quarter of `system`'s root weight beside the servers' ~4,900,
  about 17 % of `system`, not a takeover.
- Alternatives: keep 100 and leave the table's condition as stated (met at 1,000, which the
  image does not run); or a derived weight, the least that meets the targets, found by more
  runs and moved again by the next VM size.

Cost of the change: one number in `image/manifest.json` and the memory-checked manifests, the
page's sentence, a train. No node is blocked on it.

**Data point (STEWARD7's N=16 run, bearing on this and Q2):** at one hart a read-path server at
weight 100 (`erofsd:system`) starves under sixteen lease VMs of weight 50 (800 total on one
hart) — it cannot drain ended leases' disconnects as fast as leases end, so their slots linger
and a new launch is refused though logically there is room. At two harts it keeps pace. This is
oversubscription, not a bug, and the one-hart N=16 row is stated unreached (no target moves). But
if the agentic workload is meant to run many agent VMs beside the shared servers on few harts,
the read-path weights (not just the steward's) are the lever. A single answer that sets the
trusted/shared servers' weights relative to N agent VMs of weight 50 would settle Q1, Q2 and this
together; my recommendation stays steward 1,000, and I would raise the read-path servers
(`erofsd:system`, `verityd`, `blkd`) only if a measured agent workload on the target hart count
shows them starving there, not for this one-hart oversubscription.

## Q2. A launching lease's share against its sponsor's spinning leases (B88's residual)

On rv32 at two harts a lease VM's launch beside two spinner leases takes 5.3–5.7 s (3.9 s
alone): one of three equal `agent` budgets (weight 50) on two harts. STEWARD8 states the launch
as a measured quantity and derives the shell's `start_agent` bound from it, so the rows are
measured; the question left is whether a launching lease should outrank its sponsor's spinning
leases.

- **Recommendation: no.** A lease's share is the manifest's `agent` weight for its life; a
  weight step at launch (the steward carving at `session`'s 100 and lowering after start, which
  the kernel supports: "the lead follows the weight") would make a flood of launches a way to
  take CPU from running agents, and the pages would carry a second weight per lease. The
  derived bound is the honest statement.
- Alternative: a launch weight for leases (`sizes.agent.launch_weight`), lowered at
  `process_start`: a steward mechanism, a manifest field, a page rule and a case.

## Q3. Struck: moot. STEWARD7's arithmetic shows `memory_mib` 1536 and R4a's headroom both afford N = 16 under the ruled derivation (rulings file "F2 Q5", "F2 Q6"); no owner choice remains. Kept for the record:

### (was) The latency case's N = 16 round: memory for sixteen leases (conditional)

STEWARD7 derives a principal's leases from its share, so N = 16 under one principal needs the
latency manifest's alice at about 190,722 pages (745 MiB; her two label sets double the share's
95,360). The implementer does the memory rule's arithmetic under `memory_mib` 1536 beside the
five other principals. If it fits, no question. If it does not:

- **Recommendation: raise the case's `memory_mib` to what the arithmetic needs (2048 at most),
  with the number in the toml's comment.** The stand-in's N = 16 column is the comparison the
  page promised; a case's machine is the bench's to size.
- Alternative: record the largest N the machine gives (8 or 12) with its arithmetic beside the
  stand-in's column at 16, and leave 16 unreached on the real steward.

## Q4. How many TCP sockets a person's session may hold (BEAM5's residual)

`ipd` admits 8 sockets per bucket by default, shared fairly among the badges in the account's
bucket (`limit / (n + 1)` each); a session's `/net` connection takes one unit, each socket one,
and a closed socket keeps its unit through TCP's TIME-WAIT (10 s): that is the settled rule
(ipd.md: a socket "stays charged to its owner until it is gone"), and it stands. With alice's
console session beside her SSH session the image gives each about three sockets: BEAM5's
diagnostic boot met `emfile` on a fourth connect within 10 s of three closes. An HTTP client
opening connections in succession meets it at once. The mechanism to change it exists: the
manifest's `limits=BADGE:INFLIGHT:STATE:SOCKETS` override per scope badge, and the entry's heap
sized to it (each socket's buffers live there).

- **Recommendation: the image gives alice's `net` scope `limits` with 32 sockets** (a
  browser-class client holds about six connections per host and churns them through TIME-WAIT
  over 10 s; 32 holds that with room for the console session's share), `ipd`'s entry re-derived
  by the rule with the arithmetic on ipd.md; bob unchanged (no `net`). A node (Tier A, S) when
  you answer.
- Alternatives: raise the default sockets per bucket for every account (every deployment pays
  the heap); or leave three and state it on sessions.md as what a session gets.

## Q5. gatewayd and the agent-suite's second wave (the harness, the loop, the escape room)

The `agent-suite` step's attack cases and Alice's scripted agent under a lease are built or
buildable now (ATK10–13, and the kernel/steward halves). What remains of the step — the agent
harness (`Redoubt.Agent.start/prompt`), the pi-small agent loop, and the escape room — is marked
**M5 (self-hosted development)** on agents.md and in m5-self-hosted.md (steps 2, 6, 7), because
each needs `gatewayd`: the loop streams the model through a `gatewayd` capability, the harness
grants `gateway:`, the escape room records B's egress through it. `gatewayd` is a new egress
server that holds credentials and speaks TLS (R65/R66) — the allowed-egress wall, owner
territory. So the step cannot complete past the attack suite without a decision on gatewayd.

- **Recommendation: keep the harness, loop and escape room at M5, as the book places them, and
  close `agent-suite` now at the attack suite + scripted agents.** The milestone's goal
  ("every property backed by an attack case, the attack suite passes") is met by ATK10–13 and
  the built halves; the agentic loop is the next milestone's, where real model egress belongs.
- **If you want the agentic loop pulled into M3 now** (the project's north star — the most
  capable agents running and attacked): build gatewayd **loopback first** — its capability shape,
  R65/R66 checks, name-allowlist, the label sink and the request log, with the "model" a bench
  peer and no real TLS or API key — so the harness, loop and escape room can be built and
  attacked at once; the real TLS-and-keys gateway to one provider lands with M5's model egress.
  This sequences the risk: the capability machinery and the escape room (the highest-value test)
  come now; the credential-holding TLS server comes when real egress is wanted. I would then
  draft gatewayd-loopback (Tier A, owner territory for the new server's wire and scope) and the
  harness/loop/escape-room wave against it.
- Alternative: build the full gatewayd (TLS, keys, one provider) now — the largest option, and
  it needs the provider and resolver choices the owner has not set.

Your one answer (keep M5 / loopback-first in M3 / full gatewayd in M3) sets the step's second
wave; until then I draft nothing past ATK13 for `agent-suite`.

## Noted, no question

- BEAM5's second residual (with a pty the bench's `exit` does not end the VM) is not a finding:
  closing a pty channel's input detaches the context by design (sessions.md "Closing SSH
  detaches"); a pty case ends its session with `exit` at the prompt, then the bench's `ended`
  step. No node.

- The `sshd` step's two items (vault sessions over SSH, `approve@box`) are built with their
  cases; the orchestrator closes the step; SSH3 (pre-login connections bounded in time, slots
  from the manifest) is its one follow-up and runs last in the wave.
- BEAM5 is designed without a new native (Tier B, red review for the attack case); if it needs
  one it is re-tiered and comes back as a question.
- B53 (awaiting the owner, per the orchestrator's log) is not the Architect's and is not
  repeated here.

## Follow-up (Architect, 2026-10-19, STEWARD9): a session cannot observe its own lease's end
lease_ended (op 17) is consumed by the shell's notices listener; no lease-liveness op. The latency case uses a guest-clock derived wait (deadline 14 s + 1 s over R10) at N = 16. Design answer belongs to the agent harness (Q5): Agent.status poll or the shell forwarding lease_ended to session code. Not a node until the harness wave.

## Follow-up (Architect, 2026-10-20, WAKE1): the shell's per-line print cost
A [lease ended] line costs 250-370 ms against consoled's 6.5 ms per reply, now the visible term of a burst's last notice. Not a node yet: entry condition is one measurement, a shell print at idle on the release build and where the time goes (VM reductions, the relay, consoled); then a shell-track node if it is the print path's own. Harness wave (Q5): the listener's take rate matters again when session code reacts to lease ends.
