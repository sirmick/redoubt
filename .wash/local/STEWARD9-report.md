# STEWARD9 report (steward9-implementer) — INTERIM, paused pending STEWARD10

Branch `wp-STEWARD9`, WIP `5e0cce692` on origin/main `41090437b` (train 73, STEWARD8 in).
Worktree `/home/mcloonan/redoubt/.worktrees/STEWARD9`. Node re-tiered from B to **A** (the fix
needed a probe constant and a grouping change in `servers/steward/src/bin/steward.rs`). Reviewer
steward-red-5. This is interim: the deadline-notice rows are unjudged until **STEWARD10** (a
steward serving-path node STEWARD9's cluster exposed) lands; I rebase on it and run the table.

## What the node does

STEWARD8 found the latency case's kernel-deadline lease (5 s, twice an older 2.5 s launch) now
dies inside its own launch (launch 6.64 s on rv32 at two harts), so the rv32 N = 3/4 rows that
complete cannot be judged. The ruling: derive the deadline lease and its sleep from the same
launch measurement STEWARD8 took for `launch_timeout`/`MAX_CALL_MS`.

## Delivered (WIP 5e0cce692, by path)

1. `servers/steward/src/bin/steward.rs` (probe, `latency-probe` feature only): a named constant
   `LATENCY_DEADLINE_US = 14_000_000` beside `LATENCY_LEASE_US`, the derivation in its doc; and
   `others` (a sample's group) counts a sample net of the deadline class — `lease` itself plus
   every lease NOT of the deadline length, so the round's clustered deadline leases, dying
   together, each give a sample grouped at N (not scattered over N+15 … N). `leases_live` (the
   window-close check) unchanged.
2. `tests/steward-latency.toml`: the D deadline leases launched back to back as a cluster and
   left to their deadlines together, one 16 s wait after the cluster (was: each 14 s, 16 s each).
   The derivation and the ring arithmetic in the toml comment and the description.
3. `docs/kernel/scheduling.md` "With the real steward": the parameter sentence — the deadline
   lease 14 s (2 × 6.64 s = 13.28 s, rounded up; the launch bound's own number), the cluster, the
   grouping by class, the one 16 s wait, and why a wait-per-lease (144 s/round) overran the trace
   ring.

Derivation (all from STEWARD8's one launch measurement, nothing chosen): worst launch over both
widths at one and two harts = 6.64 s (rv32, two harts, beside fourteen spinners). Deadline lease
= 2 × 6.64 = 13.28 → **14 s**, so a deadline falls on a running VM (≥ 14 − 6.64 = 7.36 s of VM
before it) and never inside its own launch (B82 / `budgets.md` "Deadlines"). Post-cluster wait =
lease + 2 s = **16 s** (the 2 s holds the 40 ms notice target and the 300-500 ms of checked-build
audits after a VM's destruction, the margin the old 7 s gave the 5 s lease).

## Why a cluster, not a wait per lease

A 16 s wait per deadline lease adds 16 × (16 − 7) = 144 s of guest time to each round (72 s at
N = 16). The kernel's `sched-trace-large` ring holds 8,388,608 records ≈ **1,395 s** of this
case's guest time on rv64 at either hart count (4-8 k records/s in every round, idle or spinning:
the VMs' schedulers and the timer). Sequentially the five-round run reaches its window's end at
**1,747 s** (one hart) and **1,902 s** (two), past the ring: the dump reports `dropped > 0` and
the oracle, which needs `SCHED-TRACE-END … dropped 0`, judges nothing. Measured:
`.tmp/STEWARD9/rv64-smp1/` (`dropped 1536145`), `rv64-smp2/` (`dropped 3194478`). The Architect
ruled the cluster (option (c)): the deadlines pass concurrently (the more realistic load, moves
no target), each notice still per lease, the round waits once — the deadline phase shrinks to
about one lease's wait and the five-round run fits the ring.

## Finding (found by STEWARD9, resolved as STEWARD10) — serving path, not the case

The cluster's concurrent deadline kills recycle the steward's handle indices, and the steward's
`DestroyBudget` cleanup for a deadline-killed lease (`drive.rs:520`) holds a raw handle index
across the recycle. `drive.rs` tolerates `kernel.destroy` → `BadHandle` when the lease is gone
and `!running` (its comment names exactly the deadline-killed case), but under the cluster the
recycled index resolved to a *different, non-budget* object, so `budget_handle`
(`kernel/handle.rs:282`) returned **`WrongObject`**, which the handler does not tolerate →
`steward: a step failed (WrongObject): DestroyBudget {… kind: Lease …}`, a forbidden line, both
rv64 widths. Sequentially each index freed before the next launch, so only `BadHandle` ever
appeared; the cluster's overlap is what surfaces it.

The Architect verified the kernel and ruled the loud `WrongObject` the lesser form: the **silent**
form is catastrophic — a *different budget* at the recycled index (the common case, since launches
are leases) makes `budget_handle` answer `Ok` and the steward destroy the **wrong live budget**
with no error. STEWARD10 (Tier A, steward-red-5): the cleanup keys on the budget's identity and is
cancelled under STEWARD5's queue lock when the lease is deadline-killed, before any handle can take
the freed index — covering `BadHandle`, `WrongObject` and the different-budget case by construction;
a host test where the recycled index's new budget survives. STEWARD9's reshape depends on it.

### Wrong-object check this run (the orchestrator's ask) — clean

Id-level accounting over the cluster run `rv64-smp2.log` (reached N = 2 before the halt): 55
distinct `AgentStarted` lease ids, 49 distinct `LeaseEnded` ids; **no `LeaseEnded` id was ever
un-started**, none started or ended twice. The 6 started-but-not-ended are the leases in flight
at the halt. So no wrong budget was destroyed THIS run — only because the recycled index landed
on a non-budget (the safe `WrongObject`); that is luck, not safety, which is why STEWARD10 closes
it by construction. Detail: `.tmp/STEWARD9/cluster-wrongobject-finding.md`.

### Display artifact (cosmetic, folded into STEWARD10)

The steward printed `lease alice/agent-19 … was killed 0` twice under concurrent kills (smp=2
lines 287/290) against two distinct lease ids: its id→name lookup returned one name for two
kills. The oracle judges on ids, not names, and the `deadline_notice` count stayed a distinct 16,
so no judged quantity depends on it. The Architect folded a one-line fix (the line carries the id,
or the lookup keys on it) into STEWARD10's brief, same area; I confirm no judged quantity depends
on it.

## Grouping verified (the part that does not need the full table)

rv64 smp=2 reached `steward: LATENCY-COUNT N=1 deadline_notice 16` — all sixteen clustered
deadline notices grouped at N = 1 as intended, each a per-lease sample. The constant and the
grouping are sound; only the serving-path halt stops the run.

## Gates — pending the runs

Not yet run (they need the rebased tree on STEWARD10): `set CASES=formatting`, `set CASES=docs`,
size-budget, unsafe-budget, the steward set both widths, `steward-host-tests`, and
`steward-latency` both widths at one and two harts. The prebuilt built clean with the committed
changes (`prebuilt rc=0`), and the cluster boots (grouping correct) up to the serving-path halt.
Run commands and the oracle-by-hand invocation: `.tmp/STEWARD9/notes.md`.

## Documentation check

- `docs/kernel/scheduling.md` "With the real steward": updated (the parameter sentence, the
  cluster, the ring). The filled rows wait for the runs.
- `docs/kernel/budgets.md` "Deadlines": cited, unchanged (it already states a lease shorter than
  its launch never runs).
- README.md, GETTING-STARTED.md, SECURITY.md, docs/servers/steward.md, docs/userland/shell.md,
  docs/servers/README.md: no claim about the deadline-lease length or the case's grouping;
  unchanged. No new R<n>; no dates or package ids on the pages or in the case.

## Open

- STEWARD9 lands on STEWARD10; then the table's rv32 two-hart N = 3/4 (and N = 16 where reached)
  rows are filled from one run each at seed 3.
- The WIP is one commit; it folds into clean commits before acceptance.

Head: `5e0cce692` (`wp-STEWARD9`), clean tree, on origin/main `41090437b`.
