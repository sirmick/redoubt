# The test programs' build does not watch every input of the stub

## What

The test programs embed the loader stub, which their build script builds with a nested `cargo
build -p stub`. The script tells Cargo to rebuild when the stub's sources, `stub/Cargo.toml`, and
the sources of `libs/sys` and `libs/wire` change. It does not watch `libs/sys/Cargo.toml`,
`libs/wire/Cargo.toml` or the workspace's `Cargo.lock`, so a change to a dependency's version or
features there does not rebuild the stub the test programs carry. (Checked against the current
script: the source directories were added; the manifests and the lockfile were not.)

## Why it matters

A bench run could then test a stub built from different dependencies than the tree names, and
pass or fail for a reason nobody can see in the diff
([the loader stub](../servers/init.md#launching-through-the-loader-stub)).

Belongs to no follow-up package: test build tooling.

## Where

- [`tests/programs/build.rs`](../../tests/programs/build.rs): the `rerun-if-changed` list.

## Done when

- The script also watches `libs/sys/Cargo.toml`, `libs/wire/Cargo.toml` and `Cargo.lock`, and
  changing any of them rebuilds the embedded stub.
