# Budget destruction's cost is near its latency target

## What

Destroying a budget (R10 (destruction)) scans every kernel-object page, several times, with
interrupts off and the kernel not preemptible. Destroying a budget that holds two processes
takes about 21 ms of virtual time; its p99 is within a few milliseconds of the 30 ms target that
`bench:sched-latency` holds it to. The cost grows with the kernel-object pages in the whole
system, not with the size of the budget destroyed, and any budget can add to those pages by
creating objects.

## Why it matters

Every interrupt, wake and timeout on the machine waits for a destruction. It dominates
driver-wake and lease-end latency whenever a lease ends, and human control (ending an agent's
lease) rests on that latency. It is the one stated exception to R12 (scheduling)'s bound on a
call's kernel time. It must be brought well inside the target before the steward, which ends
leases, is built, in M1 (separation and containment).

## Where

- [`kernel/src/budget.rs`](../../kernel/src/budget.rs): `destroy_subtree`, `destroy_marked` and
  `lift_dying`, and their scans of `0..=high_frame`.
- [`kernel/src/message.rs`](../../kernel/src/message.rs) and
  [`kernel/src/process.rs`](../../kernel/src/process.rs): `budgets_dying`.
- [`tests/sched-latency.toml`](../../tests/sched-latency.toml): the pinned target.
- The pages: [budgets](../kernel/budgets.md#residual-risks) and
  [scheduling](../kernel/scheduling.md#residual-risks).

## Done when

- A destruction's cost follows the objects the dying subtree holds, not every object page in
  the system, or is bounded by a constant well inside the target.
- `bench:sched-latency` asserts the destruction time with a clear margin on rv64 and rv32.
- A bench case fills the system with other budgets' objects and shows the destruction time does
  not grow with them.
