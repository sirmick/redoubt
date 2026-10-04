# K16-churn-ceiling: ruling (architect-8)

## The ceiling is not a rule

R12 is a floor: "while it has a runnable thread, a budget gets at least its weight's share". The
carving rules over-charge the carver on purpose and say so: "A budget that runs while most of its
weight is carved away accrues its lead at the small weight it kept ... That only over-charges the
budget that carved; it never under-charges" (scheduling.md, "Running while carved down"), and "the
budget ends a little higher: over-charged, never ahead" ("The lead follows the weight"). The
program's own doc comment says the shell's create/destroy calls are charged at its halved weight.

So the shell variant's victim gets half **plus** whatever the shell runs while carved, at double
price. How much that is follows the kernel's speed: commit 1 makes walks and audits cheaper, and
that moves where the shell's slices end relative to its carves. 501 on main and 576 at commit 1
are both what the rules give. A ceiling on the victim measures the kernel's speed, which is why
four packages have met it from four sides.

The property the ceiling was put there for, "creating and destroying a child that never ran moves
nothing" (the entry-wait double count), is exact arithmetic, and it is judged exactly where it is
exact: the oracle recomputes every lift and every weight change from the trace and every pick in
rank order (`124 lifts by the rule`, `372 weight changes by the rule`), and
`mutation:R12LiftCountsEntryWait` and `host:redoubt-stride::a_carve_and_its_return_leave_the_state`
hold it in the model and the crate. Netting audits stays guarded by the gate's `audit-billed`
recorded negative run.

## What K16 does

1. `tests/programs/src/bin/sched-budget-churn.rs`: the shell variant's bounds become those of the
   other variants, `(500 - TOL, 1000)`. Remove the `variant == 4` branch. Rewrite the comment
   above it and the doc comment's "is neither starved nor favoured" to say the victim keeps at
   least half, the oracle's recomputed lifts show creating and destroying moved nothing, and the
   shell pays at its halved weight for what it runs while carved, its calls included.
2. **One evidence run, not a feature.** In a local, uncommitted edit, make the kernel's lift count
   the entry wait (the old bug) and run `sched-budget-churn` on rv64. The oracle's lift check must
   fail. Report the line. If it passes, stop and tell me: then the ceiling's guard is not
   replaced, and I rule on a recorded negative feature instead.
3. Pages, in K16's commit:
   - **scheduling.md**, "Inheritance", replace "Its last variant, a shell that keeps giving a
     child half its weight and taking it back with no run between, leaves the victim neither more
     nor less than half." with:
     > Its last variant, a shell that keeps giving a child half its weight and taking it back with
     > no run between, leaves the victim at least half, and the recomputed lifts show that creating
     > and destroying moved nothing. Its victim's share has no ceiling: the shell pays at its
     > halved weight for what it runs while carved, its own calls included
     > ([running while carved down](#running-while-carved-down)), so how far above half the victim
     > gets follows the kernel's speed.
   - **scheduling.md**, the share paragraph in "Responsiveness": "and the shell's victim 500 and
     499" takes K16's measured numbers on both widths; delete the next sentence ("With the audits
     billed to the budget that ran them, as before, ... more than half.").
   - **testbench.md** needs nothing.

Not an owner choice: R12's text is a floor, and the carving sections state the over-charge.
