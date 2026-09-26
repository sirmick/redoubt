# WP-K6 handoff (implementer 2 → fresh implementer)

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
| 7d-10 | - | not started |

Kernel unsafe after 7c: 46 (backends 13, arch 12, core 21). The ratchet's max values in
tests/unsafe-budget.toml are still the old ones: lower them at 7d (plan section 5).

## What was benched on which tree

- 5 (7b46d3dd3): full bench, 182 PASS; the only failures are bench-ssh-loopback x3 (host) and
  sched-latency [rv64] (fails identically on plain redoubt d6715b8f0). Log /tmp/k6-bench-5.log.
- 6, 7a, 7b, 7c: the affected cases only (listed in each message), both widths, all pass but
  sched-latency rv64. Kernel built with RUSTFLAGS="-D warnings" at each, both widths, plus
  `qemu-virt,smp,sched-trace,dma-reset-deaf` on rv64.

## 7d: next

- Plan section 8, 7d; OD7 (private S-mode switch in sched.rs, `SWITCH_TAG`, single U-mode
  decoder `redoubt::handle`, `compile_error!` without `sbi`); section 2 (Setup path keeps all
  four duties); `ppid`, exception-handler state (`SetExceptionHandler`, `begin_exception_handler`,
  `RETURN_FROM_EXCEPTION_HANDLER`, EXCEPTION_TID use), `platform_call`, syscall.rs deleted (drop
  it from tests/unsafe-budget.toml), then lower the ratchet.
- What syscall.rs still serves: SwitchTo (kmain), Yield, ReturnToParent (refused), WaitEvent,
  CreateThread, TerminateProcess, Shutdown, GetProcessId/ThreadId, JoinThread, PlatformSpecific,
  SetExceptionHandler. Check tests/programs and libs/rt use none of them first (grep for
  `redoubt_abi` in tests/programs: only map-fixed-attack's constants remain).
- `legacy-gone` (plan section 7, "New cases") lands at 7d. Run it on 7c's tree (0191ee66a) first
  and record the FAIL in 7d's message (checkout --detach, see the traps). It is also red round 2's
  P2-1 (old numbers refused with the exact error, both widths): say so in the message.
- 7d is a full-bench commit.

## Later

- 8a-8c (section 4a; OD4 PageError; libs/abi -> libs/layout). flatipc is already gone.
- 9: OD10 TIDs + `thread-limit`, renames, the cruft sweep, `no-cruft`. Known cruft for it: the
  unused `core::fmt::Write` import in tests/programs/src/bin/sleeper.rs (a warning on every
  build); the kernel's remaining `allow(dead_code)`s (services.rs, io.rs, args.rs, smp.rs,
  arch process.rs, intc_plic.rs `mask`, debug/shell.rs's `#![allow(dead_code)]`, arch mem.rs's
  two per-width ones, which want a width `cfg` instead); unused features (`stats_alloc`/
  `report-memory`, `debug-proc`, `hwsim`, `wrap-print`?, `dump-kernel-pages`?), checked against
  their `cfg(feature)` users.
- 10: docs, including docs/testbench.md:180-183 (still describes attack-checker).

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

7d, then 8a-c, 9, 10 (above). Checkpoint after each commit. Delete this file before merge.
