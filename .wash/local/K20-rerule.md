# K20 re-ruled: a correctness fix of the billing, proved directly

Ruling on QA `K16-timer-flood-share`, after K20's checkpoint (`.worktrees/k20/.k20/checkpoint.md`).
This replaces the evidence and the cases in `K20-timer-entry-billing.md`. That brief's page lines
and its package, apart from the changes below, stand.

## The ruling: option (1)

**K20 goes ahead.** The rule is the pages', and the code contradicts it, whichever way the share
moves.
- scheduling.md "Charging" leaves an interrupt with no device object to nobody. The timer has no
  device object.
- timer.md "R12 (scheduling) for timer work" ends: a timer flooder "spends its own CPU share, not
  a neighbour's".
- `irq.rs` bills the rest of a timer entry to `cur`, the budget it interrupted: `begin_billing`
  after expiry, then `on_interrupt` and `rearm`.

**Option (2), restating the page to match the code, is refused.** It would weaken R12's "not a
neighbour's". That is a guarantee, so it would be the owner's to give, and I do not recommend
it.

**The share is not the instrument.** The victim's share is spin work over wall time under
`icount`. It mixes how often the sleepers can wake with who pays for each wakeup. The
checkpoint shows that it does not track the entry's length the way the old brief said. Struck
from the brief:
- "This is why padding `reconcile` moved the victim's share, and why a faster entry still lands
  below the floor";
- the "about 10" recorded negative;
- "more margin than today's 3-6".

The report records that the share did not reproduce: on base ca5a6437b it was 445/446 (K16), and
on 31dfad3f0 it is 469/472 rv64 and 468/469 rv32. It also records that padding raises the share.

## A second leak the fix must close

`sched::leave` takes `now` and calls `close_billing` first. It then sets `user_since = now`
before `runnable()`, `reconcile` and `time::rearm` run. So everything `leave` does after it
starts counts as user time of the budget that runs next. After a timer entry, that is the
victim, because a wake never preempts. "Billed from the end of expiry to the return" in the
brief means to the actual return:
- the tail's payer is billed through `leave`'s walk, `reconcile` and `rearm`;
- user time starts at the return.

`leave`'s billing order is K20's. The bodies of `runnable` and `reconcile` stay K16's.

## The proof: direct, with a recorded negative

These cases replace the padding self-check. Drop `sched-pad` and its case. A check that passes
both with and without the defect guards nothing.
- **A direct check in the trace** (`sched-trace` and `sched_oracle`, in test builds only). For
  every timer-interrupt entry from user mode, the trace shows:
  - the interrupted budget;
  - the budgets billed;
  - the ticks billed to each.

  The oracle asserts that an entry that expired something charges its time, from the trap to
  the return, to the expired items' budgets and to nobody else. One exception: the interrupted
  budget pays its own items. An entry that expired nothing is charged to the interrupted budget
  only when it ends that budget's slice. Run it in `sched-timer-flood`, both cases, both
  widths.
- **A recorded negative run.** A test-only feature that keeps the old billing (a name like
  `timer-tail-billed`, off in every default build, as `audit-billed` is) must fail the direct
  check on both widths. List it with the other diagnostic features in scheduling.md's R23 lines,
  "The other diagnostic features are off by default in the same way".
- `sched-timer-flood` still passes its 450 floor, and Responsiveness's two "against sleepers and
  staggered budget deadlines" figures are re-measured. No bar is set above the floor.
- These stay green: `deadline-flood-billed`, `sched-latency` at seed 3, `sched-exit-churn`,
  `sched-server-busy`, `sched-destroy-billing`.
- No model mutation is added, because the model charges runtime only. The report says so.

## K16

K16 no longer needs K20. Its need came from a floor miss that does not reproduce on main. K16
rebases onto main and reruns `sched-timer-flood` with commit 4, both widths, seeds 3 to 5. If it
holds the floor, K16 goes on without K20. Both packages edit `sched::leave`, so whichever merges
second rebases onto the other: K16 owns the walks, and K20 owns the billing order.
