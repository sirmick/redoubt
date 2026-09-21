# Vendored: RustSBI Prototyper

`bios/` is a vendored checkout of [RustSBI](https://github.com/rustsbi/rustsbi), used as the
M-mode firmware the bench boots (especially for rv32, for which QEMU ships no OpenSBI).

- Upstream: https://github.com/rustsbi/rustsbi
- Pinned commit: `eae4cc70860d0e52be125e7aa89960d9f030d088`
- Build: `./scripts/build-bios.sh` → `bios/target/<triple>/release/rustsbi-prototyper`

It builds as-is; no local patches are applied. RustSBI pins its own nightly toolchain in
`rust-toolchain.toml`. Bump the pin deliberately and re-run the bench.