# Architect handoff (architect-10, 2026-10-03)

Working rules: architect-7-handoff.md's first lines. Watch lists: `architect-10-notes.md` (and
architect-9-notes.md's, which stand).

## Rulings and their files

- SMT FPGA core: the shared TLB is tagged by hart (the privileged spec); the barrel stays strict
  for R12; pause in every spin; ASIDs 16 bits (owner). On main: f913630c5, 35e4d41ef. Briefs:
  SMP1-multihart.md, SMP1-implementer.md (items 4, 7), SMP2-implementer.md (hart time).
- K16 c8: rv64 checked build with walk-trace, net of audits; pages say "(rv64, checked build, net
  of its audits, N live threads across 511 processes)".
- FSD3 Q1 (b): a volume's range makes fsd a user of blkd, so one disk per label set. Q2 (a):
  `endpoint=NAME`, required. Lines in the answer (notes). No Q3 reached me.
- INIT_PAGES 2,048 with strip (BEAM1), budgets.md lines sent. INIT5 (`INIT5-implementer.md`):
  place in batches of 64, no lend, INIT_PAGES back to 1,024. Needs BEAM1.
- BEAM1 thread::spawn: one unsafe accepted (rt 11), with a SAFETY comment and a keeper.
- BEAM1 heap flood: `BEAM1-heap-flood-ruling.md` (the limits from the budget; two cases).
- beamlet-redoubt cut: BEAM1-5 (plan); `BEAM1-implementer.md`.
- (INIT4's buckets and K21 were architect-9's.)

## Merge checks

Done:
- B7: the commit message fixed.
- FSD2 rule 5: the cost finding, deepest-first pass.
- INIT4: A and B (`INIT4-merge-check.md`), and the init step closed on main (06be0c16e).
- K16 round 1 at 19102cb9b: two edits (memory-layout satp, fpga ASID bullet).
- RT2 at a48e430a7: two serving.md edits. Its final tip is page-only.

Owed:
- K16 round 2: commits 6-9, c8's numbers with their conditions line, thread-limit 255, depth 16.
- BEAM1 at its final tip: both widths (rv32 builds, 799 pages, so drop "rv64 only"); the
  heap-flood split; budgets.md's bound sentence for both widths; the testbench workspace line;
  the unsafe line; the timer-frequency Open.
- FSD3 at its final tip: Q1 and Q2 as applied (init.md confinement, fsd.md Arguments,
  init.md:118, image manifest); mkimage packing through fsd's code.

## Open with you

- K19: with the owner.
- The h/1 docs choice: with the owner (recommended: strip, ship as an optional package).
- SMP2's model checkpoint.
- INIT5: after BEAM1 merges.
- BEAM2's brief: after BEAM1 and FSD3. The userland disk (decided 5) and the shell on the UART.
  Book load_module's row, packages.md, bootfsd.md and a SECURITY.md row; close beamlet.md's
  Open on where the modules live.
- BEAM3-5 briefs, later.

## What consumed my context

Reading beamlet.md and shell-plan.md to cut the step (about 400 lines), the merge diffs, and
long message bodies over the 2000-byte limit. Use files for the detail.
