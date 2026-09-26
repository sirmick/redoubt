# The vendored crates' unsafe code has never run under Miri

## What

`ipd` links smoltcp and its dependencies (`heapless`, `managed`, `byteorder`, `hash32`,
`stable_deref_trait`), vendored under `vendor/` and pinned by checksum. They hold about 500 uses
of `unsafe`, outside the unsafe budget because they are third-party code. Nobody has run them
under Miri, the interpreter that detects undefined behaviour, in the configuration `ipd` uses.

## Why it matters

The vendored crates are part of a system server that parses packets from the network. Leaving
their `unsafe` out of the ratchet is reasonable only if something else checks it; today only
`ipd`'s own fuzz targets reach it, and fuzzing finds crashes, not every undefined behaviour
([ipd](../servers/ipd.md#residual-risks)).

Fixed in the kernel follow-up package after the documentation rewrite, with the bench and
tool work it carries.

## Where

- [`vendor/`](../../vendor/README.md): the vendored crates and their provenance.
- [`servers/ipd/`](../../servers/ipd/src/lib.rs): the configuration it builds them in.

## Done when

- `ipd`'s host tests, and the vendored crates' own tests for the features `ipd` enables, run
  clean under `cargo miri`, and a bench case or a recorded run says so; or the reason a crate
  cannot run under Miri is stated on the `ipd` page.
