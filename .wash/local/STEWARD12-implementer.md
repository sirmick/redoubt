# STEWARD12: a burst of expiring leases does not delay the last sponsor notice

Node STEWARD12 (parent AGENTS). Tier A (the steward's loop; a responsiveness target under a
burst). Size S–M. Reviewer: steward-red (the orchestrator's pick). Ruling:
`.wash/local/wave-2026-10-18-rulings.md` "STEWARD12"; evidence
`.tmp/STEWARD9/cluster-deadline-notice-finding.md`.

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting`, docs gate `set CASES=docs`. No edit while prebuilt
  builds or cases run. A long `steward-latency` run goes detached (`setsid nohup`); copy job logs
  aside before a rerun.
- Messages at most 2000 bytes; detail in a file. No target moves; a bound is measured on both
  widths. The core (`libs/steward`) is not touched unless the oracle moves with it.

## The finding

STEWARD9's clustered rounds: when N lease deadlines fire together, the steward dequeues the N
`Exited` events serially at ~70 ms each (the per-exit cleanup, the `LeaseEnded` audit, the
console lines), so the last `lease_ended` notice reaches the sponsor ~N × 70 ms after its
deadline — net p99 1.1–1.2 s at a 16-lease burst against the `deadline_notice` target 40 ms. The
per-notice target holds (a single expiry is 10 ms; the sequential N=16 round meets it). It is the
burst that misses, because the notice is sent **after** each exit's bookkeeping.

The sponsor being told promptly that a lease ended is a human-control promise
([agents.md "Leases"](../../docs/userland/agents.md); the sponsor can always end a lease and is
told when one ends), so a burst of expirations must not push the notice behind the bookkeeping.

## The rule

On a burst of `Exited` events the steward sends the `lease_ended` notices **first** — the cheap
send to each sponsor's `notices` endpoint — then does, per exit, the cleanup (already a thread's,
STEWARD5), the `LeaseEnded` audit and the console lines (already queued, STEWARD3U). So the last
notice's latency is ~N × (one send) rather than ~N × 70 ms. Read the loop's current order of
"notice vs per-exit work" first (it may already do some of this since STEWARD5/3U moved cleanup
and lines off the critical path; the ~70 ms is what remains on the loop); the change is to hoist
the notice send ahead of the remaining per-exit work for a batch of exits.

## Tier A invariants the reviewer checks

- A burst of N expirations sends all N notices before the per-exit bookkeeping of the batch, so
  no notice waits behind another exit's audit/cleanup/lines.
- No notice is lost or duplicated by the reordering; the audit and console order per exit is
  preserved (STEWARD3U's queue order holds), only the notice is hoisted.
- A single expiry is unchanged (still ~10 ms); the `deadline_notice` target is met per-notice and
  now under a burst.
- No target moves; the cleanup (STEWARD5) and console (STEWARD3U) rules are unchanged.

## Cases

- Bench: measure the burst's last-notice p99 before and after, on `steward-latency`'s clustered
  expiry (a clustered-expiry variant, or a small new case `steward-deadline-burst` that starts N
  leases with one shared deadline and times the last `lease_ended`), both widths; state the
  before/after and the per-send cost. The `deadline_notice` target is then met under the burst,
  not only per-notice. If notices-first alone does not reach 40 ms at the image's realistic burst
  size, **state the residual** (the burst size at which it holds) rather than moving the target —
  and raise it to the Architect if the residual is large.
- The steward set (`steward-lease-ends`, `steward-end-lease`, `steward-sub-agent-dies-with-agent`,
  `steward-blame-lockout`, `steward-server-restart`) both widths, unchanged in verdict.
- Host: a burst of exits sends the notices before the per-exit work (the fake kernel's order of
  sends vs audits), and a single exit is unchanged.

## Pages

- steward.md "Failure and restart" / "Residual risks": the notices-first rule and, if a residual
  remains, the burst size at which the target holds.
- scheduling.md "With the real steward": the burst finding and its fix (the per-notice target
  held, the burst's last-notice latency before/after).
- SECURITY.md R39/R40 rows if their tests change. Summaries checked: say which.

## Owned paths

`servers/steward/src/bin/steward.rs` (the Exited-event handling / notice order), `src/drive.rs`
if the notice send is there, its tests; `tests/steward-latency.toml` or a new
`tests/steward-deadline-burst.toml`; the pages above. Coordinate with STEWARD9 if it still owns
the latency toml (send the orchestrator the lines rather than editing it under STEWARD9). Not
`libs/steward`, not the kernel.

## Gates

Host tests; release builds rv64 and rv32 with and without `latency-probe`; the cases above both
widths; `set CASES=formatting`, `set CASES=docs`, `size-budget`, `unsafe-budget`. Report each with
its exit code and the burst last-notice p99 before/after with log paths.

## Checkpoint

After the before/after burst measurement, one progress line: the per-send cost, the last-notice
p99 after, and whether 40 ms holds at the realistic burst size or a residual remains.
