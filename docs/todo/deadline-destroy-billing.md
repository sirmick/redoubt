# Part of a deadline's destruction is billed to nobody

## What

When a budget's deadline passes, the timer path destroys it (R10 (destruction)). The work up to
the lift (killing the processes, destroying endpoints, failing messages in flight) is billed to
the dying top budget, and moves up to its parent with its debt. The rest, `lift_dying` and
`destroy_marked` (closing handles everywhere, freeing frames), is billed to no budget. A top
whose free weight is 0 is not billed even for the first part, since a weight-0 charge adds
nothing to a pass.

This departs from R12 (scheduling)'s inheritance rule: the top returns its carve to its parent
before any of the destruction's work is charged, so the parent pays for all of it at the weight
it has once the child is gone. `budget_destroy` meets the rule, because its caller pays for the
whole call as system-call time. The deadline path does not.

The rule text for R10 and R12: "Every destruction's whole cost is billed to someone. For
`budget_destroy` that is the caller. For a deadline it is the top's parent, after its carve
returns, or the nearest ancestor with free weight above 0 if the parent has none. `root` always
does. No part of a destruction is billed to nobody."

## Why it matters

A creator can make many empty weight-0 budgets with short deadlines, one `budget_create` each,
and have the machine spend time destroying them that no budget pays for. That time comes out of
every other budget's share. The 64 staggered deadlines of `bench:sched-timer-flood` leave the
victim its half; larger floods are not attacked.

Fixed in the kernel follow-up package after the documentation rewrite, before the work on
`init` and the manifest.

## Where

- [`kernel/src/budget.rs`](../../kernel/src/budget.rs): `destroy_subtree` (the `bill` step, then
  `lift_dying` and `destroy_marked`).
- [`kernel/src/time.rs`](../../kernel/src/time.rs): the deadline branch of expiry, which calls
  `destroy_subtree` with billing on.
- [`libs/stride/src/lib.rs`](../../libs/stride/src/lib.rs): `charge`, which adds nothing at
  weight 0.
- The pages: [budgets](../kernel/budgets.md#r10-destruction) and
  [scheduling](../kernel/scheduling.md#residual-risks).

## Done when

- The whole `destroy_subtree` interval, `destroy_marked` included, is billed after `lift_dying`
  to the top's parent, or the nearest ancestor with free weight above 0.
- A bench case, `deadline-flood-billed` (or an extension of `sched-timer-flood`), has a creator
  make N empty weight-0 deadline budgets, and shows the creator's own pass rising with N while a
  victim keeps its share.
- A planted mutation that drops the bill after the lift fails that case.
- R10 and R12 on their pages carry the rule text above.
