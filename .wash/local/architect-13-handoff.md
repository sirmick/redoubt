# Architect handoff (architect-13, 2026-10-03)

Working rules: architect-7-handoff.md's first lines, which stand. Watch lists in architect-12's,
11's, 10's and 9's notes also stand.
- Messages to the orchestrator are at most 2000 bytes, counted strictly. Aim for 1,700: four of
  mine bounced at 2,0xx this stretch.
- Review-only checks: never Write, Edit or redirect into a file.
- Use the worktrees by SHA. `cd` into a worktree moves the session's working directory, so prefer
  absolute paths.

## Sent and done this stretch

| Item | State | Where |
| --- | --- | --- |
| BEAM2 confined index (labelled beamlet can't reach bootfsd under R34) | sent to the owner via the orchestrator; **superseded** by the verified-volume recommendation if the owner takes it | `BEAM2-confined-index-ruling.md` (recommended: index on the disk, hash in the signed manifest) |
| MEM1 (stack_pages, paint, bench RAM scan) / MEM2 (heap_pages cap in rt, startup block v2) | briefs trimmed to S+ each; plan nodes rev 365; MEM1 needs BEAM2, MEM2 needs MEM1 + ABI1 | `MEM1-implementer.md`, `MEM2-implementer.md`. Owner may cut MEM2 further (drop heap record + measured caps): pending |
| Yield (no-MMU seam) | assessed: yield = rt's `sleep(0)` (poll, no kernel change); XS, rides in ABI2 (doc paragraph on sleep + host test); optional `rt::yield_now()` alias is the owner's pending choice | answer in the orchestrator's thread |
| K16 recheck of IPC3/K22/HW1/ASID1 briefs on main 53bcd9704 | sent. K22: reconcile todo page now K22's to delete, budgets.md line dropped, rv32 by local override. IPC3: links at testbench.md:193 and budgets.md:766. HW1: no changes. ASID1: holds (15 flush sites). Order HW1 then ASID1 agreed | instructions to each implementer |
| ABI1 merge check (3314623c8) | OK, then follow-ups folded into the rewrite (abi1-implementer-2): (1) drop `installed-transport`; (2) device_info in rt is intended, reversing INIT2 ruling (c), plus a native.md line ("Nothing above the runtime calls the `ecall` itself..."); (3) abi.md:3/:30 stay; (4) crate item names allowed; abi.md "It refuses" -> "`decode_result` refuses"; **`unsafe trait Transport`** with a `# Safety` contract, unsafe impls with reasons, abi.md "(the crate's only `unsafe` code)"; scripted fake: the bootfsd outcomes test fixed (raw map_anon lends), and my refinement that **the script-loading fn is an `unsafe fn`** with `# Safety`, not only a reason on the impl | orchestrator merges; I see the tip only if the rewrite goes beyond those |
| BEAM2 blkd endpoint | ruled (b): `endpoint=NAME` required, like fsd. Page lines: blkd.md "Its endpoint" bullet; init.md sentence after the R35 one-of-each rule | sent to beam2-implementer-3 |
| IPC3 layout checkpoint | **OK with rulings**: two lists (group and send-group); per-budget stamp chains at budget words 104/105; audit only at outermost exits, id 3 (K22 told to take 4); expiry walk+sort billed in **equal shares** (R12); EXPIRY bracket narrowed to collect+sort in time.rs; page lines (ipc.md receiver arrival order, devices.md IRQ FIFO, timer.md:244-246, scheduling.md:195-197, budgets.md R10 note) | `IPC3-layout-ruling.md`; ipc3-implementer-2 codes from it |
| Verified volumes (owner thinking aloud) | **sent** to the orchestrator for the owner: block-level verity in fsd (dm-verity shape), root pinned in the signed manifest entry, no signing now; VOL1 Tier A size M; land BEAM2 as built, then VOL1 replaces system.index (alternative: pause BEAM2) | `verified-volumes-assessment.md` |

## Open: what the next Architect watches

- **The owner's answers:**
  - verified volumes (VOL1 and the order against BEAM2). If yes, cut VOL1's brief from the
    assessment.
  - BEAM2 confined index: moot if VOL1 is taken.
  - The MEM2 further cut.
  - The yield alias.
  - Still pending from before: K19, h/1 docs (BEAM2 point 9), SMP2's checkpoint, the 30% mark, B6.
- **BEAM2:** point 7 is in progress (beam2-implementer-3). The index and cases 1-5 are held until
  the owner answers on verified volumes. At its merge, check the brief's page lines plus the
  "Rulings during the build" section, the blkd endpoint lines, and init.md's per-disk lines.
- **IPC3:** check at merge against `IPC3-layout-ruling.md`: equal-share billing, the audit only at
  outermost exits (never inside X..Z), both-list tail moves, the page lines.
- **K22:** at merge, check that the reconcile todo page and its links are deleted, the
  scheduling.md residual is consistent with IPC3's (whichever merged second), and audit id 4.
- **HW1:** no ASID_BITS row. **ASID1** launches after HW1 and adds the row; recheck its line
  references then.
- **ABI1:** merges after the rewrite. Then **ABI2** carries the yield doc paragraph and its host
  test, plus the alias if the owner wants it. Add both to ABI2's brief when the owner answers.
- **MEM1** after BEAM2, **MEM2** after MEM1 and ABI1. If VOL1 is taken, MEM2's startup block still
  stands; check that nothing in MEM1/MEM2 names system.index.

## What consumed my context

The IPC3 layout check (model next_sender/served, frame words, the oracle's audit rules), the ABI1
diff, the MEM briefs. Use `git diff -U0` and targeted ranges.
