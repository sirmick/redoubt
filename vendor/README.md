# Vendored crates

Third-party source kept in the tree so that it is read and built from here, not fetched. Each
crate is used through a `[patch.crates-io]` path in the root `Cargo.toml`.

## ipd's TCP/IP stack (answer 174)

`ipd` runs `smoltcp`. Mick chose to vendor it and its dependencies (2026-09-24). Each directory
below is the crate exactly as crates.io published it: the `.crate` file, unpacked, with nothing
added, removed or edited. The checksum is the published `.crate` file's SHA-256, as the
crates.io index records it.

| Crate | Version | License (ours to use under) | crates.io SHA-256 |
| --- | --- | --- | --- |
| `smoltcp` | 0.14.0 | 0BSD | `b6f8b28ad56c6e35524a37dd492af5d1a47e31e1a4d175cd12f89c075f01980f` |
| `managed` | 0.8.0 | 0BSD | `0ca88d725a0a943b096803bd34e73a4437208b6077654cc4ecb2947a5f91618d` |
| `heapless` | 0.9.3 | MIT (of MIT OR Apache-2.0) | `25ba4bd83f9415b58b4ed8dc5714c76e626a105be4646c02630ad730ad3b5aa4` |
| `hash32` | 0.3.1 | MIT (of MIT OR Apache-2.0) | `47d60b12902ba28e2730cd37e95b8c9223af2808df9e902d4df49588d1470606` |
| `stable_deref_trait` | 1.2.1 | MIT (of MIT OR Apache-2.0) | `6ce2be8dc25455e1f91df71bfa12ad37d7af1092ae736f3a6cd0e37bc7810596` |
| `byteorder` | 1.5.0 | MIT (of Unlicense OR MIT) | `1fd0f2584146f6f2ef48085050886acf353beff7305ebd1ae69500e27c67f64b` |

Their licence texts are in `LICENSES/` (`smoltcp-0BSD.txt`, `heapless-MIT.txt`, ...), as well as
in each directory.

**Left to `Cargo.lock`, not vendored.** smoltcp also needs these two. Both are already locked
from crates.io for other packages: `cfg-if` reaches the kernel through `getrandom`, and
`bitflags` reaches the test programs through `uart_16550`. A patch applies to every user, so
vendoring them would move those packages' dependencies as well. Both are small and
macro-only. They stay pinned by version and checksum instead.

| Crate | Version | License | crates.io SHA-256 |
| --- | --- | --- | --- |
| `cfg-if` | 1.0.0 | MIT OR Apache-2.0 | `baf1de4339761588bc0619e3cbc0120ee582ebb74b53b4efbf79117bd2da40fd` |
| `bitflags` | 1.3.2 | MIT OR Apache-2.0 | `bef38d45163c2f1dde094a7dfd33ccf595c92905c8f8f4fdc18d06fb1037718a` |

**Locked, never built.** `Cargo.lock` also lists `defmt`, `defmt-macros`, `defmt-parser` and
`thiserror` 2.0.0. smoltcp's `alloc` feature names `defmt?/alloc`, a weak feature, and the
lockfile resolves optional dependencies whatever the features. None of them is compiled:
`cargo tree -p redoubt-vendor-check -e normal` shows only the crates above. `thiserror` is
held at 2.0.0 so that no other package's locked `proc-macro2` moves.

**Checked.** `tools/vendor-check` (the `vendor-check` bench case, `kind = "host-tests"`) proves:
- every file matches `vendor/SHA256SUMS`, and no file has been added or removed;
- `Cargo.lock` builds each vendored crate from its path, at its version, with no registry copy;
- `cargo metadata` resolves each one to `vendor/<name>/Cargo.toml` in this tree, so the patches
  point here, and no registry copy of it is in the graph;
- `cfg-if` and `bitflags` are locked at the versions and checksums above;
- this table agrees with the test's own.

**Integrity, not provenance.** `vendor/SHA256SUMS` is generated from the tree itself (see
"Updating"), and the table's `.crate` checksums are compared only with the test's constants, never
with the bytes. So the bench proves that nothing has changed since the sums were written. It does
not prove that the tree is what crates.io published: someone who edits a file, regenerates the
sums and updates both tables passes it. Provenance is checked against crates.io itself, by

```
tools/vendor-check/provenance.sh
```

For each crate in the first table it downloads the `.crate` from `static.crates.io`, checks its
SHA-256 against the live crates.io index and against this table, unpacks it, and `diff -r`s it
against `vendor/<name>`. It exits 0 only if all three agree for every crate. It needs the network,
so the bench does not run it; **the review of any change under `vendor/` must run it** and quote
its output. It passed for 737a0a41a (the red team's independent run, QA D3-code-review-1) and at
the provenance commit that added it.

**Not workspace members.** The six directories are in the root manifest's `exclude`, not in
`members`. Were they members, `Cargo.lock` would take in their dev-dependencies (test
frameworks, `rand`, `url`, ...), and `cargo test --workspace` would build their default `std`
and `libc` features. As path dependencies they build only with the features `ipd` asks for:
`alloc`, `medium-ethernet`, `proto-ipv4` and `socket-tcp`.

**Outside the unsafe ratchet and rustfmt.** `tests/unsafe-budget.toml` counts code this
repository owns, and these crates are third-party code pinned by these checksums. What that
leaves:
- `smoltcp` is `#![deny(unsafe_code)]`, apart from `rand.rs` and the host phy backends, which
  `ipd` does not compile.
- `heapless` does use `unsafe`. smoltcp uses it only through `Vec` and `LinearMap`, and those two
  modules are read before `ipd`'s stack lands (that commit records it).

rustfmt ignores `vendor/`.

**Warnings.** Cargo caps lints only for registry and git packages; a path package, patched or
not, is built as local code, and there is no per-package lint cap on stable (`profile-rustflags`
is nightly-only). A cold build prints four warnings, all from `managed`: two
`mismatched_lifetime_syntaxes` and two `redundant_semicolons`. `heapless` prints none with
rustc 1.98.1. Nothing builds with `-D warnings`, and silencing them by a workspace-wide
`-A` flag would hide the same lints in our own code, so they are left.

**Build scripts,** which run on the build host. Both were read.
- `smoltcp`'s reads `SMOLTCP_*` environment variables to size its buffers. `ipd`'s own build
  script refuses to build while any is set, so the compiled configuration is the one in the
  source.
- `heapless`'s sets a cfg for a few 32-bit targets that are not ours. It also compiles a
  one-line ARM `clrex` probe with the build's own `rustc`; on RISC-V the probe fails, so it sets
  nothing.

**Updating.** Take the new `.crate` files from crates.io and check their SHA-256 against the
index. Unpack each over an emptied directory, then regenerate the sums:

```
(cd vendor && find smoltcp managed heapless hash32 stable_deref_trait byteorder -type f \
    | LC_ALL=C sort | xargs sha256sum) > vendor/SHA256SUMS
```

Update both tables here and the constants in `tools/vendor-check/tests/vendored.rs` in the same
commit, run `tools/vendor-check/provenance.sh`, and read the diff.

## `getrandom`

Vendored earlier for the kernel's `rand`; not covered by `vendor-check`.
