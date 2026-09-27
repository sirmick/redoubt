# The steward decision-wake target is set from a seed sweep

## What

`bench:sched-latency` holds a stand-in for the steward to a 50 ms decision-wake p99 under load.
In some rv64 runs at N = 16 the stand-in wakes with a lead of a few milliseconds of its own
runtime, lands behind several weight-100 spinners' slices, and its p99 passes 50 ms, up to about
100 ms. The difference between runs comes from the guest's boot RNG seed: QEMU fills it from host
entropy on every boot, the kernel draws PIDs from it, and the PIDs shift the instruction-count
phase. It is a real miss under this workload, not a measurement fault.

The rule is settled ([responsiveness](../kernel/scheduling.md#responsiveness)): targets are guest
instructions under `icount` (1 ms is 125,000 at `shift=3`); the gate runs one pinned seed; a sweep
of about 16 seeds measures the spread, and the decision-wake target is its worst case plus a
stated margin, expected about 100 ms; a lease's end stays the sum of the decision-wake target and
the R10 (destruction) target, each stated. The bench and the case do not do this yet.

## Why it matters

Human control rests on a measured lease-termination latency (R12 (scheduling) promises a prompt
wake, not a bounded one). A target the bench misses by chance is neither a guarantee nor a
reliable gate, and a flaky case hides real regressions. A pinned seed alone would hide the miss;
the sweep is what makes the relaxed target honest.

## Where

- [`tools/testbench`](../../tools/testbench): the case's QEMU arguments, where the seed is pinned,
  printed and overridable.
- [`tests/sched-latency.toml`](../../tests/sched-latency.toml) and its program in
  [`tests/programs`](../../tests/programs): the targets and the lease-end sum.
- The pages: [scheduling](../kernel/scheduling.md#responsiveness) and
  [the test bench](../testbench.md#the-case-file).

## Done when

- The guest seed is pinned for `bench:sched-latency`, printed with the result, and can be
  overridden to replay a run.
- The sweep has been run on rv64 and rv32, and its seeds, per-seed results and worst case are
  recorded on [scheduling](../kernel/scheduling.md#responsiveness).
- The targets are set from the sweep, each stated in instructions and in milliseconds with its
  margin, and the case asserts the decision-wake target and the lease-end sum.
- `bench:sched-latency` passes on rv64 and rv32 in repeated runs.
