#!/bin/bash
cd /home/mcloonan/redoubt/.worktrees/SCHED1
eval "$(/home/mcloonan/redoubt/.wash/local/jobserver env)"
export PATH=$HOME/.cargo/bin:$PATH RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains
unset TESTBENCH_QEMU_SEED
E=/home/mcloonan/redoubt/.wash/local/evidence/SCHED1/five-cases
J=/home/mcloonan/redoubt/.wash/local/jobserver
run() { # case arch tag
  $J take cargo testbench --arch $2 $1 > $E/$3.bench.txt 2>&1
  echo "$3 rc=$?" >> $E/progress.txt
  d=$(grep -o 'run-[0-9]*-[0-9]*' $E/$3.bench.txt | tail -1)
  [ -z "$d" ] && d=$(ls -t target/testbench | grep '^run-' | head -1)
  f=$(ls target/testbench/$d/$1-$2-smp1.log 2>/dev/null | head -1)
  [ -z "$f" ] && f=$(ls -t target/testbench/run-*/$1-$2-smp1.log 2>/dev/null | head -1)
  [ -n "$f" ] && cp "$f" $E/$3.console.log && echo "$3 saved $f" >> $E/progress.txt
}
for c in large-weight server-busy carve-inflation debt-lift; do run diag-$c rv64 traced-$c; done
run diag-walk rv64 walk-large-weight
run diag-release rv64 release-1ms-rv64
run diag-release rv32 release-1ms-rv32
git status --porcelain > $E/scratch-status-before-edit.txt
sed -i 's/pub const SLICE_US: u64 = if cfg!(feature = "slice-10ms") { 10_000 } else { 1_000 };/pub const SLICE_US: u64 = if cfg!(feature = "slice-10ms") { 10_000 } else { 10_000 };/' kernel/src/sched.rs
git diff kernel/src/sched.rs > $E/scratch-slice-edit.diff
run diag-release rv64 release-10ms-rv64
run diag-release rv32 release-10ms-rv32
git checkout -- kernel/src/sched.rs
git diff kernel/src/sched.rs > $E/scratch-slice-after-revert.diff
rm -f tests/diag-*.toml
git status --porcelain > $E/scratch-status-after.txt
echo done >> $E/progress.txt
