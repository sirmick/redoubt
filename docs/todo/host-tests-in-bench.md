# Host tests the bench does not run

## What

Several crates' host tests run only under `cargo test`, never in a bench case: `redoubt-rt` (the
serving library, the startup block, the runtime), `redoubt-sys`, `littlefs`, `stub` and
`redoubt-keyd`. Three oracles run outside every bench case as well: `keyd`'s timing tests
(`servers/keyd/tests/timing.rs`, ignored unless built optimised), littlefs's differential tests
against the C reference (`libs/littlefs/diff/`, its own workspace), and the generated Elixir codec
against the Rust one (`libs/wire/elixir/run-vectors`).

## Why it matters

The server pages mark sections built on these tests. A change that breaks one passes every bench
run, so "built · tested" can go stale without anything failing where the project looks.

Fixed in the servers follow-up package after the documentation rewrite.

## Where

- [`tests/`](../../tests): the bench cases; `kind = "host-tests"` cases name the packages they
  run (`r4-host-tests`, `wire-host-tests`, `netd-host-tests` and others).
- The pages: [serving](../servers/serving.md#residual-risks), [init](../servers/init.md),
  [keyd](../servers/keyd.md), [fsd](../servers/fsd.md), [wire](../servers/wire.md#residual-risks).

## Done when

A host-tests bench case runs each of those packages, and `./test` over it passes; the three
oracles run in a bench case, or each page states why one cannot.
