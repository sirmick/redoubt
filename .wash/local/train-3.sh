#!/bin/bash
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper
export RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper
export BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains
unset TESTBENCH_QEMU_SEED
eval "$(/home/mcloonan/redoubt/.wash/local/jobserver env)"
mk=/home/mcloonan/redoubt/.wash/local/jobs.mk
wt=/home/mcloonan/redoubt/.worktrees/train-3
log=$wt/target/jobs; mkdir -p $log
echo "train-3 start $(date -u +%FT%TZ) head $(git -C $wt rev-parse --short HEAD)" > $log/train-3.status
make -k -f $mk -C $wt build-rv64 build-rv32 > $log/train-3-build.out 2>&1; echo "build rc=$?" >> $log/train-3.status
# shared, net and bounded classes of both widths in one make (the pool paces them); exclusive tails serial after
make -k -f $mk -C $wt cases-rv64 cases-rv32 > $log/train-3-cases.out 2>&1; echo "cases rc=$?" >> $log/train-3.status
make -k -f $mk -C $wt exclusive-rv64 > $log/train-3-excl64.out 2>&1; echo "excl64 rc=$?" >> $log/train-3.status
make -k -f $mk -C $wt exclusive-rv32 > $log/train-3-excl32.out 2>&1; echo "excl32 rc=$?" >> $log/train-3.status
echo "train-3 end $(date -u +%FT%TZ)" >> $log/train-3.status
