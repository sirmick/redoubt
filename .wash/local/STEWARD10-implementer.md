# STEWARD10: a deadline-killed lease's DestroyBudget cleanup keys on identity

Node STEWARD10 (parent AGENTS). Tier A (the steward's serving path; a wrong-object destruction).
Size S–M. Reviewer: steward-red-5. Ruling: `.wash/local/wave-2026-10-18-rulings.md` "STEWARD10";
evidence `.worktrees/STEWARD9/.tmp/STEWARD9/cluster-wrongobject-finding.md`, logs
`.tmp/STEWARD9/rv64-run/`.

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting`, docs gate `set CASES=docs`. No edit while prebuilt
  builds or cases run.
- Messages at most 2000 bytes; detail in a file. Read source before ruling on it.
- Pages carry no dates or ids; a security property lands with an attack case whose verdict comes
  from the system, never the attacker. The core (`libs/steward`) moves only with its oracle; a
  model generator change draws no extra random values. STEWARD5's cleanup-queue machinery
  (`cleanup.rs`, the thread, the Mutex) is on main; this builds on it.

## The bug (verified in the kernel)

`servers/steward/src/drive.rs:520` `Step::DestroyBudget { budget }`: `let b = self.budget(budget)?;
... kernel.destroy(b)`. A lease's kernel deadline (R10) destroys its budget and **revokes the
steward's handle**, freeing its handle-table index. `kernel/src/handle.rs:294` `install_handle`
reuses the **lowest free index** for a newer object. If a newer object takes the freed index
before the queued DestroyBudget cleanup runs, the stale index resolves to it:

- a **non-budget** at the index → `budget_handle` (handle.rs:287) returns `WrongObject`, which
  drive.rs:526 does not tolerate → `a step failed (WrongObject)`, a forbidden line (the loud form
  the test hit);
- a **different budget** at the index (a new lease — the common case, since launches are leases)
  → `budget_handle` returns `Ok(its frame)` and the steward **destroys the wrong live budget with
  no error**. `budget_at`'s I1 assert (`b.id == r.id`, handle.rs:152) guards only that the handle
  *at the slot* is self-consistent, not that the steward's index still means the lease it
  intended — so the silent catastrophic form is not caught.

drive.rs:526's `Err(BadHandle) if !running => {}` was correct only while sequential deadlines
freed each index before the next launch, so only `BadHandle` (the index still free) ever appeared.

This is a real production scenario: several agents' leases expiring close together cause
concurrent R10s and the same handle recycle. It is not a test artifact; do not treat it as one.

## The rule (the deadline is the identity witness; no kernel change)

The ruled cancel-under-lock does **not** map onto the code and is superseded: DestroyBudget is
not a STEWARD5 queue step (that holds connections) — it runs **inline** in `drive.rs` `step()`
(the `Exited` event, an `end_lease` batch, and `forget()`); and the steward cannot read the
kernel's `BudgetRef` id (`Machine::budget_id` returns the index). The budget's **minted deadline**
is the identity witness instead, using two facts: `time.rs` runs expiry at every kernel entry
first, so a budget with deadline `d` is destroyed at the first entry **at or after `d`, never
before**; and only the loop installs handles in the steward's table.

`Made::Budget` carries its deadline `d`. A destroy by index reads a fresh `kernel.now()` = `t`
**immediately before** the destroy, with no install between (the loop runs this inline):

- **`t >= d`:** the kernel took it at `d` (and the index may be recycled) → drop the record,
  **no destroy by index**. This is the whole safety guard.
- **`t < d`, `Ok`:** the index is still the original (it cannot have been expiry-killed before
  `d`) → the right budget, destroyed.
- **`t < d`, `BadHandle`:** the budget is gone → **benign, drop the record** (nothing was
  destroyed; safety does not depend on why it is gone). Only raise a diagnostic if you first
  confirm every legitimate before-deadline end (`end_lease`, blame-lockout) destroys **and
  removes the record inline**, so a lingering record at `t < d` is a true anomaly; if any
  legitimate path can leave a record past its budget, keep this branch benign, not a failure.
- **`WrongObject`:** a failure. With no inline install between the `now()` read and the destroy,
  it should be unreachable — a genuine invariant tripwire.

The `BadHandle if !running` tolerance at drive.rs:526 goes. The **same rule in `forget()`**: the
crossing's budget (deadline `CROSSING_LIFE`) is destroyed there with the error ignored, so the
silent form is reachable there too. This also covers an `end_lease` served after `d` but before
the exit notice, which a cancel-at-the-notice could not.

Only the `t >= d` guard matters for safety: the sole unsafe outcome is `Ok` on a recycled live
budget, and the guard never destroys by index at or after `d`; `BadHandle` and `WrongObject`
destroy nothing whatever `t`.

## Tier A invariants the reviewer checks

- No DestroyBudget ever destroys a budget other than the lease it was queued for: a recycled
  index is cancelled/skipped, never destroyed.
- A deadline-killed lease's budget is destroyed exactly once (by R10); the steward's cleanup for
  it is cancelled, not re-run on a stale index.
- An end-initiated DestroyBudget (live budget) is unchanged.
- The cancel is race-free against index reuse: the single-arbiter loop cancels before it installs
  the handle that could take the freed index (or the queue lock serialises them).

## Cases

- Host (`servers/steward/tests/steward.rs`, the Recorder/fake kernel): a lease's handle index is
  recycled for a **new budget** before its DestroyBudget cleanup runs; the new budget **survives**
  (the verdict is the new budget's liveness in the fake kernel, never the steward's output); and
  the recycled-by-a-non-budget case yields no `WrongObject` step failure. A deadline-killed lease
  runs no DestroyBudget; an end-initiated one still does.
- Bench: `steward-latency` with STEWARD9's concurrent-deadline shape (coordinate: STEWARD9 lands
  on this), both widths, no forbidden `a step failed` line and every round judged;
  `steward-lease-ends`, `steward-end-lease`, `steward-sub-agent-dies-with-agent`,
  `steward-server-restart` unchanged in verdict.

## A display artifact in the same area (fold in, not its own node)

STEWARD9's run printed `agent-N was killed 0` twice for two distinct lease ids: under concurrent
kills the id-to-name lookup (test-only probe/log path) returned the same name for two ids. It is
cosmetic — the oracle judges on ids, not the printed name — so confirm no judged quantity depends
on it, then fix it one-line in the same area (the line carries the lease id, or the lookup keys on
the id), since you are already here. Not its own node.

## Pages

steward.md "Failure and restart" (the cleanup bullet: a deadline-killed lease's budget is R10's,
its cleanup cancelled, so no stale index is destroyed); SECURITY.md R10/R41 rows if their tests
change. Summaries checked: say which.

## Owned paths

`servers/steward/src/drive.rs` (the DestroyBudget step and the exit-notice handling),
`src/cleanup.rs` (the cancel under the lock), `bin/steward.rs` (the wiring), its tests; the pages
above. Not `libs/steward`, not the kernel (unless the alternative is ruled, which it is not yet).

## Gates

Host tests; release builds rv64 and rv32 with and without `latency-probe`; the steward set both
widths; `steward-latency` by name (with STEWARD9); `set CASES=formatting`, `set CASES=docs`,
`size-budget`, `unsafe-budget`. Report each with its exit code and the host test's verdict (the
new budget's survival).

## Checkpoint

After the host test proving a recycled index does not destroy the new budget is green, one
progress line: the cancel's shape and why the ordering is race-free.
