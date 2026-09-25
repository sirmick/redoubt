# WP-K5b handoff (2026-09-25)

For the next K5b implementer. The approved plan is
`/home/mick/.claude/plans/wash-inbox-1-message-greedy-whisper.md`. Read it first: OWNER DECISIONS,
§1-§7, Traps and Successor notes. This file records only what has happened since, and what I
found while doing it.

## State of `wp-k5b` (worktree `/home/mick/riscv/.worktrees/k5b`)

On top of redoubt `de7be4c89`, in order:
- `2e8f3d735` proc-test flake fix (bench-process-flake). The red panel accepted it.
- `63b3632bc` sched-latency bound (K5-latency-flake). Red B1 rejected it.
- `e72d63275` **K5b model** (plan §7 commit 1). Done; the full model suite passes with
  126/126 mutations caught.
- `c5884cda1` revert of `63b3632bc` (ruling on B1: the pinned 50 ms stays).
- `6d3154240` red P3-2/P3-3 on proc-test.
- This file.

Gates 1-2 and fix round 0 are reported and complete (assignments 3c304971, 8d4c4003). The
K5-latency-flake root cause is posted in its QA thread; the orchestrator decides the follow-up.
Do not depend on any sched-latency bound.

## Update 2026-09-25 (second implementer): model fix round 1, kernel WIP

**Model fix round 1** (QA K5b-code-review-1: red B1, P3-1, P3-2) is committed as a model-only
commit. It changes the following:
- B1: `Ghost::dma_reset(d, frames)` disarms only the dying holder's frames.
- There is a new mutation `K5bResetClearsCoHolderReach`. `Boot::default` gains a second healthy DMA
  device (dev 7, init handle 10), so init's new handles start at 11.
- The generator hands a child each DMA device 50% of the time. A child holding one issues
  `map_device`/`dma_alloc` on it 12% of the time. Without that, the random search never built the
  co-holder shape (measured: 0 in 4000 seeds, now about 10).
- `dma_contracts` has the co-holder case, with and without the mutation.
- P3-1 is covered in VALIDATION.md. P3-2: `ghost.armed` is removed when a no-parent frame is dropped.
- **Architect ruling, QA K5b-od6-sweep (answered):** quarantine sweeps every handle to the device;
  the kernel destroys the device object with message.rs `destroy_device`, as R10 does.
  - Copies in unreceived messages arrive as 0.
  - There is no NotPermitted for a quarantined device. The guard is
    `assert!(!quarantined)`, "I-DMA: a live device object names a quarantined device".
  - Case §6: after D2 dies, `map_device`/`dma_alloc` on the old index give BadHandle, and a handle sent
    before the quarantine arrives as 0.
  - The Architect writes the KERNEL-SPEC R10/Device text and answer 173's residual when commit 4 is
    ready; tell them then.
  - The model now sweeps in `quarantine_device`. The invariants R10 message check counts a
    quarantined device as destroyed.
- Results: all model tests pass. Mutations: all 127 caught; the K5b six at kernel_sequence seeds
  22, 171, 11, 28, 29, 677.

**Kernel: WIP commit "WIP K5b kernel: ..."** builds on rv64 and rv32 (`--features qemu-virt`), and
rv64 with `dma-reset-deaf`, and the loader on both. **No bench case has been run on it yet.** By file:
- `kernel/src/dma.rs` (new):
  - `Registry` (slots keyed by base, `runs[32]`, `doomed`, `deaf_spent`) and the window
    read/write (2 unsafe);
  - `dma_register` (boot, maps the window, classifies virtio), `dma_slot`, `dma_quarantined`,
    `dma_mapped`, `dma_holder`;
  - `dma_new_run`/`dma_drop_run`/`pool`;
  - `dma_release` (the P1-1 rule, with its assert), `dma_reset` (OD4 bound, deaf feature);
  - `dma_migrate_quarantine` (N1), `dma_holds_any`, `dma_take_doomed`.
  - `reset_epoch` from the plan is dropped (nothing reads it).
- `libs/abi/src/arch/riscv/mem.rs`: `KERNEL_DMA_REGS` (rv32 0xff7f_0000, rv64 0xffff_ffff_f400_0000)
  and `KERNEL_DMA_PAGES` = 16, with const asserts.
- `loader/src/main.rs`: `reserve_tables` for the window, so the kernel allocates **no** page table
  for it (`arch::mem::map_kernel_page` walks with no allocator and panics if a table is missing).
  That removes the boot_budgets ordering trap: `dma_register` runs inside `boot_devices`.
- `intc_plic.rs`: asserts that the PLIC ends below the window.
- `mem.rs`:
  - `DMA_OWNER` (254) and the `dma` field;
  - `alloc_contiguous(owner, n)` no longer charges; `free_contiguous`; `free_frames` is deleted;
  - `is_dma_frame`; `unmap` keeps DMA frames; `owned_mapping` accepts the holder's DMA frame;
  - pass 1 and `check_for_duplicates` skip `DMA_OWNER`.
- `device.rs`:
  - `map_device` sets `dma_mapped`, and `dma_alloc` uses `dma_new_run`/`dma_drop_run`, both through
    `dma_slot_of` (the quarantine assert);
  - `boot_devices` registers DMA devices and gives no object past 16.
- `budget.rs`: `Account::dma_mapped: u16`; `destroy_marked` calls `dma_migrate_quarantine` after
  `return_carve`.
- `services.rs`:
  - `terminate` calls `dma_release` between `release_all_memory_for_process` and `process_ended`;
  - `terminate_process`/`kill_process` then call `message::destroy_quarantined_devices`, since
    `terminate` has no `ss` (the Architect's check (2)). `shutdown` doesn't; the machine is going down;
  - legacy `lend_memory` refuses DMA frames.
- `process.rs`: `process_map` refuses DMA frames; the `drop_unstarted` debug_assert.
- `tests/unsafe-budget.toml`: dma.rs is listed; kernel core goes 25 -> 27, with the reason.
- **Held back for commit 3:** the legacy MapMemory refusal (syscall.rs) plus its helpers
  `dma::overlaps_dma_device` and `device::dma_ranges`. They are in the untracked file
  `/home/mick/riscv/.worktrees/k5b-mapmemory-refusal.patch` (a git diff plus two appended code
  blocks). They go in with the virtio-probe move, or bench-virtio-devices breaks.

**Next steps**:
1. Boot-check the WIP: run `device` rv64/rv32 (copy it to a unique toml name), a blkd case, and
   D3 net cases. Every DMA device now maps a window page at boot, and blkd/netd dma_alloc goes
   through runs.
2. Review against the model once more: pooling at terminate, and the charge uncharged only if the
   budget is live.
3. Then commit 3 (cases and programs; the pre-K5b must-fail run first, on merged K5 without the
   kernel commit), then commit 4 (docs), and tell the Architect.

**Traps (new):**
- Model generator changes shift every mutation's catching seed. Rerun the whole mutations suite,
  not only the K5b filter.
- The model's replay test lists `K5bResetClearsCoHolderReach` as invisible to replay (ghost-only).

## Commits 3-4 (unchanged from the plan)

Commit 3: boot cases and programs. Do the pre-K5b must-fail run first, then the `virtio-probe`
move in the same commit as the MapMemory refusal. D3's `[[net.dial]]`/peer machinery exists; see
the Successor notes. Commit 4: docs.

## Traps learned this session

- `./test` takes ONE filter, and matches by substring. To run a case alone, copy its toml to a
  unique untracked name (`tests/zz<name>.toml`), filter on that, and delete it afterwards.
- A failing boot case's log stops at the first forbidden match, so a panic message on the next
  line is lost. In a temporary copy, set `forbid` to something that never matches to see it.
- icount cases are NOT deterministic across boots: QEMU fills `/chosen/rng-seed` from host
  entropy, and the kernel draws PIDs from it. For exact replay, run through a PATH wrapper that
  adds `-seed N` to `qemu-system-riscv64` (experiment only).
- Model: the generator needed the setup to hand children a DMA device, or the co-holder and reuse
  paths never occur. Ghost state must be read from primary objects (the device object), never
  from the kernel's answer, or a mutation hides itself.
- Keep test runs targeted to save context: filter mutations with `REDOUBT_MODEL_MUTATIONS=K5b`,
  and run single `--test` files.
- Env: `RUSTSBI_PROTOTYPER` / `RUSTSBI_PROTOTYPER_RV32` (see the instructions). `bench-ssh-loopback*`
  fail on this host (SELinux).

## Open QA threads

- K5-latency-flake: root cause posted (boot RNG seed plus stride debt). The latency-spread look
  is already done, so the successor does not need to repeat it; await the orchestrator's ruling
  on the follow-up.
- K5b-code-review-0: round 0 fixes are in (c5884cda1, 6d3154240); waiting for the red panel's
  resolution.
- bench-process-flake: fixed by 2e8f3d735 plus 6d3154240; waiting for the orchestrator to
  resolve it.
