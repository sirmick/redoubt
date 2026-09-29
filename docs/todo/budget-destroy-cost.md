# Budget destruction's cost is near its latency target

## What

Destroying a budget (R10 (destruction)) scans every kernel-object page, several times, with
interrupts off and the kernel not preemptible. Destroying a budget that holds two processes
takes about 21 ms of virtual time. The cost grows with the kernel-object pages in the whole
system, not with the size of the budget destroyed, and any budget can add to those pages by
creating objects.

It also grows with every live process's handle pages. When a destruction frees a process object,
taking a lease's exit notice included, the object's handle sweep walks every process's handle
table, so that nothing names the freed frame (I1 (handles name live objects)). At N = 16 in `bench:sched-latency`, with the
sessions still alive, that sweep took 6.21 ms with 19 processes holding handle pages, against
1.11 ms with 2. A change that made destructions cheaper moved the workload onto that overlap, and
R10's p99 rose from 26.4 to 35.4 ms. So `bench:sched-latency` now holds R10's p99 to 39 ms and the
deadline notice's to 54 ms, set from a seed sweep ([scheduling](../kernel/scheduling.md#responsiveness)),
not the 30 ms they had.

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
- [`kernel/src/handle.rs`](../../kernel/src/handle.rs): `sweep_handles`, which `free_object`
  (`process.rs`) runs over every process's handle table.
- [`tests/sched-latency.toml`](../../tests/sched-latency.toml): the pinned target.
- The pages: [budgets](../kernel/budgets.md#residual-risks) and
  [scheduling](../kernel/scheduling.md#residual-risks).

## Done when

- A destruction's cost follows the objects the dying subtree holds, not every object page in
  the system, or is bounded by a constant well inside the target.
- `bench:sched-latency` asserts the destruction time with a clear margin on rv64 and rv32, and
  its targets are back to R10's p99 <= 30 ms and the deadline notice's p99 <= 40 ms.
- A bench case fills the system with other budgets' objects and shows the destruction time does
  not grow with them.
