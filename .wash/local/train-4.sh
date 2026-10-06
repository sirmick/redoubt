#!/bin/bash
# train 4: the whole bench on main's tip through q (the acceptance run of the scheduler)
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper
export RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper
export BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains
unset TESTBENCH_QEMU_SEED MAKEFLAGS MFLAGS
export Q_PRIO=0
mk=/home/mcloonan/redoubt/.wash/local/jobs.mk
wt=/home/mcloonan/redoubt/.worktrees/train-4
log=$wt/target/jobs; mkdir -p $log
echo "train-4 start $(date -u +%FT%TZ) head $(git -C $wt rev-parse --short HEAD)" > $log/train-4.status
make -k -f $mk -C $wt build-rv64 build-rv32 > $log/train-4-build.out 2>&1; echo "build rc=$? $(date -u +%T)" >> $log/train-4.status
make -k -f $mk -C $wt all-rv64 all-rv32 > $log/train-4-cases.out 2>&1; echo "cases rc=$? $(date -u +%T)" >> $log/train-4.status
echo "train-4 end $(date -u +%FT%TZ)" >> $log/train-4.status
