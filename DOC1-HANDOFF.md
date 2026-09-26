# DOC1 lead handoff

Updated by the third lead, part way through the R1 fixes (assignment 20a7cb58, "R1 fixes for the
kernel set"). Read the assignment text, then this file, then the QA threads DOC1-red-R1,
DOC1-device-mapping-exec and DOC1-kernel-code-findings (workspace_get view qa, thread_id).

## State (branch wp-doc1)
- The approved manifest is `todo/DOC1-manifest.md` (sections B ID map, C pages, D3 inventory
  map, I acceptance checks, J checker, S style guide; todo shape: What / Why it matters / Where /
  Done when, enforced by the checker's C9).
- The kernel set (14 pages) is written. R1 review findings are on DOC1-red-R1 (RED-1..30 plus a
  P3 list in its last event).
- Checks before each commit: `cargo run -q -p redoubt-doccheck -- --pages docs/kernel docs/todo
  docs/GLOSSARY.md docs/SUMMARY.md` (only forward links to servers/userland/beyond/TENETS/
  testbench and SUMMARY's draft `[Title]()` chapters may remain; filter them with
  `grep -v 'C6: link \`\(\.\./\)*\(servers\|userland\|beyond\|TENETS\|testbench\)' | grep -v
  'link \`\`\|\`\` is linked'`), and `mdbook build docs`.

## Done by the third lead
- `8837e6489` group 1: `docs/todo/` has all 18 linked slugs plus
  `endpoint-destroyed-open-calls.md` (restores inventory A-32); `kernel-attack-gaps.md` moved
  there (git mv) in the todo shape. SUMMARY lists every todo page under Follow-ups. The seven
  package items carry "Fixed in the kernel follow-up package after the documentation rewrite,
  before the work on `init` and the manifest." (not "before R3": a bare R3 on a page is the rule
  R3 (lends and abandoned calls), which C5 checks, and S1 forbids package IDs).
- `56ba7782a` ipc.md and budgets.md: RED-1..7, RED-10a-f; ipc P2s 11 (restated: process-attack
  attacks a record bad at `receive`'s start, not one made unwritable while waiting, so the gap
  stays), 22, 26 (A-33), 28, 30; budgets P2s 25, 29, P3 at 73 and 407-413. Architect amendments
  on budgets.md: R6 `root`'s own page, R6 process-object counting (pid-pool-pinning), R10
  whole-cost billing; each written as the rule, with "the kernel departs from this" in a
  Residual risk with its todo, and the section's status partly tested naming the departure.
- `82321806f` memory-layout.md: RED-8; R24 (SUM and MXR clear) allocated, planned · M1, with
  a new `## Security properties` section (the page is a reference page, so C9 allows it); B3 in
  the manifest records R24; **the servers set starts at R25**. P3 at 208 and 316-317.

## Done by the fixer (assignment 6cbd0325)
- `686a08537` RED-9, RED-10g-k. `202b38004` the Architect's amendments (R12 kernel-time bound,
  R11 per-frame W^X, whole-cost destruction billing on scheduling.md and timer.md, the
  process-object count on processes.md, the loader's RAM-bound refusal under R17), RED-19 (new
  todo `boot-hart-context`), RED-20. `e44b53951` RED-11 (process-lifecycle does attack the
  same-process lend across teardown, so only the harts gap stays), RED-12..16 and the abi,
  invariants and model P3s (catchers confirmed by a full `mutations_are_caught` run).
  `5a3bd56f1` RED-17, RED-21..25, RED-27, objects and processes P3s, and a new code finding in
  `map-anon-search-cost` (the search never tries the area's last start; its wrapped pass can
  run past the area's end). Then ipc.md's decoder combinations and R2 keying, the `MREx` size,
  the handle order defined once (boot.md owns it), GLOSSARY `PID` before `powerbox`.
- RED-26, 28, 29, 30 were already applied by the third lead.

## Still to do
1. RED-18 (timer.md's Security properties have no rule IDs): asked the Architect on QA
   DOC1-timer-rule-ids; recommendation is citations of I13, R10 and R12, no new IDs.
2. budgets.md:279 (R1 numbering; the R6 status line) P3: asked the red team on DOC1-red-R1
   what it meant.
3. `K5b*` mutation names and `Mutation::rule()`'s labels change at the switch-over; model.md's
   Mutations paragraph and table then need the new names.

## Traps
- Commit trailer: the model that wrote the commit. Stage by path; never stash; never push.
- C5(c) does not see a short name wrapped across two lines: keep `R10 (destruction)` on one line.
- A rule the code does not yet keep: write the rule, say "the kernel departs from this" with a
  residual and todo, mark the section partly tested naming the departure (the Architect's
  pattern for R11), or planned · M1 when nothing of it is built (R24).
- Messages to the orchestrator at most 2000 bytes; no `<!--` in QA bodies; never read
  `.wash/QA.md`.
- The `no-cruft` gate forbids "legacy" in `.rs` and `.toml` under kernel, libs, loader, tests,
  stub and tools/testbench.
