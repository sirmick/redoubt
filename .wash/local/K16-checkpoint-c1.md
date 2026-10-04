# K16 checkpoint on commit 1 alone

Branch wp-k16, tip 91c893e29 (base main ca5a6437b): `kernel: every walk of the threads visits
only the threads that exist`. Commit 2 waits for INIT1's merge (orchestrator's option A).

## What commit 1 does

- `Account::live: u64`: bit `tid` set iff `ipc[tid]` holds a page; set in `give_ipc_frame`,
  cleared in `take_ipc_frame`, reset with the account. Asserted `MAX_THREADS < 64`.
- `budget::pids()` (every PID, lowest first) and `MemoryManager::live_tids(pid)` (the mask's set
  bits, read once; none for a PID with no account).
- Walks moved onto them, same (pid, tid) order, same answers: `find_thread` (receiver pick, R2
  sender pick, `fail_all`, notice pick), `budgets_dying`'s pass, `next_timeout`,
  `process_ending`, `poke_receivers`, `process_ended`'s loop, `destroy_subtree`'s victim loop,
  `handle.rs` `sweep_handles` (skips a PID with no account) and `check_handle_chains`.
- Pages: ipc.md ("Delivery walks every thread"), timer.md ("Expiry walks threads"), budgets.md
  item 4: the walk is bounded by the threads that exist, at most the two constants.

## Numbers (virtual time, checked build, sched-trace; all PASS)

R10 kernel time p50/p99 µs, and the threads' ending (the T..t records, pumps included) p99 µs.

### kernel-containment (wp-gate1 tip 71322bcf9, scratch worktree, commit 1 cherry-picked uncommitted)

| seed | width | R10 p50/p99 before | after | threads' ending p99 before | after | lease end p99 before | after |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 4 (pinned) | rv64 | 18,685 / 22,128 | 17,353 / 20,733 | 3,527 | 3,224 | 30,071 | 28,257 |
| 4 (pinned) | rv32 | 18,749 / 22,172 | 17,387 / 20,769 | 3,708 | 3,354 | 30,505 | 28,688 |
| 3 | rv64 | 18,344 / 21,786 | 17,113 / 20,210 | 3,043 | 2,793 | 29,729 | 27,787 |
| 3 | rv32 | 18,410 / 21,769 | 17,135 / 20,551 | 3,166 | 2,900 | 30,082 | 26,745 |

### endpoint-destroy-full (seed 3; before = main ca5a6437b)

| width | R10 before | after | threads' ending before | after |
| --- | --- | --- | --- | --- |
| rv64 | 11,082 | 10,468 | 34 | 31 |
| rv32 | 11,104 | 10,475 | 37 | 35 |

### sched-latency (seed 3; before = main ca5a6437b)

| width | R10 p50/p99 before | after | threads' ending p99 before | after | lease end p99 before | after |
| --- | --- | --- | --- | --- | --- | --- |
| rv64 | 3,627 / 6,366 | 2,879 / 5,601 | 32 | 29 | 45,815 | 34,280 |
| rv32 | 3,661 / 6,576 | 2,980 / 5,829 | 36 | 32 | 47,000 | 57,193 |

rv32's lease end rose because its net decision wake p99 moved 40,424 -> 51,364 µs (target 95,000):
the schedule shifts with the kernel's timing; every post-check target met. sched-latency-tcg also
passes both widths (R10 p99 rv64 13,451 -> 12,602, rv32 15,225 -> 13,853).

No target regresses. At today's values the saving is about 1.3 ms of R10 at the gate's full fill
(~6%) and ~0.3 ms of the threads' ending; the walk now scales with live threads, which is what
the values commit needs.

## What PID 1 runs

kmain runs as PID 1, thread 1: `ProcessTable::init` calls `setup_first_thread(KERNEL_PID, 0, 0,
0)`, and `sched::switch_to`'s S-mode `ecall` enters `_start_trap`, which saves kmain's registers
as context 1 of PID 1's header pages (`PROCESS_AREA` as the loader mapped it) and resumes them
from there when the CPU comes back. PID 1 has no budget and no IPC pages, so under ruling 1 its
TID table is empty and its context is the header's "no thread" area.

## Commands

All via `/home/mcloonan/redoubt/.wash/local/in-dev`:
`cargo testbench endpoint-destroy-full`, `cargo testbench sched-latency` (also runs
sched-latency-tcg), `env TESTBENCH_QEMU_SEED={4,3} cargo testbench kernel-containment`; every
one exit 0. `cargo build -p redoubt-kernel --release --features qemu-virt` both targets exit 0;
`cargo +nightly fmt --all --check` clean after one fix. Logs: `.k16/*.log` in the worktree.
`cargo testbench docs` (doccheck) PASS. Not yet run on commit 1: the whole bench (runs with commit 2).
