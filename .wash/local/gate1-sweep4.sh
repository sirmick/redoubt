#!/bin/bash
# usage: gate1-sweep.sh arch seed... ; one at a time, from the gate1 worktree
cd /home/mcloonan/redoubt/.worktrees/gate1
a=$1; shift
for s in "$@"; do
  l=/home/mcloonan/redoubt/.wash/local/GATE1-sweep4-logs/$a-$s.log
  /home/mcloonan/redoubt/.wash/local/in-dev env TESTBENCH_QEMU_SEED=$s cargo testbench kernel-containment --arch $a > $l 2>&1
  echo "$a $s exit $?" >> /home/mcloonan/redoubt/.wash/local/GATE1-sweep4-logs/status
done
