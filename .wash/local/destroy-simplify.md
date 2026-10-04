# The destroy path, with a simplifier's hat on (Architect, for the owner)

Read from main at 31dfad3f0: budgets.md R10, ipc.md R4/R4a/R4b, invariants.md I15, and the code:
- `budget.rs` `destroy_subtree`;
- `message.rs` `budgets_dying`, `pump`, `end_thread`, `abandon`, `fail_all`;
- `process.rs` `died`, `settle_notice`, `budgets_dying`, `endpoints_dying`.

This proposes no page edits.

## Summary

The destroy path is not as simple as its properties allow. R10's nine steps are each needed by a
rule, and its billing rule costs about ten lines. What makes the seam fragile is two structural
facts:

1. **Destruction runs IPC's live path while the world is half dead.** Step 2 kills each process
   through the ordinary path. `end_process` calls `kill_process`, which calls `process_ending`,
   and that pumps every endpoint the victim's threads were receiving on. `settle_notice` then
   pumps the exit endpoint.

   So deliveries happen in the middle of a destruction: after the mark, before step 4 has failed
   the messages tied to dying budgets. Every IPC decision must therefore ask whether something is
   dying. There are now five such checks, each added after a finding:
   - `receiver_on`'s doomed check (K17);
   - `abandon`'s no-notice-on-a-dying-endpoint check;
   - `allowed`/`settle_notice` on a dying owner or creator;
   - `drop_dying_notices`;
   - the ordering comment in `budgets_dying` ("receivers and senders on a dying endpoint go
     first, so a receiver is never offered...").

   This is one mechanism doing two jobs. The pump is both "deliver what is pending" and "carry on
   after a teardown".
2. **An endpoint has no state of its own.** Its queue, its receivers and its owed notices are
   spread over every thread's slot and every process object. So every question about an endpoint
   is a walk over all threads with a predicate: `find_thread`, `next_sender`, the restamp,
   `fail_all`. A destruction asks those questions many times.

One consequence of fact 1 that I read from the code and have not yet confirmed by a trace:
- A kill's pump at step 2 can deliver to a live receiver a send whose stamp is a dying budget. Its
  words arrive, and step 6 sweeps the handles it carried.
- Had no kill pumped that endpoint, step 4 would have failed the same send with `Dead`.
- The outcome then depends on the order of the kills. R10 says only "a message *still queued*",
  so no rule is broken, but it is exactly the class of order-dependent outcome the last two weeks
  kept finding. The first commit of the package below should confirm or refute it with a model
  trace.

## (1) One mechanism doing two jobs

Yes: the pump (fact 1). The reverse direction, IPC walking budgets, is legitimate. R2 groups by
the sender's budget, and R1 and stamps read budgets. Those are one read each, not a second job.

## (2) "Mark doomed, then reap with nothing to decide"

Yes, inside one destruction. Not across time.

**Recommended, a package after K16: "pumps run at the boundary".**
- **A. Mark.** As today: weight back, budgets dying, doomed defined (R10 step 3).
- **B. Sever.** Everything tied to the subtree is withdrawn or failed using primitives that never
  pump:
  - doomed threads end with `end_thread`;
  - every wait on a dying endpoint, and every send or taken call with a dying stamp, gets `Dead`;
  - calls are abandoned, with a notice only if the endpoint survives;
  - exit notices are recorded, or dropped as today.

  Each live endpoint that could now deliver (it lost a server, or gained a notice) goes on a
  bounded "to pump" list, at most one entry per thread ended. This is one pass. Nothing it does
  creates new work for the pass, so the "repeat until no pass fails anyone" loop goes. A checked
  build asserts that a second pass finds nothing.
- **C. Reap.** Lift, sweep, return, move, free (steps 5-9), with nothing to decide.
- **D. Pump.** Each listed endpoint is pumped once, at the end. No doomed thread and no dying
  object exists any more, so the pump needs no dying or doomed check at all.

The same rule serves every kernel operation, not only destruction: an operation changes state and
collects endpoints, and its end pumps each one once. That is K13's "an ending process pumps each
endpoint once", made general. `reply`'s `poke_receivers`, `process_ending` and `settle_notice` all
become "add to the list".

What it removes:
- the five dying and doomed checks in IPC;
- the ordering subtleties in `budgets_dying`;
- the rescan loop.

R4b's "a doomed thread takes nothing" becomes structural rather than checked. Its mutation then
cannot be written, and is retired with the reason given, as the steward's were. R10 step 4
tightens to "from the mark on, no message stamped with a dying budget is delivered", the same
"from the mark" doctrine that defined doomed.

## (3) Would different accounting remove complexity?

No.
- R10's billing is small: the payer is named after step 1, billed after step 9, and step 1
  returns the weight first.
- The complexity of K9, K12 and K16 is in bounding the walks under the 30 ms target. That is a
  cost discipline, not a consequence of who pays.
- Charging the destroyer up front needs a cost estimate before the walk. That is more machinery,
  and it is exact only by accident.
- Deferred, incremental reaping would remove the walk's latency. But it makes "dying" visible
  across kernel entries, so IPC would check it everywhere and forever: the opposite of simpler.
  The budget figure's "dying only inside one destruction, without preemption" is a simplifying
  property to keep.

## (4) What K16's walks-by-live-thread does and leaves

- **It does:** every walk's cost follows live threads instead of 512 x 255 slots. That is the cost
  half of fact 2, and enough to keep the R10 target at the new sizes.
- **It leaves:**
  - the shape: destruction still pumps mid-kill, and IPC still checks dying (fact 1);
  - `fail_all`'s rescan from the start after each failure, which is quadratic in the failures;
  - the repeated passes in `budgets_dying`.

  The boundary package removes the last two as a by-product.
- **Endpoint-owned queues** (intrusive lists in the thread pages, per R2 group) would remove the
  rest of fact 2. That is a rewrite of `message.rs`'s core and the model's IPC, and its gain is
  cost, not correctness. Decide on it only if K16's worst-walk numbers (its deliverable 8) say
  the walks are too slow.

## (5) What each option gives up

| Option | Given up | Verdict |
| --- | --- | --- |
| Pumps at the boundary (A-D) | Nothing in the rules or the targets. R10 step 4 tightens. One mutation is retired as unwritable. The destruction does the same work, so it stays inside R10's 30 ms. | Recommend |
| Endpoint-owned queues | Nothing in the rules. A large rewrite with risk to R2's ordering and the model. | Only on K16's numbers |
| Deferred reaping | "Dying only inside one destruction", the immediacy of I2, and simple IPC | Reject |
| Destroyer charged up front | Exact billing ("billed to someone, exactly") | Reject: no simplification |

## Recommendation

One package, Tier A, size M, after K16 merges (it rewrites the same walks):
1. A model trace that confirms or refutes the mid-kill delivery above.
2. Pumps at the boundary: the to-pump list, and `process_ending`, `settle_notice` and
   `poke_receivers` adding to it.
3. The destruction as mark, sever, reap, pump. The five checks go, and so does the rescan loop,
   with the checked-build assertion in its place.
4. Pages:
   - R10 step 4 says "from the mark on";
   - R4b's doomed sentence becomes a consequence of the order, not a check;
   - I15's "kept in" list shrinks;
   - the mutation is retired with its reason.

   The gate's R10 numbers are re-measured at full fill on both widths.

If the owner agrees, I will write the node and brief, and the page text moves in the package's
commits.
