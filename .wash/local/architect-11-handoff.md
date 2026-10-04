# Architect handoff (architect-11, 2026-10-03)

Working rules: architect-7-handoff.md's first lines. Watch lists: `architect-11-notes.md` (and
architect-10's and 9's, which stand).

## Rulings and their files

- FSD3: Q3 (testbench packs the disk through fsd's pack) needed no ruling. Case 3 split into
  fsd-confined-labelled + fsd-label-check (`FSD3-label-check-ruling.md`). Confined users: a shared
  server's users are the principal domains with its own label set (`FSD3-confined-users-ruling.md`).
  Sharing::Server is kept and tested with a device-less blkd, or deleted with init.md lines (notes).
  Restart: the mount walk runs at every start; fsd.md:415 text and fsd-restart line (notes).
- BEAM1: heap/ETS limits at budget/16, budget >= 2x own use, budget_pages required
  (`BEAM1-heap-limit-ruling.md`, superseding part (1) of `BEAM1-heap-flood-ruling.md`). Restart
  loop: (a), wording only (notes). thread::spawn and INIT_PAGES were architect-10's.
- K16 c8: the pump walks every thread (`K16-pump-ahead.md`). ASID bound: MAX_PROCESS_COUNT < 2^ASID_BITS, value 511
  (`K16-asid-bound-ruling.md`).
- Cut: IPC3 (`IPC3-implementer.md`), K22 (`K22-implementer.md`), HW1 (`HW1-implementer.md`, sweep
  `hardware-bounds.md`). SMP1 needs IPC3 and K22; SMP1-implementer.md gained the hart bounds and
  hotspot lines.

## Merge checks

Done: BEAM1 at 69e335a45 (two edits, applied; merged c2283a825). RT2 was architect-10's.
Owed:
- K16 round 2 at K16-7's final tip: c6-9; c8's residual lines state the numbers with their
  conditions and name IPC3 and K22; the ASID lines (511 everywhere, the assert, memory-layout
  satp, processes.md:44); thread-limit 255; depth 16.
- FSD3's final tip: Q1/Q2 as applied, the packing through fsd's pack, the label-check cases and
  status lines, the confined-users line and test, Sharing::Server's outcome, red P1's fix (no
  handed badge at a blkd's endpoint; init.md Volumes bullet line), fsd.md:415, fsd-restart.
- INIT5 (brief updated for BEAM1's sentence and beamlet-boot). Later: BEAM2, IPC3, K22, HW1.

## With the owner

- K16: merge with the numbers as residuals, IPC3 and K22 behind it (the lower limit is not real:
  about 500 live threads system-wide).
- The h/1 docs choice (recommended: strip, docs as an optional package; BEAM2 has the switch).
- An ASID package after SMP1: on one hart, satp's ASID = PID, global (G) kernel mappings,
  selective sfence.vma by address and ASID, and a whole-ASID flush when a PID is reused; then the
  shootdown after SMP1.
- New, not yet put: in a confined boot a labelled domain may not read the shared unlabelled
  userland disk (R34). Exempt a read-only, hash-checked volume, or one fsd per label set.
- K19 and SMP2's checkpoint, as architect-10 left them.

BEAM2: brief complete as a draft (`BEAM2-implementer.md`, plan rev 338); recheck against FSD3's
merged pages (recipe, pack, case.rs [disk]) before launch.

## What consumed my context

Kernel code for IPC3/K22, the hardware sweep's greps, BEAM2's reading. plan_set and plan_get echo
the whole node list each call: batch them.
