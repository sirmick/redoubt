# K16: the kernel's tables grow: processes, threads and the per-object caps

Tier A (kernel and loader), size L. Design: QA `K16-limits` (`.wash/qa/K16-limits.md`); the
owner chose every value below on that thread, and the rulings are the Architect's. Needs GATE1,
INIT1 and K13, all merged: K13 writes `kernel/src/message.rs` (every walk this package changes is
there), INIT1 writes `budget.rs`'s `boot_*`, `setup_loader_process`, the loader's handoff and its
mapping of the first processes, and boot.md's program count, and GATE1's numbers are the baseline
this package is measured against. Rebase on main after the last of them merges.

Every cargo and bench command on this host runs inside the dev container:
`/home/mcloonan/redoubt/.wash/local/in-dev <command>` from the worktree.

## The values (the owner's)

| Constant | Now | New | Notes |
| --- | --- | --- | --- |
| `MAX_PROCESS_COUNT` | 64 | 512 | The PIDs there are, the kernel's included: PIDs 2..=512. 16-bit PIDs (ruling 4). |
| `MAX_THREADS` | 31 | 255 | Each thread's saved registers are allocated with the thread (ruling 1). A TID is a byte; TID masks are 256 bits, `[u64; 4]`, bit 0 unused. |
| `MAX_OPEN_CALLS` | 64 | 256 | The thread page holds 41 + 256 words of calls plus 32 of saved registers: 329 of 512. A process's lends cost it at most 256 x 16 = 4,096 pages. |
| `WAIT_CAP` | 16 | 32 | R2's restamp is one write per queued message of the group: at most 32. |
| `MAX_DEPTH` | 8 | 16 | `for_each_descendant_post` recurses once a level. |
| `MAX_LABELS` | 8 | 16 | ABI records grow: `RECEIVED_SLOTS` 24 -> 32, `BUDGET_SPEC_SLOTS` 14 -> 22; the open-call page, the process object and the endpoint each hold 8 more words. |
| `MAX_START_HANDLES` | 64 | 128 | `process_start` decodes the list on the kernel stack (ruling 6). |

## Rulings (the Architect's, from tenet 1 and the pages)

1. **A thread's saved registers live in its IPC page.** A thread already costs its budget one
   page, its IPC page (processes.md: "A thread costs its budget one page"; objects.md's cost
   table), and that page has room: 297 words of IPC state today at 256 open calls, plus 32 for
   the registers. So a thread's context costs no page of its own, and a process pays for the
   threads it has, not for `MAX_THREADS`.
   - `PROCESS_AREA` becomes one page on both widths (`THREAD_CONTEXT_PAGES` = 1, or the constant
     goes): the header (the trap's scratch, the running thread's context *address* in slot 1
     where its number is today, `ProcessInner`, the TID masks), one context-sized area for "no
     thread" (what context 0 is today), and the TID -> IPC-frame table (256 x `u32`, 1 KiB).
     Assert it fits.
   - The trap entry loads slot 1 as the context's address (the IPC page's physmap address plus
     the registers' offset) instead of shifting a number: one instruction fewer. `CTX_SHIFT`
     goes. Switching thread writes the address.
   - `Account::ipc` (`[u32; MAX_THREADS + 1]`, static) goes: `Account` keeps the frame of the
     process's header page, and the kernel reads another process's table and masks through the
     physmap. Static memory then does not grow with `MAX_THREADS` at all.
   - PID 1 (the kernel) has no budget and no IPC pages. If it ever runs a context, it is the
     header's "no thread" area; say at the checkpoint what you found.
   - objects.md's cost table loses the "saved thread contexts" row (its header page is one page
     on both widths, charged where the process runs); "Accounting on both widths" loses the
     width-dependent row. Every case whose expected charges count 2 context pages on rv64
     changes.
2. **Every walk visits only processes that exist and threads that have an IPC page.**
   `MAX_PROCESS_COUNT` x `MAX_THREADS` goes from 1,984 slots to 512 x 255 = 130,560 (66x). A
   pump walks every slot today, about 1.2 ms at the gate's full fill; walked by slot, that is
   about 80 ms. The walks (`find_thread`, `fail_all`'s pass, `next_timeout`, `process_ending`,
   `poke_receivers`, the R2 sender pick, `handle.rs`'s per-PID sweeps, `process_ended`'s loop)
   skip a PID with no account and iterate the set bits of the live-TID mask. The cost then
   follows live threads, which pages pay for.
3. **Masks and widths.** `ProcessImpl::allocated_threads` (`u32`), `ProcessState::Ready` and
   `Running` (`usize`), every `1 << tid` and `sched.rs`'s `[bool; MAX_THREADS + 1]` become
   `[u64; 4]` masks or iterate them. `last_tid_allocated` stays a `u8`. Checked unaffected: the
   28-bit frame index in a handle slot, `Account::dma_mapped` (DMA slots), the PID in an exit
   notice (`u32`), a handle chain's link word (`pid << 32`).
4. **The PID is 16 bits** (the owner's decision). `Pid` in `libs/layout` becomes `NonZeroU16`,
   so the loader's and the kernel's ownership tables are 2 bytes a frame (one type, both sides).
   `OBJECT_OWNER` and `DMA_OWNER` become `0xffff` and `0xfffe`, and their asserts follow. Every
   `Pid::new(i as u8)` goes.
5. **`satp` no longer carries the PID.** Sv32's ASID is 9 bits, so PID 512 does not fit, and
   nothing needs it there: every switch, map and unmap already flushes the whole TLB
   (memory-layout.md, "`satp`"). The ASID is 0 on both widths. The kernel's `current_pid` is the
   one record of the running PID. `satp_pid`, `pid_from_satp` and the satp-against-current check
   go, and the loader's handoff record names each first process's PID in a field of its own.
   memory-layout.md's "`satp`" section says so, and "PIDs fit every ASID width, because a PID is
   a byte" goes. ASIDs for several harts are M2's to design.
6. **Static tables, in RAM, not in the image.** No table becomes frames charged to a budget.
   Measured on main at N = 253 (release): about 850 B a PID, 640 of it `Account::handles`, 128
   `Account::ipc` (gone by ruling 1). At 512 that is about 375 KB a width.
   - A table sized by a limit costs RAM, never image: the per-PID tables' empty value is all
     zeros, so they are `.bss`, and `.data` and the image stay about where they are today
     (rv32 image 217,806 B of `link.x`'s 512 KiB `FLASH`).
   - The data region is 512 KiB on both widths (`link.x`, `link64.x`). If `.data` + `.bss` leave
     less than 64 KiB of it, stop and report: the Architect grows the region on memory-layout.md
     (room up to `0xfff0_0000` on rv32).
   - No array sized by `MAX_PROCESS_COUNT` or `MAX_START_HANDLES` is duplicated on the 32 KiB
     kernel stack. `sched.rs`'s `runnable` (`[BudgetRef; N]` by value, 8 KiB at 512) becomes an
     iterator or a static. `process_start`'s `decoded` and `handles` arrays, 128 each, become one.
   - Report `.data`, `.bss` and the image on both widths, before and after.
7. **The stride queue's per-pick scan** (`libs/stride`, `slots: [Option<B>; N]`) grows to 512.
   `sched-latency` and the `sched-*` cases measure it; a missed target is reported, not
   absorbed.
8. **The model moves with the kernel.** `model/src/spec.rs` holds its own copies of the
   constants and `RECEIVED_SLOTS`; they change in the same commit. Report the model's run time
   if the flood checks grow with `WAIT_CAP`, `MAX_OPEN_CALLS` or `MAX_THREADS`.
9. **Nothing is written on a page before the code.** Today's limits are current behaviour. Every
   page below changes in the commit that changes what it states.

## Reading list, in order

1. This brief and `.wash/qa/K16-limits.md`.
2. `kernel/src/arch/riscv/asm.rs` `_start_trap`; `kernel/src/arch/riscv/process.rs`
   (`ProcessImpl`, `PidSlots`); `kernel/src/message.rs` "Walking the threads" and the thread
   page's word layout; `kernel/src/budget.rs` `Account`, `thread_created`, `process_ended`;
   `kernel/src/ptable.rs` `ProcessState`; `libs/sys/src/lib.rs` (the constants).
3. `docs/kernel/memory-layout.md` "Per-process kernel data" and "`satp`";
   `docs/kernel/objects.md` "What objects cost"; `docs/kernel/scheduling.md` "Responsiveness"
   targets and the R12 paragraph on fixed terms; `docs/kernel/budgets.md` "Residual risks".

## Owned paths

- `libs/sys/src/lib.rs` and `record.rs` (the constants, the slot counts), `libs/layout`
  (`Pid`, the process area), `libs/paging` (`make_satp`, `satp_pid`), `libs/stride` only if
  ruling 7's measurement asks.
- `kernel/src/`: `arch/riscv/` (`asm.rs`, `process.rs`, `mem.rs`), `ptable.rs`, `sched.rs`,
  `mem.rs` (owner PIDs, `RamAllocation`), `budget.rs` (`Account`; `boot_*` only where the
  constants reach), `message.rs` (the walks, the thread page layout), `handle.rs` (the per-PID
  sweeps), `process.rs` (`process_start`'s arrays, the PID draw).
- The loader: its ownership table's type, the handoff record's PID field, its process-area
  mapping.
- `model/src/spec.rs` and the model checks sized by the constants; `libs/rt/fake` if it mirrors
  them.
- Tests: `thread-limit` and its program (its `u64` TID set cannot hold 255), `process.toml`,
  `budget-carve-attack` (system's process count), `budget-test` (the depth ladder),
  `redoubt-ipc` and `redoubt-filler` (`MAX_OPEN_CALLS`), the `Busy` cases (`WAIT_CAP`), every
  case whose expected page charges include saved contexts. New cases below.
- Docs: `kernel/memory-layout.md` ("Per-process kernel data", "`satp`", the per-width table),
  `kernel/boot.md` (the handoff record), `kernel/processes.md`, `objects.md`, `abi.md`,
  `budgets.md`, `ipc.md` (R2, R4a, the delivery-walk residual), `invariants.md` (4,096 lent
  pages), `timer.md` (the expiry walk), `scheduling.md` (PID-draw counts), `model.md`,
  `kernel/README.md`, `plan/m1-separation.md` (the `WAIT_CAP` lines only if they state 16),
  `servers/serving.md`, `servers/init.md`, `userland/native.md`, `userland/sessions.md`,
  `userland/beamlet.md` (31 threads; "the rest, twenty-two" -> 246), `todo/kernel-attack-gaps.md`
  (the 63-program line, if INIT1 left it).

Anything else is a question to the orchestrator first.

## Deliverables, as commits in this order

1. **Walks by live thread**, values unchanged (ruling 2, with a 64-bit live mask for now).
   Remeasure at the gate's full fill on both widths (`kernel-containment`,
   `endpoint-destroy-full`, `sched-latency`, seed 3): no target may regress against GATE1's
   numbers on budgets.md and scheduling.md. Report the pump's time before and after.
2. **Contexts in the IPC page** (ruling 1), values unchanged. Whole bench on both widths.
3. **16-bit PIDs and a `satp` without them** (rulings 4 and 5), values unchanged. Whole bench;
   `pid-reuse-authority` and `uaf-lent-page` (the `satp` section's cases) by name.
4. **Tables in `.bss`, nothing N-sized twice on the stack** (ruling 6), values unchanged.
5. **The values**, with ruling 3's masks, the page deltas and the case updates.
6. **`process-fill`** (new, both widths, checked build): processes started under budgets carved
   for it until PID 512 is in use and every PID 2..=512 has been drawn; the next
   `process_create` is refused by the budget (`OutOfProcesses`), never by a panic; all end and
   every page comes back. The verdict is the kernel's results and `budget_usage`.
7. **`thread-limit` at 255**: one process holds 255 threads with TIDs 1..=255, and the 256th is
   `TooManyThreads`; its budget is charged exactly one page per thread plus the header page,
   and all of it comes back. Both widths, checked build.
8. **The worst walk, measured** (new, rv64 release, `memory_mib` as needed): as many live
   threads as the guest's RAM holds across all PIDs (report the count), then one receive's
   pump, one timer expiry and one destruction, timed from the trace. The numbers go into ipc.md's
   and timer.md's walk residuals and budgets.md's measured list as stated residuals, not
   targets; one past R10's 30 ms is a stop-and-report.
9. **Depth 16**: `budget-test`'s ladder reaches depth 15 and is refused at 16, and destroys from
   its top so `for_each_descendant_post` recurses the full depth on the checked build.

## Acceptance

1. Each case above by name on both widths, then `cargo testbench` whole (rv32, the unsafe
   ratchet, the size budget, the docs checker).
2. The gate (`kernel-containment`) and `sched-latency` meet every target at seed 3 on both
   widths, net of audits, at the new values.
3. The report states the kernel's static footprint and image on both widths, the per-process
   cost before and after, what was deleted, and the worst-walk numbers.

## Early checkpoint

Stop and report after commit 2: the pump's and R10's times before and after commit 1, both
widths; the trap entry's diff; what PID 1 runs.
