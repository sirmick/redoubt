ást# Architect handoff (architect-5 → architect-6, 2026-10-19 checkpoint, ~70% context)

## Branch state / traps (read first)
- I hold no branch; I edit only `.wash/local/` files and ship plan changes as `plan-*.toml` for the orchestrator to apply (never plan_set a started node). Nothing is lost if this process ends.
- TRAPS: (1) messages/QA ≤2000 bytes, plan node body ≤2000 bytes, title ≤200 — I hit the body cap twice this wave, trim into the brief. (2) An ephemeral implementer's key may be inactive when you reply (READ1's first key errored) — route via `orchestrator` on the thread if `message_send` says "not an active workspace member". (3) `plan.toml` is rewritten by agentd; `.wash/local` is the durable medium. (4) Every ruling's detail goes in a file the thread names; I keep the wave's rulings in `.wash/local/wave-2026-10-18-rulings.md` (append sections, don't rewrite). (5) Don't design to pass an unsupported config (rv32 at 16 agent VMs is beyond M6); state the row, name the node.

## The one open thread: STEWARD8-bound (and READ1)
Almost all this wave's QA runs on thread **STEWARD8-bound** (the orchestrator + the steward/latency implementers) and **READ1** (the read-path implementer). What members may still ask:
- STEWARD12 (steward12-implementer): proceeding on (1) — revert notices-first, land the burst cases recording deadline_notice unbounded + residual. May ask about the residual's wording or the SHELLN1 handoff of `steward-deadline-burst`.
- READ1 (read1-implementer-2, key 0292169…): fix in WIP (162a573cd+, caps 57/71/71/47, (b') exit, client retry, gate `lease-code-load.toml`); host tests green, boot cases running. May bring the gate results or the measured supported-N back on thread READ1 for a yes.
- STEWARD9/10/11/12 in review/flight (steward-red-5); SCHED5, BENCH1, SHELLN1, READ1 live.

## Rulings made this wave, all in wave-2026-10-18-rulings.md unless noted
- F1→STEWARD5 (cleanup off the loop; Mutex+parked-call reply; await_cleanup refused; (b) fail-now). F3→STEWARD6 (SERVER_US=4s bound). F2→STEWARD7 (lease term derived: min(pages,processes,weight,home_quota); in_flight joint-share bound; R4a bench assertion; six-principals walfsd 10→9). F4→STEWARD8 (launch measured; launch_timeout/MAX_CALL_MS one derived number, option (a); async start_agent deferred). STEWARD9 (sequential-all, two boots→three cases N=1-3/n4/n16-rv64-only; marker-end probe; per-width smp=BENCH1). STEWARD10 (stale-handle wrong-destruction: deadline-as-identity-witness, t>=d kernel-owned; NOT swallow; steward-red-5). STEWARD11 (same guard on the Scope arm). STEWARD12 (burst = sponsor's take rate, not bookkeeping; (1) record+residual, revert notices-first).
- Findings→nodes: READ1 (VM death=Files-cap admission; full ruling + cap sign-off + 2b=(b') in `.wash/local/READ1-ruling.md`); SCHED5 (rv32 2-hart N=2 timer_wake, kernel lock-contention); BENCH1 (per-width smp); SHELLN1 (shell notices listener take-before-print). ATK10-13 (agent-suite attack cases) earlier. Kernel finding (first burst notice ~N×R10) stated as residual, node only if it matters.
- STEWARD3V-budgets thread: all Q1-Q8 ruled (confirmed orchestrator provisionals; lease derivation).

## Pending drafts you owe
- **SHELLN1**: plan `plan-2026-10-19-shell-notices.toml` + brief `SHELLN1-implementer.md` — DONE, ready for the orchestrator to id/launch (shell track, Tier B).
- **SCHED5's brief**: after its measurement (why N=2/one-spinner; fix or stated residual).
- **STEWARD13's brief**: after STEWARD10 and STEWARD12 land — design the launch off the loop vs STEWARD10's "no install off the loop" premise (steward-red-5's note 1); plan already in `plan-2026-10-19-steward13.toml`.
- **READ1 phase 2**: watch thread READ1 for the gate/supported-N; give the yes.

## Owner questions: `.wash/local/owner-questions-2026-10-18.md`, five, still unanswered (orchestrator flashes)
Q1 steward weight (rec 1,000), Q2 launching-lease share (rec no change), Q3 struck (moot), Q4 sockets per session (rec alice net 32), Q5 gatewayd/agentsuite-2nd-wave (rec keep harness/loop/escape-room at M5, or gatewayd loopback-first if pulled in). Q1/Q2 carry the read-path-weight data point (a shared server at weight 100 starves under 16 agent VMs on one hart).

## Ids taken: SCHED1, BENCH1 used (SCHED1 was the done slice-length pkg — timer_wake is SCHED5). Live: STEWARD5-13, READ1, SCHED5, BENCH1, SHELLN1, ATK10-13, IPD1/SSH3/BEAM5 (merged). The orchestrator ids SHELLN1/READ1/SCHED5/BENCH1's series.

## Process notes
- The agent-suite step: first wave ATK10-13 (merged/merging); its 2nd wave (harness, pi-loop, escape room) is M5, gated on gatewayd (owner Q5). After READ1/the steward-latency chain and ATK13, agent-suite is near done bar the gateway.
- Memory rules applied: order on events not the host clock (a derived guest-time wait under icount is fine); no target moves for a measurement; a miss is a finding with a number; size for the need not margin; a deliberate rule (the park, R75/R76) outranks a convenience fix.
