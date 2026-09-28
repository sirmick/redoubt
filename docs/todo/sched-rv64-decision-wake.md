# The steward's decision wake on rv64

## What

`bench:sched-latency`'s sweep (seeds 1 to 16, recorded on
[scheduling](../kernel/scheduling.md#responsiveness)) shows the steward stand-in's decision wake at
N = 16 is structurally slower on rv64 than on rv32 and than its own timer wakes:

- rv64: p50 17,900 to 17,904 µs on every seed; p99 61 to 104 ms (worst seed 3).
- rv32: p50 about 7,600 µs on every seed; p99 18 to 40 ms.
- The same steward's timer wakes: p50 about 9.9 ms (rv64) and 10.5 ms (rv32).

The same median on every seed is not noise: something in the rv64 path puts the waking steward
behind about one more spinner's slice. The targets were relaxed from the sweep (15 to 20 ms p50,
50 to 115 ms p99) so the gate passes, and so the case no longer shows it.

R10 (destruction)'s kernel time is close to its own target too: p99 26.4 ms (rv64) and 27.4 ms (rv32) on every
seed, against 30 ms.

## Why it matters

Human control rests on how fast a lease ends after the steward decides, which is this wake plus
R10's time. A target set from today's behaviour stops catching a regression in it, and a margin
of a few milliseconds on R10 fails the gate at the next object the destruction walks.

## Where

- [`tests/sched-latency.toml`](../../tests/sched-latency.toml) and
  [`tests/programs/src/bin/sched-latency.rs`](../../tests/programs/src/bin/sched-latency.rs);
  `TESTBENCH_QEMU_SEED` replays any seed of the sweep.
- The scheduler: [`kernel/src/sched.rs`](../../kernel/src/sched.rs) and the wake path it ranks.
- The page: [scheduling](../kernel/scheduling.md#responsiveness).

## Done when

- The cause of the rv64 decision-wake median is found, and either fixed with the targets
  tightened back toward 15 and 50 ms, or explained on the scheduling page as a stated residual.
- R10's margin is widened or its target restated with a reason.
