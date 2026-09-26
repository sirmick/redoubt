# DOC1 lead handoff

Updated by the second lead after the kernel set (below).

The first lead handed off early: the plan phase (surveys, the manifest, the inventory map)
filled its context before the kernel set began. A fresh lead continues from here.

## State (branch wp-doc1)
- The approved manifest is `todo/DOC1-manifest.md`. The owner approved it on 2026-09-26 with
  every section-L decision as recommended. It is the contract: section C lists the pages,
  D3 the inventory item map, H the order of work, I the acceptance checks, J the checker, K the
  switch-over, and S the style guide.
- Committed so far:
  - `9fe7d1c15`: `docs/book.toml`, `docs/theme/` (mdbook-mermaid 0.17.1 assets),
    `docs/SUMMARY.md` (skeleton; unwritten pages are draft chapters `[Title]()`),
    `docs/GLOSSARY.md` (draft) and the manifest.
  - `8625ec08d`: `docs/kernel/ipc.md`, the finished sample from the manifest.
- `mdbook build docs` succeeds. It prints one warning from the plugin itself: "mdbook-mermaid
  preprocessor was built against version 0.5.0". The checker's `the_book_builds` must ignore
  that line. Tell the checker implementer in QA thread DOC1-checker if they have not seen it.
- Running in parallel on other branches: wp-doc1-wire (manifest section E) and wp-doc1-check
  (manifest section J). The orchestrator merges them. Once the checker lands, run
  `cargo run -p redoubt-doccheck -- --pages docs/kernel` before every commit.

## The kernel set: written (step W3 done)
All 14 pages of `docs/kernel/` are committed: README, objects, ipc, memory, budgets,
scheduling, timer, processes, devices, boot, memory-layout, abi, invariants, model
(`cd48e40ff`, `df0d1fd71`, `340a7d993`, `c53f651c7`). SUMMARY lists them all.
`doccheck --pages docs/kernel` is clean except links to pages outside the set (servers,
TENETS, testbench, todo, beyond); `mdbook build docs` is clean.
- How they were written: one drafter per page read the code and tests (brief kept at
  `/tmp/doc1-kernel/BRIEF.md`, notes per page at `/tmp/doc1-kernel/<page>.notes.md`; /tmp is
  not durable). The lead read and edited README, objects, budgets, timer, devices, processes,
  memory, scheduling and boot in full. memory-layout, abi, invariants and model were reviewed
  from their notes and the checker only: the red team (R1) should read those four closely.
- Rule IDs as written are recorded in the manifest's B3. The servers set starts at R24.
- Attack gaps: `todo/kernel-attack-gaps.md`, per page. `boot.md` links it as
  `docs/todo/kernel-attack-gaps.md`, so W6 moves it there.
- New follow-up slugs the pages link are listed in the manifest's C7 (W6 writes them).
- Code findings sent to the orchestrator for routing (not doc questions): `set_flags` makes
  device and DMA pages executable (R11 gap); `map_anon`'s quadratic search before any budget
  check (a machine-wide stall); `boot_budgets` leaves `root`'s frame uncharged (a one-page
  overcommit that ends in a kernel stop); a deadline's destruction is billed only in part, and
  a weight-0 budget's not at all. The pages state each as a residual with a todo link; if the
  code is fixed first, update the page and its status line.
- Checker notes: C4 flags `ed25519-compact` as a hash (boot.md writes `ed25519_compact`);
  C5(c) does not match a short name wrapped across two lines (the pages keep each citation on
  one line).
- Small leftovers for the editor: GLOSSARY `PID` sits after `powerbox` (alphabetical order);
  mutation variants named `K5b*` carry a package ID in their names (rename at switch-over, e.g.
  `Dma*`, and update devices.md, invariants.md and model.md).

## Next: review R1, then the servers and userland sets (W4, W5)

## Fixed decisions a writer must keep
- New kernel IDs are candidates; you may merge or drop one while writing, and must record the
  final list in the manifest's B3:
  - R13 one outcome per call; R14 unforgeable sender. Both are in ipc.md.
  - R15 verified boot; R16 the loader confines images; R17 fail closed at boot.
  - R18 device objects are the only device authority; R19 the kernel's own mappings are W^X.
  - R20 a reused PID inherits nothing; R21 crash blame (fold "the exit endpoint must be badge
    0" in here, or allocate the next number).
  - R22 a range call's cost follows page-table occupancy; R23 no test-only diagnostic channel
    in the production kernel.
  - I16 DMA pages reset before reuse (replaces the model's `I-DMA`).
  The servers set continues from the kernel set's last number.
- Short names are exact, because citations must match the heading:
  - I1 handles name live objects; I2 revocation is complete; I3 minted badges are non-zero and
    narrow; I4 only badge-0 handles receive; I5 usage within limits; I6 labels only grow
    downward.
  - I7 every flow obeys R1; I8 class and account inherited; I9 pages W^X, zeroed, lends
    unmapped; I10 create-destroy leaves the parent unchanged; I11 fair turns; I12 ids never
    reused.
  - I13 every blocking call returns by its timeout; I14 no call panics the kernel; I15
    abandoned calls reported once; I16 DMA pages reset before reuse.
  - R9 is "stamps", R10 "destruction", R11 "memory".
  - Rule F's short name is "trusted verdicts" (GLOSSARY links `testbench.md#rule-f-trusted-verdicts`).
- ipc.md links anchors that the neighbouring pages must provide:
  - `objects.md#r9-stamps` and `objects.md#mint`
  - `budgets.md#r10-destruction`
  - `processes.md#exit-notices` (a heading "Exit notices")
  - `abi.md#errors-and-the-order-of-checks`
  - `../servers/README.md#labels` (a heading "Labels")
  - `../servers/init.md#restarts-and-reboots`
  - `../TENETS.md#threat-model`
  - `../todo/receive-output-late-invalid.md`
- The GLOSSARY links `kernel/budgets.md#r7-carving`, `kernel/memory.md#r11-memory` and
  `kernel/ipc.md#r4a-open-calls`, among others.

## Traps
- Commit attribution: the orchestrator's message asked for a "Claude Fable 5.1" trailer. The
  commits so far carry the model that actually wrote them (Opus 5.5). Use your own model's
  trailer.
- Never git stash. Stage paths by name. No `<!--` in QA bodies. Messages to the orchestrator
  are at most 2000 bytes.
- The `no-cruft` gate forbids "legacy" in `.rs` and `.toml` under kernel, libs, loader, tests,
  stub and tools/testbench.
- Do not read `.wash/QA.md`; it is 1.65 MB.
- The switch-over (manifest K) also deletes `docs/.nojekyll`, which the checker's C8 flags. The checker implementer has been told.

## Open QA
- DOC1-plan: approved.
- DOC1-wire-tables and DOC1-checker: the implementers may ask spec questions. Answer them
  briefly, from manifest sections E and J.
