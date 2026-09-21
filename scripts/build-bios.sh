#!/usr/bin/env bash
#
# Build the RustSBI Prototyper firmware that the test bench boots under.
#
# The firmware is vendored in-tree at bios/. QEMU ships no rv32 OpenSBI, so
# `cargo testbench --arch rv32` boots the RustSBI Prototyper instead (rv64 uses QEMU's
# bundled OpenSBI by default but can also use RustSBI). This script builds the Prototyper
# for both widths, leaving the ELFs exactly where the bench looks for them:
#
#   bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper
#   bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper
#
# The bench (tools/testbench, resolve_firmware) defaults to bios/; override with
# RUSTSBI_PROTOTYPER / RUSTSBI_PROTOTYPER_RV32.
#
# RustSBI pins its own nightly toolchain in bios/rust-toolchain.toml, which rustup selects
# automatically.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"   # the repo root
dest="${RUSTSBI_DIR:-$here/bios}"

if [ ! -f "$dest/Cargo.toml" ]; then
    echo "error: no RustSBI checkout at $dest (expected the vendored bios/ tree)" >&2
    exit 1
fi

# The Prototyper's xtask converts the ELF to a raw .bin with rust-objcopy, so it needs
# cargo-binutils. (The bench boots the ELF, not the .bin, but xtask fails without it.)
if ! command -v rust-objcopy >/dev/null 2>&1; then
    echo "==> installing cargo-binutils (needed by the Prototyper's xtask)"
    cargo install --locked cargo-binutils@0.4.0
fi

build() {
    local label="$1" target="$2"
    echo "==> building Prototyper for $label ($target)"
    ( cd "$dest" && cargo xtask prototyper build ${target:+--target "$target"} )
}

# Default target (rv64, riscv64gc) needs no --target flag; rv32 is explicit.
build rv64 ""
build rv32 "riscv32imac-unknown-none-elf"

echo "==> done. Firmware:"
for t in riscv64gc-unknown-none-elf riscv32imac-unknown-none-elf; do
    bin="$dest/target/$t/release/rustsbi-prototyper"
    [ -f "$bin" ] && echo "    $bin" || { echo "    MISSING: $bin" >&2; exit 1; }
done