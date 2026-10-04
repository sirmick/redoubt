# K16 commit 5: the walks at 512 PIDs (Architect, ahead of attribution)

The gate fails at 512 PIDs: bystander share 752/723 against 783, `budget_destroy` p50
63-65 ms, and decision_wake p50 42.6 ms. Below is what the brief already allows, and what I
will rule once the two rv64 attributions come back.

## Already allowed by ruling 2: no new ruling needed

Ruling 2 says "every walk visits only processes that exist … the cost then follows live
threads". A walk over all 512 slots that skips the empty ones keeps the letter of that rule but
not its point. So:

- **A live-PID set.** Keep a 512-bit live-PID mask (`[u64; 8]`), set at `process_create` and
  cleared when the process slot is freed. Every per-PID walk iterates its set bits
  (`trailing_zeros`): `pids()`, `Runnable::fill`, `handle.rs`'s per-PID sweeps,
  `process_ended`'s loop, and the R2 sender pick. The cost is 8 word reads plus the live
  processes. The commit-1 "64-bit live mask for now" grows to this.
- **The stride queue, dense.** `Queue`'s `slots: [Option<B>; N]` becomes a dense array with a
  count: insert appends and removal swap-removes.
  - Every scan (the floor, the pick, reconcile's membership tests) then visits only queued
    budgets.
  - The pick is a minimum over `(pass, tie, id)`, which is a total order, so slot order never
    mattered. Check that `queued()`'s "slot order" has no other reader that depends on it.
  - The differential and the stride host tests must pass unchanged.
- **A checked build audits** the live set against a slot scan at each process create and free,
  as the PID index is audited today (R12's index rule).

## If `Runnable::fill` on every entry still dominates after the live set

- Make reconcile follow what changed in the entry, not every live process.
  - A budget is marked when one of its threads becomes runnable or stops being runnable.
  - Reconcile visits only the marked budgets.
- The rules do not change: one reconcile per kernel entry, with the same wakes and leaves in the
  same order (descending id within the entry). So the model and the oracle see the same events.
- Sort the marked set by id (at most the threads touched in one entry) to keep the
  descending-id order.
- This is in K16's scope, as one more commit before the values, under ruling 2's "cost follows
  live threads". If it is large, report its size first.

## Was 512 right?

- **512 stays if the walks follow live objects.** Then an unused PID slot costs a bit, not a
  loop pass, and only a system that runs 512 processes pays for 512, in pages it was charged.
- **The owner decides if it still fails.** If the gate still fails after the live set, the
  dense queue and (if needed) the marked reconcile, then a walk that cannot follow live objects
  remains. In that case:
  - name the walk and its cost per slot;
  - I put the choice to the owner as a decision_request: a lower `MAX_PROCESS_COUNT` (128 or
    256), or the walk's redesign as its own package.
- **Not a free option:** lowering the value to pass the gate without that question. 512 is the
  owner's value.

## Attribution I want

- The per-entry count of PID-slot walks on the gate's path, by function.
- The per-pick queue scan length.
- `budget_destroy`'s time split into the per-PID sweeps and the rest.
