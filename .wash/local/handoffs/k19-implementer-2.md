# K19 implementer-2 final handoff (K19 accepted, merging; K24 merged earlier)

## Branch state
- wp-K19 head 21eda8acf on main bdb38430e, four commits, accepted; the orchestrator merges. Never pushed by me.
- /home/mcloonan/redoubt/.worktrees/K19 is clean. The K24 worktree was removed after its merge.
- No processes of mine running.

## What in .wash/local/K19* should outlive me
- .wash/local/K19-report.md. Its last sections are the record of every rebase (hunks resolved), the rv32 expiry regression and its fix (Page::Pumped), and the red's two model P2s.
- .wash/local/K24-report.md: the slice-at-return fix and its gates.

## Worst-walk measurement procedure (RECON1 builds on it)
1. Run: `make -f /home/mcloonan/redoubt/scripts/jobs.mk -C <wt> prebuilt`, then `make ... rv64/worst-walk rv32/worst-walk` (detached; about 10-13 min a width, 1 core each). The case is whole_run=false, so run it by name.
   - The case uses sched-trace-large (192 MiB ring). At the 1 ms slice it writes about 2.6M records (rv64) and 2.9M (rv32). The 64 MiB ring overflows ('dropped N'), which fails the case.
2. R10 numbers: `grep -a -o 'R10 2 destructions[^;]*;[^;]*' <wt>/target/jobs/rv{64,32}-worst-walk.log`.
   - At 21eda8acf's code: p50/p99 16354/17098 µs (rv64), 17381/18210 µs (rv32).
   - Threads' ending: 13097 and 13796 µs.
3. The expiry walk (R12's 30 ms bound, the tightest margin: rv32 about 29.3 ms on main) is not printed on a pass. Compute it from the console trace with /tmp/expiry.py (may be gone; it is 25 lines):
   - pair 'SCHED-TRACE <i> <entry> M 2 <hex µs>' with the next 'm 2 <hex µs>' (walk id 2 = EXPIRY; 1 pump, 3 reconcile);
   - subtract the U/V audit spans inside;
   - the max is what the oracle's expiry_max_us judges.
   - Console logs: <wt>/target/testbench/run-*/worst-walk-rv{64,32}-smp1.log, over 100 MB, so never cat them.
   - Measured: rv32 29,297-29,320 µs, rv64 27,142-27,150 µs.
4. The expiry walk has no trace records inside it; it is pure code: collect_due plus the ipclist merge sort over 250 due waits, about 10k checked list reads. A few instructions per read moves it by hundreds of µs.
   - Example: K19's 4th list-member kind briefly cost +850 µs on rv32.
   - Compare builds with `nm -C -S` on target/prebuilt/rv32/cargo-qemu-virt+sched-trace-large+walk-trace/riscv32imac-unknown-none-elf/checked/redoubt-kernel. Look at List::page, insert_after, remove, and ProcessTable::with_mut<expire_due>.
5. A hang after '250 holders waited' was K24's slice livelock (fixed on main). If it recurs, debug with gdb:
   - add `-gdb tcp::PORT` to QEMU by hand (the bench has no flag);
   - sample with interrupt-only batch gdb scripts reading $sepc, $scause, $satp and TIMER (`nm` for redoubt_kernel::time::TIMER);
   - never time-kill gdb mid-continue: it wedges the stub, and QEMU then needs kill -9.

## Traps
- Rebase tooling:
  - `/tmp/k19-size.sh` recounts the ceilings per commit;
  - `/tmp/k19-ours.py` takes the earlier side of a size-budget.toml conflict.
  - The recount raises every crate over its ceiling, including foreign ones when main itself is over. Check its log and take back any raise outside your package.
- Conflict unions of code blocks can cut functions mid-body. Rebuild from main's file plus your own hunk, and compare changed-line counts.
- `Mutation::ALL`: count the array entries after every merge; a clean merge can leave the count stale.
- Model gates: rv64/model-host-tests and rv64/model-mutations through jobs.mk, now minutes after B18/MODEL1. The q log shows one job per mutation.
