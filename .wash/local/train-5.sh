#!/bin/bash
# train 5: main's tip through q and the prebuilt directory; the quiet set runs after B18 lands
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper
export RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper
export BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains
unset TESTBENCH_QEMU_SEED MAKEFLAGS MFLAGS
export Q_PRIO=0
mk=/home/mcloonan/redoubt/scripts/jobs.mk
wt=/home/mcloonan/redoubt/.worktrees/train-5
log=$wt/target/jobs; mkdir -p $log
echo "train-5 start $(date -u +%FT%TZ) head $(git -C $wt rev-parse --short HEAD)" > $log/train-5.status
make -k -f $mk -C $wt prebuilt > $log/train-5-prebuilt.out 2>&1; echo "prebuilt rc=$? $(date -u +%T)" >> $log/train-5.status
make -k -f $mk -C $wt cases-rv64 cases-rv32 > $log/train-5-cases.out 2>&1; echo "cases rc=$? $(date -u +%T)" >> $log/train-5.status
echo "train-5 cases end $(date -u +%FT%TZ)" >> $log/train-5.status
