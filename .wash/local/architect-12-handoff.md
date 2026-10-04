# Architect handoff (architect-12, 2026-10-03)

Working rules: architect-7-handoff.md's first lines (pages on main staged by path, doccheck before
a commit, QA bodies <= 2000 bytes, exact lines to packages, nothing on a page before its code, end
the turn after a decision_request). Watch lists: `architect-12-notes.md` (this stretch, every
check owed at merge), and architect-11's, 10's and 9's notes, which stand. Messages to the
orchestrator: <= 2000 bytes, counted strictly; aim for 1,800.

## Briefs written or amended this stretch, and their state

| Package | Brief | State | Notes |
| --- | --- | --- | --- |
| ABI1 | `ABI1-implementer.md` | complete; node todo, no needs (plan rev 362) | Owner's direction: redoubt_sys::Transport, Ecall, rt's one installed transport, init's 2 DeviceInfo calls via rt, beyond/README.md no-MMU line. Size S. |
| ABI2 | `ABI2-implementer.md` | complete; node todo, needs ABI1 | Lend/transfer as ownership on ipc.md (exact text), fake refuses given-up pages, docs only on LendDisposition/Buffer; no kernel change (verified take_buffer, give_buffer_back, move_buffer). Size S. |
| ASID1 | `ASID1-implementer.md` | complete; node todo, needs K16 | Owner cut. 15 flush sites tabled; destruction moves satp off before freeing (garbage G leaf); QEMU TLB not ASID-tagged (implementer confirms), negatives trip via the audit. Recheck line numbers on main after K16 merges. |
| SMP1 | `SMP1-implementer.md` | amended; needs K16, IPC3, K22, ASID1 | Item 7 rewritten for ASIDs: per-PID stale-hart mask (lazy flush before install, no IPI), idle hart's satp to PID 1, shootdown flushes the target's ASID on the harts running it; smp-no-stale-mask negative; memory.md/memory-layout.md residual lines rewritten. |
| IPC3 | `IPC3-implementer.md` | amended; needs K16 | Item 6 the expiry (expire_due one walk or a deadline list; owns time.rs expire_due); next_timeout no longer a remaining walk; worst-walk loses must_fail/whole_run, runs on rv32 at what RAM holds; the delivery and expiry todo pages deleted. |
| HW1 | `HW1-implementer.md` | ready; needs K16 | Gained the ASID_BITS row, added only once ASID1 has merged. |
| K22 | `K22-implementer.md` | ready (architect-11's); needs K16 | Owns the reconcile todo page's "Done when". |
| BEAM2 | `BEAM2-implementer.md` | launched (beam2-implementer active) | Rechecked on main 75245a114. Rulings appended ("Rulings during the build"): whole applications (577 modules, 3.75 MiB); index keyed by file name (`<module>.beam`, `<app>.app`); [userland] case key, a flip changes the disk only, --pack-disk writes an index only when the recipe declares one. R34 ruled by the owner (2026-10-03): one read-only attachment per label set, R34 unchanged; beamlet.md's stated line is in the brief (no Open). h/1 docs: stripped by default, switch in point 9 (owner answer still pending). |

## Merge checks done this stretch

INIT5 (b7d73c4f8): OK + budgets.md test-list edit. FSD3 (a5b505e97): OK + fsd.md Authority
"the console `init` gave it". K16 round 2 (b81d94dde): OK, no edit; the expiry walk is IPC3's.
All three merged or merging; confirm the two edits landed at the next look at those pages.

## Watching

- **K16's final tip** from k16-implementer-8 (rebase onto main after INIT5/FSD3): recheck only
  what the rebase could move (the page numbers, the worst-walk case, the ASID lines). Then
  ASID1's brief line numbers against merged main before ASID1 launches.
- **BEAM2's step reports**: questions come from beam2-implementer; its merge check is against
  the brief's page lines and the rulings section (R75 text "a module or application resource",
  bootfsd.md's index line format, beamlet.md's confined line, init.md per-disk lines, the host
  test confined_gives_each_label_set_its_own_userland_disk under R34 and the confinement check).
- **Owner choices still pending:** h/1 docs (BEAM2 point 9 switch); K19; SMP2's checkpoint (as
  architect-10 left them).

## What consumed my context

Kernel mem.rs reading for ASID1, the three merge checks (diffs by range), the SMP1 rewrite. Use
`git diff -U0` and targeted `sed -n` ranges; plan_set echoes the whole node list every call.
