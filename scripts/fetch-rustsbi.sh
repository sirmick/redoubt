#!/usr/bin/env bash
#
# Fetch and build the RustSBI Prototyper firmware that the test bench boots under.
#
# QEMU ships no rv32 OpenSBI, so `cargo testbench --arch rv32` boots the RustSBI
# Prototyper instead (rv64 uses QEMU's bundled OpenSBI by default but can also use
# RustSBI). This script clones RustSBI at a pinned commit and builds the Prototyper
# for both widths, leaving the ELFs exactly where the bench looks for them:
#
#   <rustsbi>/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper
#   <rustsbi>/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper
#
# The bench (redoubt/testbench, resolve_firmware) defaults to a `rustsbi` checkout
# beside this repo; override with RUSTSBI_PROTOTYPER / RUSTSBI_PROTOTYPER_RV32.
#
# No source patch is needed: the pinned commit builds as-is. RustSBI pins its own
# nightly toolchain in rust-toolchain.toml, which rustup selects automatically.
set -euo pipefail

REPO="https://github.com/rustsbi/rustsbi.git"
# Pinned so the firmware is reproducible. Bump deliberately, and rerun the bench.
PIN="eae4cc70860d0e52be125e7aa89960d9f030d088"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"   # the xous-core repo root
dest="${RUSTSBI_DIR:-$(dirname "$here")/rustsbi}"          # sibling checkout by default

echo "==> RustSBI checkout: $dest (pinned $PIN)"
if [ -d "$dest/.git" ]; then
    git -C "$dest" fetch --quiet origin "$PIN" || git -C "$dest" fetch --quiet --all
else
    git clone --quiet "$REPO" "$dest"
fi
git -C "$dest" checkout --quiet "$PIN"

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
