#!/bin/bash
# alone reruns of the wave bench's shared-run failures, after train-1.sh ends
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper
export RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper
export BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains
unset TESTBENCH_QEMU_SEED
eval "$(/home/mcloonan/redoubt/.wash/local/jobserver env)"
js=/home/mcloonan/redoubt/.wash/local/jobserver
wt=/home/mcloonan/redoubt/.worktrees/train-1
log=$wt/target/jobs/reruns; mkdir -p $log
while pgrep -f '^/bin/bash /tmp/train-1.sh' >/dev/null; do sleep 30; done
echo "reruns start $(date -u +%FT%TZ)" > $log/status
cd $wt
for wc in "rv64 userland-read-only" "rv64 bench-ssh-loopback-exit" "rv64 bench-ssh-loopback-host-key" "rv64 memory-host-tests" \
          "rv32 userland-read-only" "rv32 init-boot" "rv32 netd-restart" "rv32 aio-many-reads-two" "rv32 bench-ssh-loopback-exit" "rv32 bench-ssh-loopback-host-key" "rv32 host-tests"; do
  set -- $wc
  $js all cargo testbench --arch $1 $2 > $log/$1-$2.log 2>&1; rc=$?
  line=$(grep -E '^(PASS|FAIL|SKIP) ' $log/$1-$2.log | head -3 | tr '\n' ';')
  echo "$1 $2 rc=$rc $line" >> $log/status
  # keep this run's consoles
  d=$(ls -td target/testbench/run-* 2>/dev/null | head -1); [ -n "$d" ] && cp -r "$d" "$log/$1-$2-run" 2>/dev/null
done
echo "reruns end $(date -u +%FT%TZ)" >> $log/status
