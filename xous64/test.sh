#!/usr/bin/env bash
# xous64 regression test: boot the IPC test pair on QEMU virt and check the verdict.
# Exits 0 on "IPC TEST PASSED", 1 on failure, kernel panic or timeout.
set -euo pipefail
cd "$(dirname "$0")/.."
TARGET=riscv64imac-unknown-none-elf
OUT=target/$TARGET/release
LOG=$(mktemp)
trap 'rm -f "$LOG"' EXIT

cargo build --release --target $TARGET -p ipc-test
QEMU_ARGS="-display none -serial file:$LOG -monitor none" SMP="${SMP:-1}" \
    timeout "${TIMEOUT:-60}" loader64/run-qemu.sh $OUT/ipc-server $OUT/ipc-client >/dev/null 2>&1 &
QEMU=$!

verdict=timeout
for _ in $(seq "${TIMEOUT:-60}"); do
    if grep -aq "IPC TEST PASSED" "$LOG"; then verdict=pass; break; fi
    if grep -aqE "IPC TEST FAILED|PANIC" "$LOG"; then verdict=fail; break; fi
    kill -0 $QEMU 2>/dev/null || break
    sleep 1
done
pkill -P $QEMU 2>/dev/null || true
kill $QEMU 2>/dev/null || true

grep -a -A40 "loader64:" "$LOG" | grep -av "^    \|^ [a-zA-Z] *|\|^key \|^--- \|^====\|Kernel Debug\|Kernel arguments" || true
echo "xous64 test: $verdict"
[ "$verdict" = pass ]
