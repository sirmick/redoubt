# The kernel crate has no host test target that compiles

## What

`cargo test -p redoubt-kernel` does not compile on the build host: the kernel crate is a
`no_std` binary for the target, and a host test build of it fails with 17 errors. The crate holds
no `#[test]` of its own; its rules are tested on the target by the bench, and its pure parts
through `redoubt-stride` and the executable model. Nothing says that a host test build is not
supported, so `cargo test` over the workspace stops at the kernel.

## Why it matters

A host test build that fails for everyone is noise: it hides a real failure in the same run, and
it invites someone to "fix" the kernel crate for a target it was never meant to build for. Either
the kernel has host tests, for logic that needs no hardware, or its crate says plainly that it
has none.

Fixed in the kernel follow-up package after the documentation rewrite, before the work on `init`
and the manifest.

## Where

- [`kernel/Cargo.toml`](../../kernel/Cargo.toml): the binary target and its test settings.
- The page: [the model](../kernel/model.md#residual-risks) (host tests do not reach the kernel's
  boundaries).

## Done when

- `cargo test` over the workspace passes on the host: the kernel crate either builds its host
  tests or declares that it has none (`test = false` on its target), and the bench says which.
