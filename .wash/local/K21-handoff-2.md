# K21 handoff 2 (k21-implementer-2 → successor)

**Tip:** wp-k21 at 4333cdea2, on main 082e00ccf. The worktree is clean.
- 2dc04ae37 — the bitmap. This is the slice commit: `FreeFrames.bits: &'static mut [u64]`, with
  the Architect's SAFETY text. The kernel core unsafe budget is 19→20 here and 18 from the next
  commit on. Size ceiling 7943. The message is already reworded: the scan and the stale-mapping
  hazard are gone, and it gives the R10 numbers.
- 6d4f2cccb — the rollback walk.
- 1120d6d25 — scan-bounds. The negative is re-recorded: map_anon 1174/374 (rv64) and 1274/474
  (rv32), FAIL; the control is 354/354 and 453/453.
- 0f1590e05 — the DMA pool, ceiling 7983.
- 4333cdea2 — testbench: the budget reason line matches the budget's whole name. This fixes the
  unsafe-budget checker for "kernel: core", as granted. Host tests a_raise_needs_* pass, and
  unsafe-budget PASSes.

**Round 4 table: complete, none empty.** Uninstrumented R10 p50/p99 µs, seed 13, lean `mark_free`:

| build | rv64 | rv32 |
| --- | --- | --- |
| main | 18648/22383 | 18713/22524 |
| slice+flush (tip) | 19194/22923 | 19857/23511 |
| slice, no flush | 19195/22924 | 19869/23661 |
| kframe+flush | 24492/28220 | 25229/28987 |
| kframe, no flush | 24492/28228 | 25220/29044 |

**The rule for the slice (Architect):** the slice stays if kframe with no flush is more than 1 ms
over main's p99. It is +5.8/+6.5 ms, so the slice stays.

**The no-flush walk:** scratch only, never committed. It was an `AtomicBool` flag in budget.rs,
set between R10_BEGIN and R10_END, that skipped `flush_tlb` (.wash/local/K21-prof-noflush.patch).
It changed nothing: the flush is not the cost, and the per-page flush stays. So the Architect's
page line for memory-layout.md ("except a destruction's walk ... needs none") is NOT to be added.
The real cost was mark_free's instruction count (the gate runs at -icount shift=3), fixed by a lean
loop. Deferral is not needed.

**Done:** on 0f1590e05, exit 0:
- the gate, both widths
- scan-bounds
- sched-budget-churn (shell 500/487)
- dma-destroy-quarantine, dma-reset-quarantine, dma-reset-reuse, dma-rules
- page-table-reclaim
- size-budget, doccheck
- the negative re-record
- commit 1's message reword

On 4333cdea2: unsafe-budget PASS.

**Remaining before the merge:**
1. Rebase onto main ccf648bad (INIT3 and FSD1 landed; K16 merges later).
2. On the rebased tip:
   - the gate, seed 13, both widths, against main
   - scan-bounds, sched-budget-churn, the dma cases, page-table-reclaim
   - size-budget, unsafe-budget, doccheck
3. The whole bench on the orchestrator's word, after RT1's.

**Detail:** .worktrees/k21/.wash/local/K21-r10-free-cost.md has sections for (c)/(a), Round 2, Round
3 and Round 4. Scratch patches in the same directory: K21-free-instrumentation.patch,
K21-bitmap-raw-instr.patch, K21-prof-tip.patch, K21-prof-noflush.patch.

**Traps:**
- Plain `cargo fmt` reformats the whole tree. Use
  `in-dev rustfmt +nightly --config skip_children=true <file>`.
- testbench takes ONE filter.
- In a checked negative, set `forbid = ['PANIC']` so the run reaches map_anon.
- rv32 has no AtomicU64.

**What consumed context:** four rebuilds of the commit stack by cherry-pick, the profiling runs, and
reading mem.rs in full once.
