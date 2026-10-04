# K14 progress

## Early checkpoint: deliverables 1 and 2 (commit 7830101d5 on wp-k14)

### The fix (ordering, not undo)
`kernel/src/process.rs` `process_map`:
- Source check: a page whose PTE is an untouched reservation (`virt_to_phys` -> `PageError::Reserved`)
  is counted (`untouched`), not backed; every other page still goes through `owned_mapping` + RAM + not-DMA.
- Right after the source loop: if the caller's budget has fewer free pages than `untouched`,
  `InvalidArgument`. This is what the old `ensure_range_exists(..).map_err(|_| InvalidArgument)`
  returned at the same position, so the error and its place in abi.md's order are unchanged.
- Stage 4: `charged = if shared { untouched } else { pages }`; refuse if `tables + charged > free(child budget)`.
  When shared this equals the old check after backing (`tables <= free - untouched`).
- `mm.ensure_range_exists(src, len).expect(..)` moved after stage 4 (nothing can refuse from there);
  `free_before` for the tables debug_assert is read after it.
- No arch change; `ensure_range_exists` unchanged. No new `unsafe`.

### The case: `process-map-untouched-attack` (rv64 + rv32)
Program `tests/programs/src/bin/process-map-untouched.rs`, sole first boot program holding UART
and Reset (map-fixed-attack's pattern; verdicts are `budget_usage` results from the kernel).
- Taken destination: child page at DST, then process_map of 2 untouched stack pages onto DST:
  `InvalidArgument`, (system usage, child usage) unchanged.
- Child budget cannot pay: smallest page limit process_create accepts (found by trying 1..64),
  free < 2 asserted; process_map of 2 other untouched pages: `OutOfMemory`, both usages unchanged.
  Separate sources so the second refusal cannot pass on pages the first backed (old kernel).
- Success: the first source, still untouched, moves to DST+PAGE: system usage unchanged, child +2.

### Commands (all through .wash/local/in-dev, in the worktree)
- `cargo testbench --arch rv64 process-map-untouched-attack` -> exit 0 (PASS)
- Same, kernel/src/process.rs reverted to main -> exit 1:
  `[process-map] FAIL: nothing charged` (the case catches the bug). Fix re-applied.
- `cargo testbench process-map-untouched-attack` -> 0 (rv64, rv32 PASS)
- `cargo testbench map-fixed` -> 0 (map-fixed-attack both, map-fixed-tables, -rv32)
- `cargo testbench process-attack` -> 0; `cargo testbench stub-launch` -> 0
- `cargo +nightly fmt --all --check` -> 0

### Docs done in the commit
memory.md: "The mapping calls" paragraph states a refused call charges nothing and how
process_map gets there; Failure and restart status names the case, its bullet says a refused
process_map charges neither child nor caller; the residual item is gone.

### Open question (not blocking yet): pages outside my owned paths
The fix makes these lines stale; they are not in my owned paths:
- docs/kernel/abi.md:299 (`process_map` row: "untouched source pages are backed first";
  OutOfMemory's "the pages unless the budget is the caller's" now also counts untouched pages
  when shared).
- docs/kernel/abi.md:367-370 residual "An error may leave a page backed": drop "or a
  `process_map` source" (lend/transfer part stays; message.rs is not mine).
- docs/plan/m1-separation.md:89-90 links both todo pages I must delete; doccheck will fail on
  the dead links.
- docs/kernel/processes.md:135 "(a reserved page is backed first)" stays true; no change planned.
Proposal: I make those three minimal edits in the same commits. Waiting on the orchestrator's yes.

## Final (branch wp-k14 at 4516a0288, base cc6b697f2, two commits)

- 5d192739c kernel: a refused process_map leaves its untouched source unbacked
  (process.rs, mem.rs map_fixed comment, the case, memory.md, abi.md :299 and :367,
  SUMMARY line, m1-separation link, todo page deleted)
- 4516a0288 kernel: a boot process's stack is reserved once, by the loader
  (arch process.rs: reservation and DEFAULT_STACK_SIZE gone; mem.rs: reserve_range gone,
  two comments; arch mem.rs: reserve_address, unreserve_address gone, is_occupied comment;
  the case; memory-layout.md status and residual; SUMMARY; m1-separation; todo page deleted)
Kernel: +30 -87. Loader unchanged (its 32-page reservation was already the right one).
map-fixed-attack unchanged: its rv32 LAST_FREE comment gives USER_AREA_END (the stack ends
there on Sv32) as the reason, not the extra page.

### boot-stack-reservation (rv64 + rv32)
Sole first boot program: map_fixed(0x8000_0000 - 32 pages) -> InvalidArgument, system usage
unchanged; map_fixed(one page lower) -> Ok, usage up; unmap -> baseline. Old kernel (rv64):
exit 1, "[boot-stack] FAIL: the page below the stack is free".

### Gates (all via in-dev)
- testbench boot-stack-reservation 0; process-map-untouched-attack 0 (both widths, also at
  the first commit alone); map-fixed 0; process-attack 0; stub-launch 0
- fmt --all --check 0; doccheck 0 (also at the first commit); ./build --arch rv32 --programs 0
  (only warning: tests/programs kernel-half-attack.rs:65 unused mut, not mine)
- unsafe: kernel/src 46 before, 46 after; bench unsafe-budget PASS, size-budget PASS,
  formatting PASS.
- Whole bench `cargo testbench`: exit 1, 254 PASS, 16 FAIL:
  - 7 bench-ssh-loopback* and 7 sshd-loopback*: "bench error: No such file or directory" /
    podman not installed: host.
  - vendor-check: host test of a vendored crate fails on an elided-lifetime lint
    (`iter_mut(&mut self) -> IterMut<K, V>`); nothing in vendor/ is touched.
  - process-review [rv64]: see below.

### process-review rv64: intermittent, timing
Failure: proc-review.rs:105, the zero_entries child's notice is Faulted, not Exited(43).
Console: "Instruction page fault of 0x0 at 0x0", Thread 2 PC 0: one of the two threads the
child creates with entry 0 was scheduled before its process_exit(43). The test assumes no
preemption in that window ("Neither allocation yields"); a timer tick there faults it.
Runs, rv64, process-review alone:
- main kernel: 0/18
- this branch: 3/12 (the bench run, then 1 of 3 and 1 of 8 alone)
- process_map change alone: 0/12
- boot-stack change alone: 1/12
I find no functional path from either change to that child (a launched process: no
reservations, sources map_anon-backed). Boot no longer reserves 33 pages per boot process,
which shifts timing; that is my best explanation, not proven. proc-review is not mine.

### Next
Deliverables 3-5 (done, above): drop the kernel's second stack reservation (`setup_loader_process`,
`DEFAULT_STACK_SIZE`, then `MemoryManager::reserve_range`, which has no other caller), a
boot-stack case on both widths, the rv32 `LAST_FREE` comment in map-fixed-attack (it names
USER_AREA_END as its reason, not the extra page: to check), memory-layout.md, todo pages,
SUMMARY.md, then the whole bench.

## The process-review dig (no code changed; branch still 4516a0288)

Fault, from both saved failing consoles (branch, and boot-stack change alone), identical:
- `InstructionPageFault(0, 0)`: "Instruction page fault of 0x00000000 at 0x00000000",
  a fetch, not a load/store.
- PID 60 / PID 40 (random PIDs): the child `spawn` launched for `zero_entries`, not a boot
  process. Thread 2 (the first `thread_create(0, ...)`): PC 0, SP 0x6000_3ff0, RA EXIT_THREAD.
- SP is `stack + 4*PAGE - 16`, which `zero_entries` sets from its own
  `map_anon(4 pages)` = 0x6000_0000..0x6000_4000, mapped in the dump. The child's
  memory map has no page near 0x7FFD_F000 or below 0x7FFE_0000: a launched process has
  no loader stack, and the deleted reservation was only ever made for loader processes.
- So the fault is not a stack past the reservation. The entry-0 thread was scheduled before
  the child's `process_exit(43)`: the test assumes no preemption in that window, and this
  case has no `icount` (tools/testbench/src/case.rs), so the timer phase is host time.

Counts, process-review rv64 alone:
- old dev image: main 0/18; branch 3/12; process_map change alone 0/12; boot-stack alone 1/12
- new dev image, serially after the bench: branch 0/20, main 0/20

## Whole bench, new image: `cargo testbench --allow-skip`
exit 0; 269 PASS; 1 SKIP bench-ssh-loopback-openssh (podman); process-review PASS rv64, rv32.

## Fix round 1 (tips 30fcbf072, 7a380a31d on cc6b697f2)

- process-map-untouched-attack gets a third refusal: the caller's own budget cannot back the
  source (process.rs's early check) -> InvalidArgument, system and child usage unchanged.
  Only a loader-started process holds a reservation, so the caller is the case's own boot
  program, its `system` budget filled with map_fixed at FILL to <= 1 free page, then unmapped.
  With the check disabled (rv64): exit 1, kernel PANIC at the `ensure_range_exists` expect.
  Named on abi.md's status line; commit 1's message names all three refusals.
- process.rs: the early check's comment says why it and stage 4 are both needed.
- Commands (in-dev): testbench process-map-untouched-attack 0 (rv64, rv32);
  testbench boot-stack-reservation 0 (rv64, rv32); fmt --all --check 0; doccheck 0;
  testbench --allow-skip: exit 1, 268 PASS, 1 SKIP (bench-ssh-loopback-openssh),
  1 FAIL touch-beyond-ram [rv32]: the survivor reported (and log-server powered off) before
  the attacker's "refused after N pages" line. The survivor waits a fixed 300 ms of wall time
  (touch-beyond-ram-survivor.rs:15); the attacker zeroes 32 MiB of rv32 frames, 1.3 s for the
  case under the bench's load. It passed in the previous whole bench; alone on the branch
  rv32: 0/10 failed (0.5-1.5 s). Neither commit touches map_anon or the fault path.
