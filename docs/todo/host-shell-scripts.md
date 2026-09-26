# Host shell scripts

## What

The build host runs shell scripts: `build`, `test`, `launch`, `mkimage` and `dev.sh` at the repo
root, `scripts/build-bios.sh`, `scripts/pi-ensure.sh`, `scripts/ssh-key-ensure.sh`,
`tools/vendor-check/provenance.sh`, `libs/wire/elixir/run-vectors`, and the Elixir runtime's tools
under `userland/otp/tools/` (`env.sh`, `build-beamlet`, `build-lib`, `difftest`,
`elixir-tests`).

## Why it matters

[Tenet 3](../TENETS.md#3-rust-and-assembly-only-where-rust-cannot-reach) says no C and no shell
script runs on the machine, and names these scripts as its residual. None of them runs on the
machine, but each is code a reader of the build must audit in a second language. Whether the tenet
reaches the build host is the owner's to decide.

Belongs to no follow-up package until the owner rules.

## Where

- [`build`](../../build), [`test`](../../test), [`launch`](../../launch),
  [`mkimage`](../../mkimage), [`dev.sh`](../../dev.sh)
- [`scripts/`](../../scripts)
- [`tools/vendor-check/provenance.sh`](../../tools/vendor-check/provenance.sh)
- [`libs/wire/elixir/run-vectors`](../../libs/wire/elixir/run-vectors)
- [`userland/otp/tools/`](../../userland/otp/tools)

## Done when

- The owner has ruled on the build host. If the tenet reaches it, each script is a Rust tool (or
  a `cargo` alias) and this page is deleted; if not, tenet 3 states the build-host exception in
  place of its residual, and this page is deleted.
