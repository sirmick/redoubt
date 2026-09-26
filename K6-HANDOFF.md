# WP-K6 handoff (implementer 4 → fresh implementer)

Plan: `/home/mick/.claude/plans/wash-inbox-1-message-sorted-rocket.md` (approved, final; do not re-plan).
Branch `wp-k6`, worktree `.worktrees/k6`, base `redoubt` d6715b8f0 (K5b merged). Rebase onto
`redoubt` only.

## State per plan commit

| Plan commit | Commit | State |
| --- | --- | --- |
| 0 re-grep | (in 1's message and the first checkpoint) | done |
| 1 log endpoint | 46b83272e | done; round 1 MERGE/OK/OK |
| 2 fixture, DONE, reporter | 045efb2e4 | done; round 1 MERGE/OK/OK |
| 3a-3d, 4 | 4b284b404 .. 7ba70c996 | done; red round 2 MERGE with notes |
| 5 delete legacy cases | 7b46d3dd3 | done (WIP squashed); full bench 182 PASS |
| (handoff file) | 868b1f5ca | delete before merge |
| 6 hosted arch | 25ae0d7df | done; flatipc removed here, not 8c |
| 7a servers, messages | bc9961cfc | done |
| 7b callbacks, irq, grants | fc65020b3 | done; kernel `Grnt` refusal in main.rs init |
| 7c legacy memory, heap | 0191ee66a | done; red r2 P3-1 answered in its message |
| 7d switch, syscall.rs | d00a3c501 | done; full bench 184 PASS; round 4 (red) reviewing |
| 8a constants | ac0f4e995 | done; 35 affected cases PASS |
| 8b Error fold | 7929a361e | done (squashed, implementer 4); core unsafe 20 |
| 8c layout | f51d0f723 | done; 75 PASS |
| 9a OD10 TIDs | 6bd0e916d | done; 113 PASS (sched-latency rv64 only FAIL) |
| 9b renames | e948d0199 | done; red r4/r5 P3-1 folded |
| 9c cruft sweep | 2e19fc5e4 | done; 115 PASS; core unsafe 19 |
| 9d no-cruft gate | dc89c3bf3 | done; plants verified |
| 10a testbench.md | 5a5459247 | done |
| 10b spec/boot/layout/grants docs | 8d3512ce6 | done |
| 10c | - | remaining (below) |

Kernel unsafe after 7d: 46 (backends 13, arch 12, core 21); tests/unsafe-budget.toml lowered to
match at 7d. 8a/8b add or remove none.

## What was benched on which tree

- 5 (7b46d3dd3): full bench 182 PASS. Log /tmp/k6-bench-5.log.
- 7d (d00a3c501): full bench 184 PASS; only failures bench-ssh-loopback* (host) and
  sched-latency [rv64] (fails on plain redoubt). Log /tmp/k6-bench-7d.log. legacy-gone FAILs on
  7c's kernel (3cf9618bf), both widths, "a0 = 0x0 was not refused as unknown" (in 7d's message).
- 8a: ./test map-fixed, stub-launch, budget-syscall, proc-, write-only, blkd, netd, d3-net,
  bench-virtio, loader-rejects, unsafe-budget, legacy-gone, pid-reuse, sched-share: 35 PASS.
- 8b WIP 2 (644e3a07d): ./test mem-, map-, touch-beyond, wx, write-only, lend, move-borrowed,
  return-lent, uaf-, budget-, dma-, device, redoubt-, ipc, process, proc-, legacy-gone,
  timeouts, stub-launch, pid-reuse: 91 PASS, 0 FAIL. cargo test -p redoubt-sys passes.
- Every commit: kernel RUSTFLAGS="-D warnings" both widths (qemu-virt) and rv64
  qemu-virt,smp,sched-trace,dma-reset-deaf; loader and test-programs both widths.

## Round 3 (K6-code-review-3) fold-ins

- Done in 7d: editor P2-1 (mem.rs check_owned_range doc), editor P2-2 (arch mem.rs lend_out doc),
  red P2-1 (U-mode SwitchTo, Shutdown and the borrowed-quantum bookkeeping deleted; message says
  the residual is closed; legacy-gone has named SwitchTo (7) / Shutdown (23) rows), red P3-1
  (irq.rs `system_call`, type `!`, the one decoder). Nothing owed from round 3.
- 7d deviation (in its message): SWITCHTO_CALLER, ORIGINAL_PID/TID, restore_last_thread and
  set_last_thread are deleted, not moved (kmain always switches to the exact pid, tid).

## 8b: finish

- Squash: `git reset --soft ac0f4e995`, commit "K6 8b: ..." with a message covering:
  - `mem::PageError` {Unmapped, NonCanonical, Reserved, Lent, InUse, NoFrame, NoSpace,
    Unaligned, BadFlags}. Deviation: the plan named five; InUse, NoSpace, Unaligned and BadFlags
    carry the legacy MemoryInUse / find_virtual_address / BadAlignment / W^X refusals.
  - `services::ProcessError` {NotFound, NotReady, Page}: the process table's own refusal (not
    in the plan; services.rs is below the boundary too).
  - Infallible now: MemoryMapping::activate, Process::activate, ArchProcess::set_tid/destroy;
    ArchProcess::activate deleted; destroy_thread returns bool.
  - address_available tests the PTE directly (same answers as before, incl. non-canonical).
  - memory_range and MemoryRange gone (reserve_range/map_range return ()); its unsafe site
    goes, so recount `./test --verbose unsafe-budget` and lower "kernel: core" if it fell.
  - MemoryFlags -> redoubt_sys::MemFlags at every kernel API (redoubt_flags deleted;
    MemFlags::contains added to redoubt-sys); MMUFlags alias -> paging::PteFlags; translate_flags
    drops the swap bit P.
  - The boundary sites, each an explicit map_err, no From (for the 8b review against
    KERNEL-SPEC's Errors rows): process.rs:300, :307 (OutOfMemory), :350 (InvalidArgument);
    services.rs:378, :385 (NotPermitted); mem.rs:267 (OutOfMemory), :922 (oom), :996 (bad);
    message.rs:490 (Dead), :775, :776 (InvalidArgument), :1058 (Dead), :1225, :1229 (Refused),
    :1331 (InvalidArgument); redoubt.rs:219 (InvalidArgument); arch mem.rs:720
    (InvalidArgument). Every one was already `|_|`, so no user-visible value changes.
- Then checkpoint.

## Remaining (10c, then the end)

- VALIDATION.md: the note the plan's section 6 asks for (K6 removes kernel-only exceptions the model
  never had: the callback hold and borrowed-quantum switches; no rule or mutation changes).
- BUILD-PLAN.md WP-K6 (about line 200): record acceptance (line counts, unsafe). SWARM.md's K6
  row (about line 103) is the orchestrator's; give it the line counts: `wc -l` over kernel/src
  and libs/layout at the tip, against d6715b8f0. There is no docs/ASTRA.md (only
  docs/archive), so ASTRA needs nothing.
- Then delete this file in its own commit, and run the full bench on the tip
  (`cargo testbench`, both RUSTSBI_PROTOTYPER vars). Known failures: bench-ssh-loopback*,
  sched-latency [rv64]; uart-irq rv32 intermittent. Report.

## Notes from implementer 4

- The loader still writes `PNam`; nothing in the kernel reads it since 9c (process_name was
  the debug shell's). BOOT.md still documents it. Left alone (a loader/BOOT change); raise it
  with the orchestrator if it should go.
- 9a found that indexing Account.ipc by `tid - 1` cost about 1 ms of virtual time per process
  exit (bisected). It is TID-indexed with slot 0 unused. Don't "tidy" that back.
- no-cruft fails on a stale `[[allow]]`: removing the last Grnt mention in kernel/src/main.rs
  means removing its allow entry too.

## Owed from round 1

- red P3-1 (Sender labels) and P3-2 (TAKE_GIFTS first-caller-wins doc): **done in 3a**.
- red P3-3 (the legacy relay's pid-0 fallback): **done in 5** (serve_legacy deleted).
- red round 2 (K6-code-review-2): P2-1 at 7d (legacy-gone), P3-1 answered at 7c.
- editor: `docs/testbench.md:180-183` still describes attack-checker. Fix at commit 10.
- Round-1 review found no plants on attack cases. The red team plants in the legacy-deletion
  rounds (5, 7a-d), so keep each of those commits reviewable on its own.

## Deviations already reported (keep them)

- `rd::Gifts`/`take_gifts` landed in 2, not 1 (no unused item).
- `bench-reporter-mismatch` (must_fail self-check of the reporter rule) added in 2.
- log-server waits for its echo thread before `Listening` (uart-irq rv32 race).
- `console::init` prints a newline (the UART init leaves a byte).
- budget-mem-churn lends to its own log thread. bench-poweroff-missing timeout 3 s.
- `rd::thread(f: fn(usize), arg)`: a trampoline so thread fns may return. redoubt-filler uses
  `call_waiting`.
- mem-attack's physical-map half and irq-attack's legacy rows were dropped. They return as
  legacy-gone rows at 7d: MapMemory by address (incl. dma-rules' virtio rows and RAM),
  Claim/FreeInterrupt (incl. IRQ 0), PlatformSpecific, the heap calls, ReturnToParent, and
  the rest in §7.

## Traps

- If every `./test` fails with "No such file or directory" (main.rs:78), target/debug/testbench
  was built from another copy (a reviewer's /tmp/k6red once shared this target dir). `touch
  tools/testbench/src/main.rs` and rebuild.
- Deleting code: `-D warnings` finds orphans only where no `allow(dead_code)` hides them.

- A spawned child is a copy of its parent's image **with its statics**. In a first program
  that called `logsrv::start`, a child must not use `Logger`, because `logsrv` reads as
  started in the copy. See logsrv-badge-forgery.rs.
- `rd::log_rx()` is `first_free() - 1` only before the program creates a handle. Read it
  first (`logsrv::start` does).
- `./test` takes one filter (substring). To run one case alone, copy its toml to a unique
  untracked name and delete it after. Export RUSTSBI_PROTOTYPER(_RV32) in the same shell
  command; `export` does not persist between Bash calls.
- `git checkout redoubt` fails (it is checked out in /home/mick/riscv). Use
  `git checkout --detach <sha>`, then `git checkout wp-k6`, and commit first.
- No edits while a bench runs in this worktree (`./test` builds per case). `pgrep -af testbench`
  matches itself.
- Only log-server prints `[server] done:`; the reporter rule forbids any other such line, so
  a first program that powers off itself must not print one.

## Next steps

Finish 8b (squash + message, above), then 8c, 9, 10. Checkpoint after each commit. Delete this
file in its own commit before the final bench report.
