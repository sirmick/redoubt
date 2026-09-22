# Vendored: RustSBI Prototyper

`bios/` is a vendored checkout of [RustSBI](https://github.com/rustsbi/rustsbi), used as the
only M-mode firmware the bench boots on both rv32 and rv64.

- Upstream: https://github.com/rustsbi/rustsbi
- Pinned commit: `eae4cc70860d0e52be125e7aa89960d9f030d088`
- Build: `./scripts/build-bios.sh` → `bios/target/<triple>/release/rustsbi-prototyper`

Redoubt carries a small `qemu-virt` Cargo feature patch. The build script enables it so only
QEMU virt's 16550 UART, SiFive CLINT, and SiFive test/finisher reset drivers are compiled.
The generic profile and all of its driver source remain available for future softcores.
RustSBI pins its own nightly toolchain in `rust-toolchain.toml`. Bump the pin deliberately,
rebase the profile patch, and re-run the bench.
