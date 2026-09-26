# A size budget for the trusted crates

## What

A bench case that fails when a trusted crate grows past its ceiling in lines of Rust, the way
[the unsafe budget](../testbench.md#the-unsafe-budget) fails on an undocumented or extra `unsafe`.
The ceilings start at each crate's size when the case lands and only fall; raising one needs a
reason in the commit that does it, the same rule as the ratchet.

## Why it matters

Tenet 1 says the size of the trusted computing base is budgeted, not observed, and the kernel is
meant to be read in full. Nothing enforces that today: the only pressure is a reviewer's opinion.
The one deletion package so far took the kernel from about 18,000 lines to 10,400 and the
`unsafe` count from 54 to 44, which shows how much a stated target moves. A ceiling in the bench
makes every later package pay for growth in the open.

## Where

`tools/testbench` (the `unsafe-budget` case is the model: `tests/unsafe-budget.toml` and the
checker it runs); the crates it covers: `kernel`, `loader`, `stub`, `libs/sys`, `libs/layout`,
`libs/paging`, `libs/signing`, `libs/rt`, `libs/wire`, `model`, the drivers and the servers.

## Done when

- A `size-budget` case lists each trusted crate with its ceiling in lines of non-comment Rust,
  counts the crate the same way every time, and fails on any crate over its ceiling.
- Ceilings can only fall: the case also fails when a ceiling in the list is higher than the last
  committed one, unless the commit message of the change states the reason (the case reads it
  from `git log -1`, as the unsafe ratchet does for its reason).
- The case runs in the full bench, and [how Redoubt is built](../SWARM.md#simplification) cites
  it as the size gate.
