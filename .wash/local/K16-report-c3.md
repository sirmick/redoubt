# K16 report after commits 2 and 3 (k16-implementer-3)

## Branch wp-k16 on main 5f9f9d61c

1. 3cc54449f kernel: every walk of the threads visits only the threads that exist (c1)
2. 1d7f7c221 tests: the shell's victim keeps at least half, with no ceiling (ruling K16-churn-ceiling)
3. 388e9b9aa kernel: a table sized by a limit costs RAM, not image, ... (c4)
4. 774cf40a1 kernel: a thread's saved registers live in its IPC page (c2)
5. 941b713ad kernel, loader: a PID is 16 bits, and satp carries none (c3)

## Commit 2 (folded)

- Aliasing fix: `set_tid` and `setup_empty_process` compute `context_addr` before borrowing the
  header. A third overlap fixed too: `setup_first_thread` held the header while taking
  `context(INITIAL_TID)`, which is the header's own `no_thread` area for PID 1.
- A stale per-width row is fixed: memory-layout.md `PROCESS_AREA pages | 1 | 2` -> `1 | 1`.
- Size budget: kernel 7863 -> 7894 (+38 lines; the message gives the reason). Loader 868 -> 866.
- Unsafe count unchanged: `process_impl`'s unsafe is generalised to `kernel_ref`.
- Trap entry (asm.rs `_start_trap`):
  ```
  -    RESTORE x1, 1                   // Load the current context number (header slot 1)
  -    slli    x1, x1, {ctx_shift}     // Each context is 1 << ctx_shift bytes
  -    add     sp, sp, x1              // sp = &contexts[current]
  +    RESTORE sp, 1                   // sp = the current context's address (header slot 1)
  ```
- What PID 1 runs: kmain is PID 1, thread 1. Its context is the header's `no_thread` area,
  because PID 1 has no budget and so no IPC pages; its IPC table is empty.
- Pump and R10 times before and after commit 1, both widths: `.k16/checkpoint-c1.md` (accepted).

## Commit 3

- `Pid` is now `NonZeroU16`. The loader's RPT and XPT are `Option<Pid>` (2 bytes a page, sized
  for that). OBJECT_OWNER and DMA_OWNER are 0xffff and 0xfffe.
- New `budget::pid_from` refuses a value too wide instead of truncating it. Every PID decode uses
  it, and no `as u8` remains on a PID or TID (red's note b). The cursor is `(Pid, TID)`, packed as
  pid<<16 | tid+1.
- satp's ASID is 0 on both widths, and `make_satp(root)` takes the root alone. Removed:
  `satp_pid`, `pid_from_satp`, `SATP_ASID_*`, `MemoryMapping::get_pid` and `is_kernel`, and the
  satp check in `Process::current`. `arch::current_pid` now re-exports `PID_SLOTS.current`.
- `setup_loader_process` now calls `set_current_pid` after `claim`. `switch_to` activates the
  mapping before `setup_empty_process` asserts the PID, which satp's ASID used to supply.
- `get_process` and `get_process_mut` now check the slot's own `pid`. `InitialProcess` has a
  `pid` field on both sides.
- Docs: memory-layout.md's satp section and per-width table, boot.md (the tables and the record),
  processes.md.
- Size budget: kernel 7894 -> 7875 (falls). Loader 866 -> 871 (`Size budget: loader:` line).

## Tests (in-dev cargo testbench --allow-skip <filter>, both widths unless noted)

Commit 3 (941b713ad, same tree as 159de631d before the rebase): every case below passes.

- pid-reuse-authority (2 PASS).
- uaf-lent-page (1 PASS; its toml is rv64-only).
- size-budget, unsafe-budget.
- boot 8, process 12, budget 27, pid-pinning 2, thread 2, dma 8, kernel-containment 2,
  endpoint-destroy-full 2, redoubt-ipc 4, stub-launch 2, map-fixed 4, page-table-reclaim 2,
  pages-exhaustion 2, sched-latency 4, sched-timer-flood 2.
- docs, and host-tests (all host-test cases, model 526 s).

The only FAIL was rv64 sched-budget-churn, before the ruling's commit was in. At the tip
everything passes: size-budget, docs, unsafe-budget, and sched-budget-churn on both widths (shell's
victim 577 net on rv64, 507 on rv32).

Commit 2 focused set (k16-b at 326398dd3): every case passes except size-budget (fixed by the
amend) and churn. kernel-containment, sched-latency, sched-timer-flood, endpoint-destroy-full,
redoubt-ipc and boot all pass.

The whole bench has not been run; it waits for your word.

## Churn ruling

- Evidence run (lift measured from the floor; uncommitted, reverted). rv64 FAIL:
  `record 4920: budget 107's lift of budget 115 gave pass 0x487b536c9e rem 42, but the rule gives
  0x487b5263ef rem 44 (parent 0x487b5263ef rem 44, child 0x48435eded0 rem 0 entry 0x48435eded0,
  floor 0x4842f77a76, weights 1 and 100)` (`.k16/evidence-entrywait.log`).
- Numbers written into the page (measured at c1+T): rv64 580 (391 gross), rv32 497 (316).
- The page's spinning-parent figures, "494 (403) / 495 (416)", now measure 494 (428) / 494 (431).
  I left them, since the ruling named only the shell's.

## Open

- docs/beyond/fpga-platform.md ("The kernel already puts the process ID in `satp`") is now false
  and is outside my owned paths. Proposed text: "The kernel writes ASID 0 in `satp` and flushes the
  whole TLB ...". Do you want it in commit 3?
- main.rs (commit 2, one call site) and SECURITY.md (commit 2, one row) are also outside the brief's
  list.
- uaf-lent-page is rv64-only by its toml.

## K21 rebase needs

- arch mem.rs `allocate`: K21 splits the context-page loop into `add_context_page`, with unwinding
  through `release_owned_frames`. Commit 2 replaces the loop with one header page plus
  `mm.set_header`. Keep K21's unwind around one header page, and call `set_header` after the map
  succeeds. `make_satp(root_phys)` (commit 3).
- `release_all_memory_for_process` is renamed `release_owned_frames`. Commit 2's
  `release_ipc_frames(pid)` must stay before it in `terminate`, and must also run in any K21
  unwind path that frees a process with live threads.
- mem.rs: K21's `FreeList {head, tail, len}` is all zero, so the memory manager stays `.bss`
  (check with `.k16/sizes.sh`). `set_owner` writes `Option<Pid>`, now u16 (the same type).
  `DMA_OWNER` is 0xfffe for the pool.
- The boot_budgets comment "saved contexts" -> "header page". process.rs and ptable.rs: comment
  and call renames only.
