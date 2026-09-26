# WP-K6 handoff (implementer 1 → fresh implementer)

Plan: `/home/mick/.claude/plans/wash-inbox-1-message-sorted-rocket.md` (approved, final; do not re-plan).
Branch `wp-k6`, worktree `.worktrees/k6`, base `redoubt` d6715b8f0 (K5b merged). Rebase onto
`redoubt` only.

## State per plan commit

| Plan commit | Commit | State |
| --- | --- | --- |
| 0 re-grep | (in 1's message and the first checkpoint) | done |
| 1 log endpoint | 46b83272e | done; round 1 MERGE/OK/OK |
| 2 fixture, DONE, reporter | 045efb2e4 | done; round 1 MERGE/OK/OK |
| 3a memory (+ ipc-client) | 4b284b404 | done; carries round-1 P3-1, P3-2 |
| 3b IPC (rng-test) | 4528271fe | done |
| 3c threads (`rd::thread`) | c4cff9bbe | done |
| 3d devices | affa74370 | done |
| 4 remaining callers (dma-rules) | 7ba70c996 | done |
| 5 delete legacy cases | 6f1d53bdf **WIP** | half done; see its message and below |
| 6-10 | - | not started |

## What was benched on which tree

- Commit 1 (WIP of fe60fb1bb, on wp-k5b): full bench, 177 PASS; failures ssh-loopback* (host)
  and sched-latency (the K5b R10 regression, fixed since).
- Commit 2 on redoubt, tip 045efb2e4: full bench **finished before any 3a edit**, 183 PASS;
  failures ssh-loopback* and sched-latency [rv64] only. sched-latency rv64 fails identically on
  unmodified redoubt d6715b8f0 ("steward decision wake" p99 ~75-110 ms vs 50), so it is not
  K6's; reported to the orchestrator.
- 3a, 3b, 3c, 3d, 4: only the affected cases (`./test <filter>`), all pass on both widths.
  No full bench since 045efb2e4. None was run while another bench ran.
- WIP 5: builds only (test-programs, loader, testbench, both widths). Not run.

## Commit 5: still missing

1. `tests/loader-rejects-grants.toml`: `programs = ["log-server"]`, `[[file]] name = "grants"`
   (any `from`, e.g. `tests/data/bundle-file.txt`), `allow_panic = true`, expect
   `'^loader PANIC'` and the loader's message ("holds a `grants` entry"), forbid `KMAIN`. The
   testbench no longer reserves the name `grants`.
2. `pid-reuse-authority` program and case (plan §7, "New cases"): rv64 and rv32, first and
   alone.
3. Reword the remaining "legacy" comments: budget-mem-churn.rs:4 and proc-test.rs:12.
   map-fixed-attack's `LEGACY_STACK_SIZE` and its `redoubt_abi::arch` consts go at 8a (OD9).
4. rustfmt the touched files. `tools/testbench` is **not** rustfmt-clean, so do not rustfmt it
   (only hand-format the lines you add). In tests/programs, format only files you rewrote,
   and check the diff for churn.
5. Full bench (commit 5 is a full-bench commit), then squash into "K6 5: ...".

## Owed from round 1

- red P3-1 (Sender labels) and P3-2 (TAKE_GIFTS first-caller-wins doc): **done in 3a**.
- red P3-3 (the legacy relay's pid-0 fallback): **done in WIP 5** (serve_legacy deleted).
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

Finish 5 (above), then 6 (hosted arch), 7a-d (kernel deletions, ratchet edits per §5;
legacy-gone lands at 7d with 7c's recorded failing run), 8a-c (one definition each,
`PageError`, libs/abi → libs/layout), 9 (OD10 TIDs, `thread-limit`, renames, cruft sweep,
`no-cruft`), 10 (docs, including testbench.md:180-183). Checkpoint after each commit.
