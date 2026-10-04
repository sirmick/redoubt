# K16 handoff from k16-implementer (1) to k16-implementer-2

## Branch and commits

Worktree `/home/mcloonan/redoubt/.worktrees/k16`, branch `wp-k16`, base main `ca5a6437b`
(INIT1 and GATE1 not merged yet). Two commits, both clean (no WIP):

1. `91c893e29` kernel: every walk of the threads visits only the threads that exist (brief commit 1).
2. `277a2c060` kernel: a table sized by a limit costs RAM, not image, and is never copied onto
   the stack (brief commit 4, values unchanged, done out of order on the orchestrator's word).

Commit 4 is COMPLETE in content. When I handed off, its whole bench (`in-dev cargo testbench
--allow-skip`) was running in the background, log `.k16/whole-c4.log` in the worktree (last line
`exit N`). Owed: read that log. The bar is exactly one SKIP (bench-ssh-loopback-openssh), every
other case PASS, exit 0. Then report commit 4 to the orchestrator: assignment
a88b63eb0220c441e30b2fc90e1f1802, report = .data/.bss/image per width + the stack arrays gone.
If a case fails, fix it in commit 4 itself (amend; history is for auditing, no fix-up commits).

Scratch files (never staged): `.k16/` in the worktree, holding logs, `sizes.sh` (section sizes
both widths), `syms.sh` (largest data symbols), `nz.sh` (non-zero bytes of .data), and
`checkpoint-c1.md`.

## Commit 1 (done, checkpoint accepted)

`Account::live: u64` TID mask (set/cleared in give/take_ipc_frame), `budget::pids()`,
`MemoryManager::live_tids(pid)`. Every walk is on them in the same (pid, tid) order: find_thread,
budgets_dying's pass, next_timeout, process_ending, poke_receivers, process_ended,
destroy_subtree's victims, handle.rs sweeps. ipc.md, timer.md and budgets.md bounds reworded.

Numbers are in `.k16/checkpoint-c1.md`. R10 p99 µs before -> after:
- gate (`kernel-containment`, measured in a scratch worktree at wp-gate1 `71322bcf9`, now
  removed): seed 4 rv64 22128->20733, rv32 22172->20769; seed 3 rv64 21786->20210, rv32
  21769->20551.
- endpoint-destroy-full s3: rv64 11082->10468, rv32 11104->10475.
- sched-latency s3: rv64 6366->5601, rv32 6576->5829.

rv32 sched-latency lease end p99 went 47000 -> 57193 µs (net decision wake 40424 -> 51364,
target 95000; schedule shift). All targets were met. No rerun was asked of me. Treat a rerun as
OWED only if the orchestrator or red asks; at the values commit the gate and sched-latency rerun
anyway.

## Commit 4 (done, bench pending)

- MEMORY_MANAGER, PROCESS_TABLE, PID_SLOTS and SCHED are .bss on both widths (`nm`: `b`).
  - mem.rs: `LoaderTable<T>(Option<&'static mut [T]>)` with Deref/DerefMut; `extra_regions:
    Option<&'static [u32]>`.
  - budget.rs: counters are `last_id`/`last_seq`/`last_msg_id` (last handed out);
    `Account::NONE` is all zeros; `process_created` sets `earliest_timeout: u64::MAX`.
  - ptable.rs: `Process.pid: Option<Pid>`, private, read with `pid()`; empty `current_thread`
    is 0; `init_from_memory` sets `current_thread = INITIAL_TID`.
  - arch process.rs: `PidSlots.current: Option<Pid>` (None = kernel).
- dma.rs (GRANT: Registry only): `slots: [Slot; N]` + `used: u16`; `State::Free` first; `Run::FREE`;
  `Registry::registered()`/`slot(i)`.
- libs/stride (GRANT: reconcile only): in-place removal in slot order; unit test
  `a_reconcile_takes_budgets_out_in_slot_order` (cargo test -p redoubt-stride passes).
- kernel/src/redoubt.rs (GRANT: read_slots only): reads each slot as its frame is checked.
- process_start: one `handles` array; decode checked first, then redone at the lookup.
- sched.rs: `Runnable { list, n }` in `Sched`; free fn `reconcile(cpu, mm, runnable)`.
- Sizes (bytes) before -> after: rv64 image 221302->131278, .data 89592->32, .bss 2096->92680;
  rv32 image 222666->138912, .data 83916->32, .bss 2056->86972.
- tests/size-budget.toml: kernel 7752->7769, libs/stride 328->329, with `Size budget:` lines in
  the commit. INIT1 also edits that file, so expect a small conflict at the rebase.
- Remaining N-sized arrays I know of on the stack: budget.rs boot_budgets' `bundle` (INIT1
  deletes it). `redoubt.rs` record_frames::<N> keeps a frames array for fixed records (N is small).

## Red's three notes for commits 2 and 3

NOT RECEIVED by me: no red-team message reached k16-implementer before handoff. Ask the
orchestrator for them.

## Findings to carry

- PID 1: kmain runs as PID 1 thread 1. `ProcessTable::init_from_memory` calls
  `setup_first_thread(KERNEL_PID, 0, 0, 0)`. `sched::switch_to`'s S-mode ecall saves and resumes
  it through `_start_trap` as context 1 of PID 1's header pages. PID 1 has no IPC pages, so under
  ruling 1 its context is the header's "no thread" area (its TID table is empty). The Architect
  wanted this stated, and it was, in the checkpoint.
- Commit 2 conflicts with INIT1 line for line. INIT1's `setup_loader_process(pid, entry, sp, a0,
  a1)` writes `process_impl().threads[INITIAL_TID - 1].registers[10] = a1`, which is the array
  commit 2 removes. The loader process's header frame must be recorded at boot (INIT1's
  boot_budgets: `process_created(INIT_PID, root)` then `thread_created`). For a created process
  record it in `MemoryMapping::allocate` (arch mem.rs, which already has `context_phys`).
  `process_created` replaces the whole Account, so set the header after it.
- The trap entry (`asm.rs` `_start_trap`) today: `RESTORE x1, 1; slli x1, x1, ctx_shift; add sp,
  sp, x1`. Under ruling 1 it becomes one load of slot 1 as the context address (`ld/lw sp, 8*1/4*1
  (sp)` style); physmap is mapped in every address space, so the physmap address works.
- The gate pins seed 4 (`tests/kernel-containment.toml` on GATE1). The brief says seed 3, so I
  measured both.
- In-dev quirks: each `in-dev` call is a fresh container (its /tmp is lost), so write scratch to
  `.k16/`. A kernel-only build needs `--features qemu-virt`. The testbench name filter is a
  substring (`sched-latency` also runs `sched-latency-tcg`). `TESTBENCH_QEMU_SEED=N` goes through
  `in-dev env TESTBENCH_QEMU_SEED=N cargo testbench ...`. Never edit sources while a bench builds
  in the same worktree; use a detached scratch worktree under `.worktrees/` for before/after.

## Order of what remains

1. Read `.k16/whole-c4.log`. If it is green, report commit 4 (assignment a88b63eb...) and wait.
2. On the orchestrator's word that INIT1 merged: rebase wp-k16 on main (fold the size-budget
   conflict). Run the real `kernel-containment` on the branch before commit 3. Then commit 2
   (contexts in the IPC page; trap-entry diff against INIT1's merged setup_loader_process; whole
   bench both widths). Then commit 3 (16-bit PIDs, satp without the PID; `pid-reuse-authority`,
   `uaf-lent-page` by name). Report the brief's early-checkpoint items still owed: the trap entry's
   diff.
3. Then brief commits 5 (the values), 6 (process-fill), 7 (thread-limit 255), 8 (worst walk),
   9 (depth 16), and the Acceptance.

## Do not re-read

- `.wash/qa/K16-limits.md` (the brief carries it).
- The SWARM sections beyond "The implementer" and "Staging, commits and handoffs".
- `docs/kernel/budgets.md` "Residual risks" history (only item 4 and the R10 numbers matter).
- INIT1's whole diff: read only its `kernel/src/arch/riscv/process.rs`, `ptable.rs` and
  `budget.rs` boot hunks after the rebase.
- message.rs beyond "Walking the threads", the thread-page word layout (W_* constants, lines
  ~64-97) and the walk functions near the end (process_ending, budgets_dying, next_timeout,
  poke_receivers).
