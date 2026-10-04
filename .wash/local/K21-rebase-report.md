# K21 rebase onto main ccf648bad (k21-implementer-3)

**Tip:** wp-k21 7381e21f8 on ccf648bad. Clean worktree. Four commits:
- 6cff2d7cb kernel: a RAM frame is taken from a free-frame bitmap (the slice)
- 72e508d27 kernel: a refused process_create gives back what it built by one walk of it
- 01676959d testbench: scan-bounds times taking frames after RAM fills, on both widths
- 7381e21f8 kernel: dma_alloc's runs come from a fixed DMA pool

## What the rebase changed
- **4333cdea2 (the budget reason-line fix) is dropped.** Main already carries the same fix,
  7d8c6e6b3 "testbench: a budget's reason line matches its name whole": the same `explains`, with
  more host tests. The only conflict was size.rs. Main's side was kept, so the commit became empty.
- **Size ceilings are +1.** Main's kernel grew by one counted line since 082e00ccf: it is 7829 at
  ccf648bad. Every K21 commit was exactly 1 over its exact-fit ceiling. The fix is folded into the
  commits that set the ceilings, and no fix-up commit was made:
  - the slice: 7943 → 7944, with its reason line "80 lines" → "81 lines"
  - scan-bounds: 7948 → 7949 (the +5 is unchanged)
  - DMA: 7983 → 7984 (the +35 is unchanged; libs/layout 61 is unchanged)
- The code is byte-identical before and after the ceiling fix: `git diff 55e1e73f7 7381e21f8` is
  that one toml line. So the QEMU runs on 55e1e73f7 stand for the tip.
- Auto-merged, with both sides kept: unsafe-budget.toml (K21's 18 with main's entries),
  budgets.md, devices.md and testbench.md. Cargo.lock had no conflict.

## Results (logs .wash/local/k21-r-*.log)
- Gate: `TESTBENCH_QEMU_SEED=13 in-dev cargo testbench kernel-containment` on 55e1e73f7, exit 0.
  Both widths PASS.

  | R10 p50/p99 µs | rv64 | rv32 |
  | --- | --- | --- |
  | rebased tip | 19194/22764 | 19855/23657 |
  | old tip 4333cdea2 (round 4) | 19194/22923 | 19857/23511 |
  | main 082e00ccf (round 4) | 18648/22383 | 18713/22524 |

  - The lease end p99 is 30677 (rv64) and 32022 (rv32), against a target of ≤ 125000.
  - Every wake gate is met.
  - Main ccf648bad itself was not re-measured. The comparison is with round 4's main.
- On 55e1e73f7, each exit 0, both widths PASS:
  - scan-bounds
  - sched-budget-churn
  - dma-destroy-quarantine, dma-reset-quarantine, dma-reset-reuse, dma-rules
  - page-table-reclaim
- On 55e1e73f7, size-budget exit 1 (kernel 7984 of 7983). Fixed as above.
- On 7381e21f8, each exit 0:
  - size-budget, at the tip and at each of the four commits
  - unsafe-budget
  - `cargo run -q -p redoubt-doccheck`
  - `cargo testbench formatting`
- Not run: the whole bench, which waits for the orchestrator's word.

## Simplifier round 3 trims (tip 6bd9d6ec0)
Both trims are folded into the slice commit c93bd23ac. The other three commits were replayed on it
unchanged, apart from their ceilings.
- `set_word` is deleted. Its one caller, the boot fill loop, writes
  `self.free.bits[self.free.level_start[level] + word]` directly.
- `find_free`'s `highest` arm is kept. Its doc now says it is only for the sched-trace build's
  `kernel_frame`. Folding it would move the frames under the trace ring, against kernel_frame's
  stated reason for taking from the top.
- LEVELS, level 0 as the table mirror and check_free_frames are untouched.
- The kernel count is 3 lower, and each ceiling follows at its exact fit:
  - slice 7941 (reason line "78 lines")
  - rollback 7939 of 7941
  - scan-bounds 7946
  - DMA 7981
- `git diff 7381e21f8 6bd9d6ec0` touches only kernel/src/mem.rs (+5/-9) and size-budget.toml.

Results on 6bd9d6ec0 (logs .wash/local/k21-t-*.log), all exit 0:
- Gate, seed 13, both widths PASS. R10 p50/p99 µs: rv64 19194/22923, rv32 19870/23628.
- Both widths PASS: scan-bounds, sched-budget-churn, the 4 dma cases, page-table-reclaim.
- PASS: size-budget (at each of the 4 commits too), unsafe-budget, formatting.
- doccheck exit 0.
