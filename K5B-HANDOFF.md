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

## Next: plan §7 commit 2, the kernel

The model is the reference. Mirror its semantics (`model/src/kernel.rs`: `dma_reach`,
`dma_release`, `reset_device`, `quarantine_device`, the OD2 checks, and `destroy_budget`'s
quarantine migration after the carve). Kernel facts gathered so far, all read on this tree:

- `kernel/src/device.rs:325` `map_device` → `map_run(pid, len/PAGE, R|W, Some(base))`. `:335`
  `dma_alloc` → `alloc_contiguous(pid, npages)` (mem.rs:329; it charges through `charge_frame`,
  which puts the frames in `account.frames`: the plan's Trap). Then `map_run(.., Some(phys))`, and
  `free_frames` on unwind. For K5b the frames must be owned by `DMA_OWNER` and charged to the run's
  budget directly, never through `account.frames`, or `uncharge_all_frames` would uncharge them
  before the reset.
- `kernel/src/mem.rs`:
  - `OBJECT_OWNER` is PID 255 (`mem.rs:94`), with a const assert that `MAX_PROCESS_COUNT` < 255.
    Add `DMA_OWNER` (254) beside it, the same way.
  - `release_all_memory_for_process` (`mem.rs:930`): pass 1 reparents lent frames to the kernel,
    then `release_owned_frames(pid)` frees `allocations[idx] == Some(pid)` and calls
    `uncharge_all_frames`. Frames owned by `DMA_OWNER` are untouched by construction; still check
    pass 1's `for_each_lent_frame`.
  - `check_for_duplicates` (`mem.rs:1020`) has to skip `DMA_OWNER`.
  - `unmap` (`mem.rs:1215`) releases any RAM frame through `release_page`. A DMA frame must only
    lose its PTE.
  - `owned_mapping` (`mem.rs:1321`) requires `allocations[..] == Some(pid)`. It must also accept a
    DMA frame whose Live run is held by `pid`, so that `set_flags` and `unmap` work.
- Legacy lend is `services.rs:1549/1638` `lend_memory`, and `process_map` is `process.rs:330`: both
  must refuse DMA frames. The legacy `MapMemory` refusal of any range overlapping a DMA-flagged
  `Devs` entry goes in `syscall.rs:855` (plan §2 P3 a: page-rounded, half-open, before anything is
  claimed).
- The terminate hook: `services.rs:276` `Process::terminate`. Call `mm.dma_release(pid)` straight
  after `release_all_memory_for_process` and before `process_ended`, in the same MM closure.
- N1: `budget.rs:816` `destroy_marked`. Migrate the quarantine charges AFTER
  `self.return_carve(top, false)` (line 830) and before the frames are freed.
- Boot: `dma::boot` goes after PLIC init and before `boot_budgets` (Trap: page tables before
  root's limit is sized). Use `arch::mem::map_page_inner` with PID 1. The window constant goes in
  `libs/abi/src/arch/riscv/mem.rs` (plan §2 gives the addresses and the asserts).
- The QEMU virt DMA devices are the virtio-mmio slots 0x10001000..0x10008000 (see the `Devs` tag
  in any boot log). STATUS is at +0x70. The magic 0x74726976 is at +0x000, and the version at
  +0x004.
- Feature `dma-reset-deaf` (OD7): the *first* attempt per device reports "not confirmed", after the
  real write of 0.

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
