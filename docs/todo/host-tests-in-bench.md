# Host tests the bench does not run

## What

Several crates' host tests run only under `cargo test`, never in a bench case: `redoubt-rt` (the
serving library, the startup block, the runtime), `redoubt-sys`, `littlefs`, `stub` and
`redoubt-keyd`. The wire crates were in the same state and now run in `wire-host-tests`.

## Why it matters

The server pages mark sections built on these tests. A change that breaks one passes every bench
run, so "built · tested" can go stale without anything failing where the project looks.

## Where

- [`tests/`](../../tests): the bench cases; `kind = "host-tests"` cases name the packages they
  run (`r4-host-tests`, `wire-host-tests`, `netd-host-tests` and others).
- The pages: [serving](../servers/serving.md#residual-risks), [init](../servers/init.md),
  [keyd](../servers/keyd.md), [fsd](../servers/fsd.md).

## Done when

A host-tests bench case runs each of those packages, and `./test` over it passes.
