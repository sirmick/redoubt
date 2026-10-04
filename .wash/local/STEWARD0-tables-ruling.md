# STEWARD0 tables: the Architect's ruling (architect-4)

Reviewed: wp-steward0 21f195c96, `libs/steward/tables/*.md`, against steward.md "The policy core".
The rules below are written on `docs/servers/steward.md` on main (see the QA answer for the
commit); rebase onto it before commit 5. The tables are sound in shape; the changes below are
what commit 1 needs before the generator.

## The choices

- **C1, accepted with a fix to the model.** `owns_labels` reads the manifest's owned labels (the
  fixed part, outside every domain), never a domain's existence. Keep the Login rows' order; the
  dispatch's domain lookup refuses with the same answer. The model's manifest gives a principal a
  one-label set it works under but does not own, so `PolicyVaultWithoutOwnership` reaches the
  guard and a family catches it.
- **C2, rule guards get mutations.** On the page's table now:
  - `approval_key`: `PolicyApproveWithLoginKey` (accepts a login key, or one `keyd` holds); P3.
  - `caller_unlabelled`: `PolicyLabelledStartsAgent`; P8.
  - `item_fits`: `PolicyDeclassifyUnfit` (accepts over `DECLASSIFY_MAX` or unprintable); P6.
    Declassification only: a pushed item is not capped (see C6).
  - `exact_labels`: `PolicyDeclassifyFromUnlabelled` (accepts a declassification from a session
    without exactly the item's labels, or a push from a labelled one); P6 or P11.
  - Missing from your list: `notify` carries R37/R38 (the notice reaches only channels whose labels
    include all the request's): `PolicyNotifyLabelledToAll`. The model's embedder records each
    notice as something its session observes, so P10's replay catches it.
  - Kind guards (`snapshots`, `reading`, and the ones C5 adds) carry no rule and have no mutation;
    the README says so.
- **C3, accepted.** `carve_lease` carries R39's sub-agent half: `PolicySubAgentOutlivesAgent`.
- **C4, accepted.** `PolicyWriteUp` is the volume's check (R25) and stays in the model's embedder.
  `exact_labels` gets its own mutation (C2).
- **C5, accepted, with the kinds shown.** The copy out is a crossing with no budget: the same
  pattern as `Granted` to a lease. What an approval starts belongs to the machine it starts.
  But no effect may branch on kind behind one name:
  - the crossing's `Open` group has one row per kind, with guards in this order:
    - a read: `carve_crossing`, `cross_item`, `destroy_crossing`;
    - the copy out: `copy_out` only;
    - the push's write (last row): carve, cross, destroy.
  - `Done`/`Failed` rows likewise.
  - Submit's snapshot has two rows: a declassification opens a read crossing; a push reads its
    unlabelled source as its own batch.
  - `copy_out` writes the request's snapshot and never re-reads (`PolicyDeclassifyLive`).
    `carve_crossing` carries `PolicyDeclassifyWithoutReader`. The page's old `one_item` is gone.
- **C6, accepted** (the page says it). A push request lives in the submitting unlabelled
  session's domain and counts against that domain's cap and the session's fair share. It names the
  target label set, and `owns_labels` covers the target.
  - Audit: a request an unlabelled session submits for a labelled target (a labelled agent, a
    push) is recorded under the target's domain from submission on. Reason: whether it was
    approved depends on that domain's lockout (C9).
  - Other requests are recorded under the request's domain.
  - A push's screen shows the source, the target, the size and the hash.
- **C7, accepted.** One channel id in the request; the last `Pending` that rendered it wins.
- **C8, accepted.** `Deny` like `Approve`: only from `Rendered`, on that channel. Families render
  before they deny.
- **C9, accepted.** The refusal answers only the approval channel, never the requester.
  `not_locked` holds trivially for a request that starts no lease.
- **C10, accepted.** Every audit read goes through `audit_visible` via the `Policy` table:
  `inspect` now, M4's read event later. No row in M1.
- **C11, changed.** Every batch's outcome lands on a waiting state. The crossing gets `Closing`:
  - a crossing that fails goes `Failed` → `Closing` (`destroy_partial`);
  - from `Closing`, `Done` or `Failed` → `Closed`;
  - the read's `pass_failure` stays on the first row.
- **C12, accepted.**

## The notation (N1-N7): accepted, with

- N5: `unreachable` is a steward bug. The server exits (fail closed; `init` restarts it). The
  model fails the run.
- Internal events (`Granted`, `Open`, the crossing's result for its request, `LockedOut`,
  `SessionEnded`): `decide` runs them FIFO before it returns. They are never a batch step and
  never fail one. One naming a removed object is dropped silently. State this in the README.

## Found in review

- **The lease-supervision edge's "learns that it ended" had no row.** Every lease row leaving
  `Running` adds `notify_sponsor`: a notice to the sponsor's unlabelled sessions naming the lease,
  not why it ended. It goes through the edge.
- **A granted lease's start failure** (`Starting`: `Failed`, or `Done` while locked) has no caller
  for `refuse`. Audit it in the lease's domain instead.
- The page said "Three" retired mutations and listed four; fixed.
