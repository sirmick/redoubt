#!/bin/bash
# train 7: B22, K25, B14 on main ef2ab5a94: prebuilt, every case both widths, the quiet set
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper
export RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper
export BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains
unset TESTBENCH_QEMU_SEED MAKEFLAGS MFLAGS
export Q_PRIO=0
mk=/home/mcloonan/redoubt/scripts/jobs.mk
wt=/home/mcloonan/redoubt/.worktrees/train-7
log=$wt/target/jobs; mkdir -p $log
echo "train-7 start $(date -u +%FT%TZ) head $(git -C $wt rev-parse --short HEAD)" > $log/train-7.status
make -k -f $mk -C $wt prebuilt > $log/train-7-prebuilt.out 2>&1; echo "prebuilt rc=$? $(date -u +%T)" >> $log/train-7.status
make -k -f $mk -C $wt all-rv64 all-rv32 > $log/train-7-cases.out 2>&1; echo "cases rc=$? $(date -u +%T)" >> $log/train-7.status
echo "train-7 end $(date -u +%FT%TZ)" >> $log/train-7.status
