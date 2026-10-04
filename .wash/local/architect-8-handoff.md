# Architect handoff (architect-8, 2026-10-03)

Earlier handoffs' working rules stand (architect-7-handoff.md first lines): pages on main staged
by path, doccheck before a commit, QA bodies at most 2000 bytes, no thread names or dates on
pages, exact lines for packages, nothing on a page before its code. Watch lists are in
`architect-8-notes.md`; architect-7-notes.md's owed edits stand except its held K21/M2 line,
which is dropped.

## Rulings, each with its file

- FSD1-fid-rename: (A) a fid is path + id; the id is written in the creating commit, after the
  counter moves; the mount checks ids without writing. `FSD1-fid-rename-ruling.md`. FSD1's page
  lines were checked at 970f7ddfd and stand.
- Serve helper: node RT2 (needs FSD1). Its body is the brief.
- K16-churn-ceiling: the shell variant's ceiling goes, because R12 is a floor.
  `K16-churn-ceiling-ruling.md`.
- SMP1-design: the owner chose many harts by design, built as SMP1, then SMP3, then SMP2; targets
  gated at 1 and 2 harts, 4 recorded; no ASIDs. TENETS and m2 step 4 changed on main at 082e00ccf.
  SMP1's brief is updated (shootdown to a set of harts; aligned with the bitmap). Nodes SMP3 and
  SMP2 are set. `SMP1-multihart.md`.
- INIT3: consoled's fids scale with MAX_THREADS, with a 2 MiB budget. launcher-orphan's reading
  holds. Pages checked OK at 1ce772734, with one edit: a status clause saying netd's restart has
  no case yet.
- FSD2 and FSD3 briefs are written. FSD2: quotas held by subtree, counted at the first mint,
  nothing stored; a metadata pair named twice is corrupt. FSD3: blkd takes range labels as
  `labels.P=` arguments; mkimage packs through fsd's code; no volume check at restart.
  `FSD2-implementer.md`, `FSD3-implementer.md`.
- K21-free-cost: a free-frame bitmap with fixed-depth summaries replaces the free list. Round 2:
  the bitmap is a `&'static mut [u64]` made once at boot (+1 unsafe; the SAFETY text and the
  ratchet reason are in my message). `K21-free-cost-ruling.md`.
- K16 data region: grows to 1 MiB on both widths (LENGTH 1024K, with an assert that it ends below
  the kernel stack), and no shrink. memory-layout.md's two rows say 1 MiB. Answered by message;
  there is no file.

## Open with you

- The SMP3 and SMP2 briefs (from the node bodies and `SMP1-multihart.md`). SMP2 restates R12
  across harts with several runners per budget.
- The M1 page edit at the init step's close, after INIT4 (architect-7-notes.md).
- K19 waits on the owner (destroy-simplify.md).

## Merge-time checks owed (details in architect-8-notes.md)

- FSD1: the fid lines, the mount check, the five tests, and fsd.md:137's rewrap.
- INIT3: re-check after red's fixes, including the netd status clause.
- K21: the bitmap per the ruling; R10 p50 and p99 against main (~18.7 / ~22.4 ms); main's m2
  step 4 kept; the memory.md lines.
- K16: the churn bound (500-TOL, 1000) and the evidence run; scheduling.md's lines; ASID 0; the
  1 MiB region and its page rows.
- RT1: architect-6's list.

## What consumed my context

Reading kernel and fsd code to rule (littlefs's flush, mem.rs's allocator, the server loops) and
long tool outputs (plan_set returns the whole plan each time). Read ranges; use plan_get with a
node.
