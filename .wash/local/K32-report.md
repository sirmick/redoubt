# K32 report: the kernel page's TCB unsafe split, checked against the ratchet

Branch wp-K32, worktree /home/mcloonan/redoubt/.worktrees/K32.
Base 58326f13c (main, SMP6 merged). Head fdcf07d50. Not pushed.

## Commits

1. 2339412cf docs/kernel: the TCB table's counts are the ratchet's again
2. fdcf07d50 doccheck: a pinned `unsafe` count is the ratchet's (C13)

## What was stale (page vs tests/unsafe-budget.toml at base)

| Row | Page said | Ratchet pins (= actual count, per rv64/unsafe-budget) |
| --- | --- | --- |
| loader | 17 | 18 |
| kernel | 44 (Sv39 13, arch 12, core 19) | 37 (11, 10, 16) |
| redoubt-sys | 1 ("the ecall itself") | 3 (unsafe trait Transport, Ecall's unsafe impl, the ecall) |
| netd (prose) | 7 | 8 |
| blkd (prose) | 4 | 4 |
| total (prose) | 74 | removed from prose |

Line counts (inclusive .rs lines, as the page defines them) were also stale and are refreshed:
loader 1,576; kernel 15,899; sys 2,464 (843 tests.rs); paging 625; layout 225; stride 2,143 (831
tests.rs); signing 149; blkd 3,410; netd 2,430; Prototyper 11,843; total without firmware 28,921
(now including the two DMA drivers, which the page counts as TCB). Still snapshots, as the status
line and residual say.

Kernel split descriptions re-derived from the source (grep of each budget's files):
- Sv39/SBI/PLIC 11: flush_here (sfence.vma), satp write, fence.i, Table::at x2, install_table,
  from_init_process (unsafe fn), PLIC, timer_sbi interrupt enables, platform/sbi console, physmap
  Window::new. (Frame zeroing no longer here.)
- RISC-V arch 10: syscall-return, set_spp, sepc write, resume_context, switch_trap, kernel_ref,
  wfi halt, sstatus.SIE window, sip.SSIP clear, sum-probe load. (Two-hart spike's three gone.)
- core 16: kframe x2, mem.rs x3 (two ownership tables, free-frame bitmap), args x3, ptable x2,
  main.rs init entry, KernelCell Sync, console x2, dma x2.

## The check (C13, tools/doccheck)

`pinned_unsafe` in tools/doccheck/src/lib.rs: every Markdown table (outside fences) with a header
cell exactly `` `unsafe` (pinned) `` is checked against tests/unsafe-budget.toml (parsed with
`toml`, already in Cargo.lock; Cargo.lock gains only doccheck's dependency line).
- `Where` column: the row's backticked paths; sum of max_unsafe over budgets whose paths ALL lie
  under one of them; a budget partly under is an error; none under is an error.
- `Ratchet budget` column: exact budget name (backticks stripped).
- A count cell is an integer or starts `not counted`; anything else is an error.
- A pinned table with neither key column is an error; an unreadable budget file is an error.
Fixtures: tests/fixtures/c13 (lines 5, 6, 7, 11, 13 each fire), good tree gains a passing table
of each kind plus a firmware-style `not counted` row. Tests: `c13_pinned_unsafe` (fires! macro),
`c13_reports_each_failure`. C4's allowance for the checker's own rule names (docs/testbench.md)
extended from C12 to C13.

Note on the brief: it said to follow "the size-budget's page check" in tools/testbench. No such
check exists at base (size.rs reads no page; doccheck has no budget code), and `make docs` runs
redoubt-doccheck's tests, not testbench's, so the check went into the docs checker as a rule with
fixtures, following C10's shape (a page held to a source file). Ceilings equal counts by the TCB
convention the page states; bench:unsafe-budget enforces count <= ceiling.

## Pages

- docs/kernel/README.md, "The TCB and its size": corrected table, new split table keyed by budget
  name, paragraph saying the counts are held by the docs checker (bench:docs) and how; status line
  adds bench:docs. No `unsafe` and no ratchet change.
- docs/testbench.md: "The unsafe budget" says a page quoting ceilings is held to them; "What it
  checks" describes C13 and lists host:redoubt-doccheck::c13_reports_each_failure (5 -> 6).

## Gates (exit codes)

- Negative: `make -f scripts/jobs.mk docs` with the kernel row set to 38: rc 1 (make 2);
  `docs/kernel/README.md:285: C13: `kernel/src`: 38, but tests/unsafe-budget.toml pins 37`.
  Log: /home/mcloonan/redoubt/.tmp/K32/docs-negative.log.
- Before the page fix, `cargo run -p redoubt-doccheck` flagged exactly loader 17/18, kernel 44/37,
  sys 1/3.
- Positive: `make -f scripts/jobs.mk docs` rc 0 (after revert, and again at head fdcf07d50).
- `cargo test -p redoubt-doccheck` (via q): rules 20 passed, docs 2 passed.
- `q run --cores 8 -- cargo test -p testbench --bin testbench`: rc 0, 187 passed.
- `make -f scripts/jobs.mk set CASES="formatting unsafe-budget"`: both PASS rc 0 (rv64; no rv32
  target). unsafe-budget's counts equal every ceiling the page quotes.
- `cargo +nightly fmt -p redoubt-doccheck -- --check`: clean.
Not run: whole bench, boots (no code on target changed).

## Affected summaries checked

- README.md, GETTING-STARTED.md, CONTRIBUTING.md, docs/README.md, docs/TOUR.md, docs/TENETS.md,
  docs/SECURITY.md, docs/plan/m2-usable-shell.md: grep for TCB line totals, `unsafe` counts,
  "C1..C12": none quote them; no change needed. TENETS and SECURITY cite the unsafe budget's
  section, whose rule is unchanged.
- docs/kernel/README.md residual risks: still true (line counts snapshots; ratchet counts words).

## Open risks

- C13 holds the page to the ceilings, not to the source counts; a ceiling left above its count
  (allowed by the ratchet, against the TCB convention) would let the page overstate. The page says
  the TCB ceilings equal their counts.
- Line counts remain snapshots (unchanged residual).
