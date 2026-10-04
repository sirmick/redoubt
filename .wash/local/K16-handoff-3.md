# K16 handoff from k16-implementer-3 to k16-implementer-4

Worktree `.worktrees/k16`, branch `wp-k16` on main 5f9f9d61c. Scratch is `.k16/` (never staged);
the detail report is `.k16/report-c3.md`.

## Commits

- 3cc54449f: c1, walks by live thread.
- 1d7f7c221: churn ruling (shell bounds become (450,1000); scheduling.md lines).
- 388e9b9aa: c4, tables in .bss.
- 41ad6a29d: c2, contexts in the IPC page (body names main.rs and SECURITY.md).
- a4d233181: c3, 16-bit PIDs and satp ASID 0 (fpga-platform.md fix folded in).
- af98ed064: WIP c5, NOT final.

The orchestrator has read the c2+c3 report. Its answers (fpga, main.rs, SECURITY.md) are all applied.

## WIP c5 (af98ed064): state

It is a WIP commit to be re-rolled into the final c5, not a target for fix-ups. The Architect's
ruling on the data region goes to you.

Largest .bss (release; rv64 / rv32, bytes): MEMORY_MANAGER 409,984 / 405,856 (~800 B a PID);
PROCESS_TABLE 32,776 / 28,680; SCHED 20,640 / 20,632; PID_SLOTS 528 / 520. .data is 32 on both.
Image: 137,202 / 146,550. Before c5: data+bss 92,712 / 87,004.

- Done: values in libs/sys and model/src/spec.rs, and MAX_PROCESS_COUNT 512.
- Done: `TidMask([u64;4])` in arch/riscv/process.rs, used by `allocated_threads`,
  `ProcessState::Ready`/`Running`, `Account::live` and sched.rs `next_thread`.
- Done: `find_next_thread` rewritten over TidMask (one version, both widths).
- Both widths build. Nothing has been run yet.
- STOPPED on ruling 6: rv64 .data+.bss is 464,368 of 524,288 B (59,920 left, under 64 KiB);
  rv32 456,096 (68,192 left). MEMORY_MANAGER is 409,984 B on rv64.
- Question 0f43fb7d sent to the orchestrator: grow the region (Architect), or shrink.
  Wait for the ruling.
- Still owed in c5:
  - Red (c): message.rs `process_ending`'s `served: [Option<EndpointRef>; MAX_THREADS]` on the
    32 KiB stack (~4 KiB at 255): measure it or move it.
  - The cases at the new values (brief owned paths, lines 118-121).
  - The pages (brief lines 122-129).
  - Model checks and its run time (ruling 8, brief line 88).
  - The size-budget line.
  - Sizes before/after for the message (`.k16/sizes.sh`, run via in-dev).
  - Then the whole bench needs the orchestrator's word.

## Remaining (brief lines 133-166)

- 6: process-fill (new case, both widths).
- 7: thread-limit at 255 (its program's u64 TID set must widen).
- 8: worst walk on rv64 release; the numbers go into ipc.md, timer.md and budgets.md.
- 9: depth 16 (budget-test ladder).
- Acceptance: brief lines 159-166.

## K21 rebase (not yet merged; the orchestrator says when)

- arch mem.rs `allocate`: keep K21's add_context_page unwind around one header page, with
  `set_header` after the map succeeds, and `make_satp(root_phys)`.
- `release_ipc_frames` before `release_owned_frames` (K21's rename), in `terminate` and in K21's
  unwind paths.
- FreeList is all zero, so the memory manager stays .bss.
- boot_budgets comment: "saved contexts" -> "header page".

## Notes

- rv64 sched-budget-churn: shell victim ~577 is expected now (ruling).
- uaf-lent-page is rv64-only by its toml.
- Never cat .wash/qa/K16-limits.md: its checkpoint comment is a huge base64 line.
- Run benches serially in this worktree; `docker kill` orphaned in-dev containers after a
  TaskStop.

## What consumed my context

The commit-2 diff review, commit 3 twice (split and restore), and the churn bisect and ruling.
