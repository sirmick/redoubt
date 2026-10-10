#!/bin/bash
# train 9 (with 10 and 11): K19, B8, B23, B20, B9, BEAM9, BEAM10, K28, RECON1 on main 26f79ed08: prebuilt, every case both widths, the quiet set
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper
export RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper
export BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains
unset TESTBENCH_QEMU_SEED MAKEFLAGS MFLAGS
export Q_PRIO=0
mk=/home/mcloonan/redoubt/scripts/jobs.mk
wt=/home/mcloonan/redoubt/.worktrees/train-9
log=$wt/target/jobs; mkdir -p $log
echo "train-9 start $(date -u +%FT%TZ) head $(git -C $wt rev-parse --short HEAD)" > $log/train-9.status
make -k -f $mk -C $wt prebuilt > $log/train-9-prebuilt.out 2>&1; echo "prebuilt rc=$? $(date -u +%T)" >> $log/train-9.status
make -k -f $mk -C $wt all-rv64 all-rv32 > $log/train-9-cases.out 2>&1; echo "cases rc=$? $(date -u +%T)" >> $log/train-9.status
echo "train-9 end $(date -u +%FT%TZ)" >> $log/train-9.status
