# K16 handoff from k16-implementer-2 to k16-implementer-3

Read first: this file; the brief `.wash/local/K16-implementer.md` (full; rulings 1-9, deliverables
1-9, Acceptance); the first handoff `.wash/local/K16-handoff.md` (sections "Findings to carry" and
"Do not re-read" still hold). Assignment a8292e925e8faf8a852c506363a9e3cb (INIT1 merged: rebase,
commits 2 and 3 with red's notes, focused cases, HOLD the whole bench for the orchestrator's word,
report after commit 3 with the trap-entry diff and the cases) is OPEN; report it when commit 3 is done.

## Branch

Worktree `/home/mcloonan/redoubt/.worktrees/k16`, branch `wp-k16`, on main `5f9f9d61c` (INIT1).

1. `3cc54449f` kernel: every walk of the threads visits only the threads that exist (commit 1, final)
2. `657d139c0` kernel: a table sized by a limit costs RAM, not image, ... (commit 4, final)
3. `6e1f5e789` WIP: kernel: a thread's saved registers live in its IPC page (commit 2, NOT final)

Scratch, never staged: `.k16/` (logs, `sizes.sh`, `focused.sh`, `tf-seeds.sh`, `sl-seeds.sh`,
`report-b341.md`). Scratch worktrees `.worktrees/k16-main` and `.worktrees/k16-b` (detached, clean)
for before/after builds; reuse or remove them (`git worktree remove`).

ENV: every cargo/bench/fmt command via `/home/mcloonan/redoubt/.wash/local/in-dev <cmd>` from the
worktree; each call is a fresh container (write scratch to `.k16/`). Never edit sources while a bench
builds in the same worktree.

## Rebases done and how conflicts were resolved

- Onto 81b5ea38b (K20): `message.rs` `next_timeout` kept K20's `Timeouts { due, next, stale }` and
  `found`/`stale` logic with commit 1's iteration (`pids()`, `mm.live_tids(pid)`). `sched.rs` `leave()`:
  K20's block kept whole; its `s.reconcile(mm, runnable)` became commit 4's free
  `reconcile(&mut s.cpu, mm, runnable)` at the same place, returning `next.is_some()`.
- Onto 5f9f9d61c (INIT1): only `tests/size-budget.toml` conflicted. Main's kernel ceiling is 7863
  (actual 7828); commit 1 -> 7823, commit 4 -> 7856, so commit 4 needs NO kernel raise: its
  `Size budget: kernel` line was dropped (only the `libs/stride` 328 -> 329 line stays). Commit 4's
  message sizes were re-measured on this base: rv64 image 218,146 -> 130,926, .data 90,104 -> 32,
  .bss 2,096 -> 93,704; rv32 image 218,944 -> 138,718, .data 83,920 -> 32, .bss 2,056 -> 87,488.

## Commit 2 (WIP 6e1f5e789): state

Done (code + docs, both widths build, fmt clean, unsafe-budget PASS, docs PASS, host-tests PASS incl.
model after re-bless):
- `arch/riscv/process.rs`: `ProcessImpl` is the one-page header: `scratch` (slot 0), `context`
  (slot 1, the running thread's context ADDRESS), `hardware_thread` (TID), `inner`,
  `allocated_threads`, `last_tid_allocated`, `no_thread: Thread`, `ipc: [u32; MAX_THREADS + 1]`
  (TID -> object frame). `kernel_ref<T>(addr)` is the one unsafe (header or a context);
  `context_addr(tid)` = physmap(RAM_START + frame*PAGE) + `CONTEXT_OFFSET` (= PAGE_SIZE -
  size_of::<Thread>()), or the `no_thread` area when the table entry is 0 (PID 1's kmain).
  `RAM_START` static set by `mem.rs` `init_from_memory` (`set_ram_start`). `set_tid` writes
  `context`. `destroy_thread` points `context` at `no_thread` when the dying thread is current.
  `setup_empty_process` leaves the `ipc` table alone (init's page is given at boot before init runs).
- `asm.rs` `_start_trap`: `RESTORE x1, 1; slli x1, x1, {ctx_shift}; add sp, sp, x1` became
  `RESTORE sp, 1` (one load, two instructions fewer); `CTX_SHIFT` and its assert gone. That is the
  trap-entry diff the report owes (`git show 6e1f5e789 -- kernel/src/arch/riscv/asm.rs`).
- `budget.rs`: `Account::ipc` gone; `Account::header: usize` (header phys, 0 none). `ipc_frame` and
  `set_ipc_entry` read/write the header's table through `kframe` (`ipc_entry(tid)` = word offset +
  shift; little-endian). `set_header(pid, phys)` (called in `arch mem.rs` `MemoryMapping::allocate`
  after the header is mapped, and in `boot_budgets(init_header)` after `process_created(INIT_PID)`).
  `release_ipc_frames(pid)` frees the IPC pages; `ptable.rs` `terminate` calls it BEFORE
  `release_all_memory_for_process` (the header names them); `process_ended` now
  `debug_assert!(live == 0)` instead of freeing them. `INIT_PID` is `pub(crate)`.
- `main.rs`: kmain computes init's header via `arch::mem::header_phys(&mapping_of(INIT_PID))` and
  passes it to `boot_budgets`. (main.rs is outside the brief's owned list: one call site; say so in
  the report.)
- `libs/layout`: `THREAD_CONTEXT_PAGES` gone; loader `map_context` maps one header page.
- `message.rs`: assert `THREAD_WORDS * 8 <= CONTEXT_OFFSET`.
- model: `Costs::contexts` default 2 -> 1 (field name kept: trace format v2); example trace
  re-blessed (`REDOUBT_MODEL_BLESS=1 cargo test --release --test traces the_example_trace`), diff is
  contexts=1 and usage pages -1.
- Docs: memory-layout.md "Per-process kernel data" rewritten; objects.md cost-table row ("process
  header | 1"), paragraph, status line, and the "Accounting on both widths" residual removed;
  processes.md, budgets.md (x3), model.md (costs line; Open item's rv32 cost table removed),
  README.md (x2), SECURITY.md row, todo/kernel-attack-gaps.md (x2).
- `tests/size-budget.toml`: kernel 7856 -> 7893 (commit 2's final message needs
  `Size budget: kernel: <reason>` for +37); loader lowered 868 -> 866.

OWED for commit 2:
1. Aliasing fix: in `set_tid` and `setup_empty_process`, call `context_addr(...)` BEFORE `let process
   = process_impl()` (two live `&mut` to the header otherwise, against `kernel_ref`'s safety text).
2. Focused cases: a run was in flight at handoff (`.k16/focused.sh budget process map-fixed
   page-table-reclaim pages-exhaustion redoubt-tight pid-pinning thread kernel-containment
   endpoint-destroy-full redoubt-ipc boot bundle stub-launch init sched-latency sched-timer-flood`,
   summary `.k16/foc-summary.txt`, logs `.k16/foc-<filter>.log`, ends with `ALLDONE`). It was started on
   the WIP tree minus fix 1. Read it; expect cases whose expected charges counted 2 rv64 context
   pages to change (none hard-code it that I found; check failures).
3. The whole bench: HELD until the orchestrator says (one merge-gate bench at a time on this host).
4. Fold into one clean commit (`git commit --amend` on the WIP, CONTRIBUTING style; reasons for the
   size raise and for the one unsafe, which is the old `process_impl` unsafe generalised: count unchanged).
5. Early-checkpoint items the brief wants after commit 2: the pump's and R10's times before/after
   commit 1 (in the first handoff, `.k16/checkpoint-c1.md`), the trap-entry diff, what PID 1 runs
   (kmain = PID 1 thread 1, context in the header's `no_thread` area, ipc table empty).

## Commit 3 (16-bit PIDs, satp without PID): NOT STARTED

Per rulings 4-5: `Pid` = `NonZeroU16` in `libs/layout` (both sides of the handoff; loader's and
kernel's ownership tables 2 bytes a frame: `RamAllocation`, loader `alloc.rs` RPT/XPT, `init_rpt` sizing
`pages.div_ceil(PAGE_SIZE)` becomes `(pages*2).div_ceil`), `OBJECT_OWNER`/`DMA_OWNER` = 0xffff/0xfffe
and their asserts (`mem.rs` ~line 122), every `Pid::new(i as u8)` / `as u8` on a PID or index removed
(red's note b: `budget.rs` `pids()` uses `Pid::new(i as u8)`; also `ptable.rs`
`init_from_memory` `Pid::new(pid as _)`, `arch mem.rs` `get_pid`, `InitialProcess::pid()`).
satp: ASID 0 both widths; `make_satp` without PID (`libs/paging`), `satp_pid`/`pid_from_satp` and
`Process::current()`'s satp check go; the loader's handoff record (`InitialProcess`) gets a PID field
of its own (boot.md says so); `MemoryMapping::get_pid` and `ptable.rs` `get_process`'s
`mapping.get_pid() != Some(pid)` check need another source (the slot's `pid`). memory-layout.md
"`satp`" section rewritten; "PIDs fit every ASID width, because a PID is a byte" goes. Cases by name:
`pid-reuse-authority`, `uaf-lent-page`, both widths.

## Red's three notes

(a) `budget.rs` live mask `u64` -> `[u64; 4]` indexed by `tid >> 6`, bit 0 unused, equivalent assert:
OWED, it belongs with commit 5 (MAX_THREADS 255); `live_tids` iterates the words.
(b) no `as u8` on a PID or index (`pids()`): OWED, commit 3.
(c) `message.rs` `process_ending`'s `served: [Option<EndpointRef>; MAX_THREADS]` (line ~1526) on the
stack, check against 32 KiB at 255: OWED (commit 5: measure or move it; EndpointRef is 16 B -> ~4 KiB).

## Measurements in hand

- Commit 1 checkpoint: `.k16/checkpoint-c1.md` (accepted).
- rv32 sched-latency lease end: phase, accepted (decision wake p99 lands on ~18.05/28.87/40.42/51.36 ms).
- sched-timer-flood on 81b5ea38b with commits 1+4: rv64 net 491/494/491, rv32 488/490/490 (floor 450).
  sched-latency s3 on 81b5ea38b: rv64 R10 p99 5620, lease end 34058; rv32 5825, 56455.
- Sizes for commit 4: above.

## K21 (free list, mem.rs) when it merges

K21 touches `kernel/src/mem.rs` allocation (`alloc_frame` first-fit, likely `allocations`,
`alloc_page`, maybe `kernel_frame` from the top of RAM, GATE1's). Collisions to expect: commit 4's
`LoaderTable<RamAllocation>` (`allocations`/`extra_allocations` became `LoaderTable(Option<&mut [T]>)`
with Deref; `self.allocations.0 = Some(...)` in `init_from_memory`) — keep K21's logic, keep the
LoaderTable wrapper and keep any new K21 static/field all-zero so MEMORY_MANAGER stays `.bss`
(check `nm` shows `b` and `.data` stays 32 B with `.k16/sizes.sh`). Commit 2's `set_ram_start` line
and `alloc_context_page`'s doc are one-liners. Commit 3 changes `RamAllocation` to the u16 Pid
(and OBJECT_OWNER/DMA_OWNER): K21's free list must use the same type. I have not seen K21's diff.

## Order after commit 3

Brief's deliverables 5 (values + ruling 3 masks + red a, c), 6 (process-fill), 7 (thread-limit 255),
8 (worst walk, rv64 release), 9 (depth 16), then Acceptance (whole bench both widths, gate and
sched-latency at seed 3 net of audits at the new values; static footprint, per-process cost
before/after, deletions, worst-walk numbers).

## What consumed my context

Mostly the sched-timer-flood investigation on the old base (bisecting commit 4, nop padding,
disassembly; ruled moot by K20), three rebases, and reading arch process.rs, asm.rs, ptable.rs's
switch paths and budget.rs for commit 2's design.
