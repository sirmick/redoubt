#!/usr/bin/env bash
# Build the rv64 kernel and loader64, pack a boot bundle, and boot it on QEMU virt.
# Usage: run-qemu.sh [init-process-elf ...]      (Ctrl-A X exits QEMU)
# Env: SMP (default 1), MEM (256M), DISK (disk image for virtio-blk), QEMU_ARGS (extra).
set -euo pipefail
cd "$(dirname "$0")/.."
TARGET=riscv64imac-unknown-none-elf
OUT=target/$TARGET/release
cargo build --release --target $TARGET -p loader64
cargo build --release --target $TARGET -p xous-kernel --features qemu-virt

# The bundle is a ustar archive: the kernel first, then the initial processes in PID order.
BUNDLE=$(mktemp -d)
trap 'rm -rf "$BUNDLE"' EXIT
cp $OUT/xous-kernel "$BUNDLE/kernel"
NAMES=(kernel)
for elf in "$@"; do
    cp "$elf" "$BUNDLE/"
    NAMES+=("$(basename "$elf")")
done
tar --format=ustar -cf $OUT/bundle.tar -C "$BUNDLE" "${NAMES[@]}"

# shellcheck disable=SC2086
qemu-system-riscv64 -machine virt -smp "${SMP:-1}" -m "${MEM:-256M}" -nographic -bios default \
    -kernel $OUT/loader64 -initrd $OUT/bundle.tar \
    -drive "file=${DISK:-/dev/null},format=raw,if=none,id=d0" -device virtio-blk-device,drive=d0 \
    -netdev user,id=n0 -device virtio-net-device,netdev=n0 ${QEMU_ARGS:-}
