#!/bin/bash
# train 6: main's tip with B18 on the fast bench: prebuilt, every case both widths, the quiet set
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper
export RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper
export BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains
unset TESTBENCH_QEMU_SEED MAKEFLAGS MFLAGS
export Q_PRIO=0
mk=/home/mcloonan/redoubt/scripts/jobs.mk
wt=/home/mcloonan/redoubt/.worktrees/train-6
log=$wt/target/jobs; mkdir -p $log
echo "train-6 start $(date -u +%FT%TZ) head $(git -C $wt rev-parse --short HEAD)" > $log/train-6.status
make -k -f $mk -C $wt prebuilt > $log/train-6-prebuilt.out 2>&1; echo "prebuilt rc=$? $(date -u +%T)" >> $log/train-6.status
make -k -f $mk -C $wt all-rv64 all-rv32 > $log/train-6-cases.out 2>&1; echo "cases rc=$? $(date -u +%T)" >> $log/train-6.status
echo "train-6 end $(date -u +%FT%TZ)" >> $log/train-6.status
