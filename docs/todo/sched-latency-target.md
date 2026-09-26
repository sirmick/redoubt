# The steward decision-wake target is missed on rv64

## What

`bench:sched-latency` holds a stand-in for the steward to a 50 ms decision-wake p99 under load.
In some rv64 runs at N = 16 the stand-in lands behind several weight-100 spinners' slices and
its p99 passes the target, up to about 100 ms. The difference between runs comes from the
guest's boot RNG seed, which shifts PID allocation and so the instruction-count phase. It is a
real miss under this workload, not a measurement fault.

## Why it matters

Human control rests on a measured lease-termination latency (R12 (scheduling) promises a prompt
wake, not a bounded one). A target the bench misses by chance is neither a guarantee nor a
reliable gate, and a flaky case hides real regressions. The target must not be weakened by
asserting 80 ms on the decision wake alone: the settled budget is a 50 ms wake plus 30 ms of
R10 (destruction) time.

## Where

- [`tests/sched-latency.toml`](../../tests/sched-latency.toml) and its program in
  [`tests/programs`](../../tests/programs).
- [`kernel/src/sched.rs`](../../kernel/src/sched.rs): the stride queue and the preemption
  points.
- The page: [scheduling](../kernel/scheduling.md#residual-risks).

## Done when

The owner has chosen one of: re-pin the target with evidence; assert the true sum (decision wake
plus R10 time, 80 ms) as one measurement; or change the steward stand-in. The guest's seed is
pinned so that runs repeat, and `bench:sched-latency` passes on rv64 and rv32 in repeated runs.
