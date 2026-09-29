# A host test build of the workspace fails

## What

`cargo test --workspace` does not compile on the build host. One crate stops it:
`test-programs` (the bench's programs, `tests/programs`), whose library calls
`redoubt_sys::syscall`, which exists only for RISC-V, so the host build fails with 36 errors.
Nothing in it is meant to run on the host.

The kernel no longer adds to this: its binary says it has no host tests (`test = false`).

## Why it matters

A workspace test build that fails for everyone is noise: it hides a real failure in the same run,
and nothing in the bench runs it, so it is not seen. The bench runs host tests only through
`host-tests` cases, which name their packages ([the test bench](../testbench.md#cases)).

## Where

- [`tests/programs/Cargo.toml`](../../tests/programs/Cargo.toml): the crate's targets and their
  test settings.

## Done when

- `cargo test --workspace` compiles on the host: `test-programs` declares that it has no host
  tests (`test = false` on its targets).
