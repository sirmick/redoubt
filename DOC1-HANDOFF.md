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

## Still to do, in order
Read each page in full before committing it.
1. Group 2 rest:
   - RED-9 model.md:216-217: `Mutation::rule()` returns "IPC", "Messages", "QUESTIONS 2",
     "answer 173"... and "R4" for R4a/R4b; state it (the switch-over fixes it, manifest K1).
   - RED-10g devices.md:160 `system_reset`: partly tested (reboot not attacked).
   - RED-10h boot.md:225: the console and DMA-reset-slot refusals are not attacked.
   - RED-10i timer.md:40 and 205-211: add the gaps kernel-attack-gaps lists for timer.
   - RED-10j processes.md:267 R21: add the parked-call-then-fault gap.
   - RED-10k README.md:70, objects.md:26, memory.md:174: plain "tested" where the page says
     model-only or argued.
2. Group 3 rest (Architect's rule text is on the two code-findings threads):
   - memory.md: `map_anon`'s residual (about line 282) says "nothing charged": the search's kernel
     time is billed to the caller; the harm is latency (interrupts off, every wake waits); cite
     R12's new sentence. R11 gains the per-frame W^X sentence ("W^X holds per frame, not only per
     mapping. Only RAM that a process owns is ever executable. ...") with status partly tested
     naming the gap; devices.md's residual points to memory.md.
   - scheduling.md: R12 gains "A system call's kernel time is bounded by a constant plus a term
     linear in the pages it maps or the objects it names. It never depends on the extent of an
     address area or on what other processes hold. Billing it to the caller does not excuse it,
     because every wake waits for it." (R10's scan is the stated exception, todo
     budget-destroy-cost); the whole-cost billing rule as on budgets.md R10, with the departure
     in its residual (around line 346); the "kernel is not preemptible" residual cites the rule.
   - processes.md: the process-object counting rule (as on budgets.md R6), departure as residual.
   - boot.md: "The loader refuses to boot when RAM extends past `PHYSMAP_SIZE`, with a clear
     message." under R17 (fail closed), as a departure/planned part with todo
     physmap-ram-bound. Also RED-19 (hart 0, dt.rs:282-289) and RED-20 (verified-boot cases are
     rv64 only) while there.
3. Group 4: RED-11 memory.md:122 (check whether `process-lifecycle` really lends within one
   process across teardown before dropping the gap; kernel-attack-gaps memory.md line already
   dropped "across teardown", restore if not), RED-12..24, 27, and the P3 list; ED-1 leftovers:
   GLOSSARY `PID` sits after `powerbox`; `K5b*` mutation names are for the switch-over.
   Unidentified P3s: ipc.md:149-154 and 211, budgets.md:279 (ask the red team what they mean).
4. Complete the assignment with a summary (at most 2000 bytes).

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
