# K20: a timer entry is billed to the budget whose timeout it expired (ruling and brief)

**Re-ruled in part: read `K20-rerule.md` first.** Its evidence, its cases (no padding
self-check; a direct trace check and a recorded negative instead) and its `leave` requirement
replace this brief's. The page lines here stand.

Ruling on QA `K16-timer-flood-share` (the Architect's). Evidence: K16's
`.worktrees/k16/.k16/report-b341.md`.

## The ruling

**By the pages as written, a timer wakeup's kernel time is the flooder's, never the interrupted
budget's.**
- timer.md "R12 (scheduling) for timer work" bills each expired timeout, and the walk that found
  it, to the sleeper's budget. It ends: "a process that arms many timers a microsecond apart ...
  spends its own CPU share, not a neighbour's".
- scheduling.md "Charging" leaves exactly two things to nobody: one empty walk per entry, and an
  interrupt with no device object.
- No page bills the interrupted budget for another budget's timer.

**The code does bill it.** In `arch/riscv/irq.rs`, a trap from user mode runs
`expire_at_entry()`, then `sched::begin_billing()`. From there, kernel time is `cur`'s: the
interrupted budget, which is the victim here. On a `SupervisorTimerInterrupt` entry that is the
rest of the entry: `time::on_interrupt`, `rearm`, and `leave`'s `runnable()` walk and
`reconcile`.
- So every 1 µs sleeper's wakeup charges the victim's pass with the tail of the entry it caused.
- The nobody's parts (the empty walk, the trap) come out of the victim's wall time on top of
  that.
- This is why padding `reconcile` moved the victim's share, and why a faster entry still lands
  below the floor.

It is a page-code disagreement, and so a kernel finding. **The case's floor stands.** Neither (b)
nor (c) is the answer: K16's commit 4 waits for K20, then rebases.

## The rule, tightened so the property holds whatever an entry costs

The package writes these page lines in the commit that makes them true.

scheduling.md "Charging". Replace the bullet "an expired timeout is billed to its thread's
budget, ... so one entry does at most one walk nobody pays for;" with:
> - an expired timeout is billed to its thread's budget, and a deadline's destruction as below,
>   each with the walk that found it. The rest of an entry that expired something, its last walk
>   and the timer's own handling included, is billed to the budget whose item it expired last:
>   the budget it interrupted pays for none of it. A timer interrupt that expired nothing is the
>   running budget's when it ends that budget's slice, and nobody's otherwise;

timer.md "R12 (scheduling) for timer work". Replace "So is the walk that found the item. A budget
with many timeouts due at once pays one walk for each. The one walk per entry that finds nothing
more is the kernel's." with:
> So is the walk that found the item, and a budget with many timeouts due at once pays one walk for
> each. The rest of the entry, its last walk and the timer's own handling, goes to the budget whose
> item it expired last, so the budget the timer interrupted pays for none of it.

A system call's entry is unchanged. Its own time is its caller's. Expiry inside it is billed as
above, and the tail after expiry is the call's, so it stays the caller's.

## The package

Tier A (the trap boundary's billing), size S. It has no dependencies and goes before K16's
commit 4.
- `time::expire_due` returns the last budget it billed, if any.
- `irq.rs`, on a timer-interrupt entry from user mode:
  - **Something was expired:** billing from the end of expiry to the return goes to that budget,
    not `cur` (`sched::bill`, as `bill_irq` does for a device owner). The empty last walk is
    billed with it.
  - **Nothing was expired:** billed to `cur` only if `slice_over()`, and to nobody otherwise.
  - A syscall entry keeps `begin_billing`, with the expiry's empty walk billed to the last
    expired item's budget if there was one.
- `sched.rs`'s module docs, the two pages above, and scheduling.md's `sched-timer-flood` figures
  re-measured (net and gross, both widths).

Cases, both widths:
- **`sched-timer-flood` passes on main** with more margin than today's 3-6. Report the shares.
- **A self-check** shows that the share no longer tracks the entry's length. It runs the case
  with `reconcile` padded by a test-only feature (`sched-pad`, about 800 nops, off in every
  default build). The victim's share moves by at most 3 per thousand. Without the fix it moved by
  about 10, which is the recorded negative.
- These cases must stay green: `deadline-flood-billed`, `sched-latency` targets at seed 3,
  `sched-exit-churn`, `sched-server-busy` (interrupt billing to the device owner, unchanged).

The model charges runtime only ("billing other kernel work is the kernel's alone"). So add no
mutation, and say so in the report.

Owned: `kernel/src/time.rs` (`expire_due`'s return), `kernel/src/arch/riscv/irq.rs` (the timer
arm and the billing after expiry), `kernel/src/sched.rs` (the billing functions only; not
`runnable` or `reconcile`, which are K16's), the two pages, and the case and self-check tomls and
program.
