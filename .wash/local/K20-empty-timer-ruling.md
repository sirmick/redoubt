# K20: who pays for a timer interrupt that finds nothing (ruling on QA `K20-empty-timer-entry`)

This adds to `K20-rerule.md` and replaces the page lines of `K20-timer-entry-billing.md` given
below. Evidence: `.worktrees/k20/.k20/question-empty-timer.md`.

## The cause, from the code

`message.rs`'s `mark` lowers the account's `earliest_timeout` and calls `time::note_timeout`
before the call is decided. `settle` then decides:
- a deadline that has already passed returns `Timeout` and never blocks;
- an answer already there (a queued message) never blocks either.

Either way the hardware stays armed for a wait that does not exist. Under `icount`, the checked
build's entry costs about 75 µs, so a 1 µs sleep has always passed by `settle`. Each of the
sleepers' sleeps then arms one interrupt that finds nothing: the attacker's ~2,700. The time.rs
doc's "a cancelled wait ... costs at most one early interrupt" is true per wait, but a process
makes waits at will.

## The ruling: (c) and (b), not (a)

**(a) is refused.** A wait that ends early leaves the timer armed for its old deadline. That
deadline can be anywhere in the future, so its owner chooses when the interrupt lands, and on
whom: in the victim's slice, at will. That is the neighbour paying, against timer.md's "not a
neighbour's".

**(c) The timer is armed only for a thread that blocks.** `settle`'s block branch notes the
timeout: the account's `earliest_timeout` and `note_timeout`. `mark` no longer does. The hint is
still never late, because a thread that is not blocked has no timeout to miss.
- The implementer checks that every way to block goes through `settle`'s block branch. There
  are four callers today, and a call taken by its server stays blocked from that branch.
- This removes nearly all of the flood's empty interrupts: none is made, so none is billed.

**(b) A wait that ends before its timeout pays for the interrupt it leaves.** No new state is
needed:
- The stale wait's account is the one whose `earliest_timeout` is at or before now, yet holds no
  due thread. `next_timeout` already walks that account and recomputes it.
- `next_timeout` reports the budget of the last such account it found, beside the due item.
- The walk that found it, and the rest of an entry that expired nothing, are billed to that
  budget, as an expired item's are.
- No owner is kept beside time.rs's global hint.

**What stays nobody's:**
- A budget destroyed before its deadline leaves the `budgets` hint early: one walk of the
  deadline list, at most one per destruction. The destroyer pays for the whole destruction,
  which costs far more, so this is a stated residual and not billed.
- A slice end moved by an audit is the audit's, and stays nobody's.

## Page lines (they replace the brief's for these two places)

scheduling.md "Charging". Replace the bullet "an expired timeout is billed to its thread's
budget, ... so one entry does at most one walk nobody pays for;" with:
> - an expired timeout is billed to its thread's budget, and a deadline's destruction as below,
>   each with the walk that found it. The timer is armed for a timeout only when its thread
>   blocks; a wait that ends before its timeout leaves it early, and the walk that finds the wait
>   gone is billed to that thread's budget. The rest of an entry that found either, its last walk
>   and the timer's own handling included, is billed to the budget it found last: the budget it
>   interrupted pays for none of it. A timer interrupt that found neither is the running budget's
>   when it ends that budget's slice, and nobody's otherwise;

timer.md "R12 (scheduling) for timer work". Replace "So is the walk that found the item. A budget
with many timeouts due at once pays one walk for each. The one walk per entry that finds nothing
more is the kernel's." with:
> So is the walk that found the item, and a budget with many timeouts due at once pays one walk for
> each. The timer is armed for a timeout only when its thread blocks, so a call whose timeout has
> passed, or that is answered at once, arms nothing. A wait that ends before its timeout leaves the
> timer early; the walk that finds it gone is billed to the waiting thread's budget. The rest of the
> entry, its last walk and the timer's own handling, goes to the budget whose item or wait it found
> last, so the budget the timer interrupted pays for none of it.

timer.md "Residual risks". Replace the bullet "**Expiry walks threads.** ... the last walk of each
entry is paid by nobody." from "Each walk" to the end with:
> Each walk that finds an item, or a wait that ended early, is billed to its budget. A budget
> destroyed before its deadline leaves the timer early: one walk of the deadline list, nobody's,
> for each such destruction, which its destroyer pays for in full.

The module docs of time.rs ("Hints") and message.rs (`mark`, `settle`) move with the code.

## Cases

- `sched-timer-flood` gets a third case, "cancelled waits". The attacker's threads block on
  `receive` with timeouts staggered a few hundred µs ahead, and a sibling answers each at once,
  so stale wakeups land in the victim's slice.
  - All three cases pass the 450 floor on both widths, with the shares reported.
  - Under (a), this case would charge the victim. Say so in the report, without building it.
- The direct check is extended. An entry that found a stale wait is billed to that wait's budget.
  The oracle reports the count of empty entries that ended no slice, per case.
- The recorded negative (the old billing) still fails the direct check.
- The other green cases are those in `K20-rerule.md`.

## Paths

These lines are in `message.rs`: `mark`, `settle`'s block branch and `next_timeout`'s return.
K16 also edits that file. K20 is started, so its owned paths are the orchestrator's to extend,
with K16's nod. Whoever merges second rebases. The size stays S, in Tier A.
