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
1. `K5b*` mutation names and `Mutation::rule()`'s labels change at the switch-over; model.md's
   Mutations paragraph and table then need the new names.
Everything else on the R1 list is applied, RED-18 as the Architect decided on
DOC1-timer-rule-ids (citations of I13, R10 and R12 on timer.md; R10 on budgets.md gains the
deadline-first and equal-instant sentence), and the red team's clarified P3s.

## Top level (assignment 55bcbb40, written by the top-level lead)
Commits on wp-doc1: `ac2487bdd` glossary, docs/README, SUMMARY, todo index, rustfmt todo;
`a59276590` TENETS; `86e854009` plan/m1..m5; `8b53b3e37` beyond/ (19 pages from C8, plus
image-cache); `ce5b957ea` testbench.md (defines Rule F), todo/ssh-loopback-host; `6c3799076`
SWARM and docs/PROJECT; `0beffa264` root README, GETTING-STARTED, CONTRIBUTING and
CODE_OF_CONDUCT (git mv'd), TENETS "no shell script on the machine"; `53fe7ebf8` SECURITY.
`cargo run -q -p redoubt-doccheck` finds only `docs/.nojekyll`; `mdbook build docs` is clean.

Left for the owner or the next package:
- TENETS keeps one **Open:** (capability closure against same-label delegation).
- CONTRIBUTING sends vulnerability reports to GitHub's private advisories: confirm the channel.
- The manifest's missing todo pages are written (assignment 03ffcfcd): print-panic-reentry,
  hosted-kernel-tests, kernel-test-hello (kernel package); raw-syscall-runtime-audit (servers
  package); verdict-strings (the switch-over); bench-load-flakes, miri-vendored-unsafe,
  programs-build-rerun (verified: the manifests and Cargo.lock are still unwatched),
  write-only-mutation-split (no package). `shared-image-pages` is `beyond/image-cache.md`.
- SWARM's claims table is the orchestrator's to keep; package IDs must avoid R, I and M (the
  checker reads them as rules, invariants and milestones).

## R3 fixes (assignment c4e3f729, the top-level fixer)
Commits: `44ee5153c` TENETS; `5836841a2` testbench; `9116b0be4` SECURITY (and invariants.md's I14
status line, so the register agrees with it); `660976a98` plan (and todo placements,
verdict-strings widened); `3667e128b` SWARM, PROJECT, GETTING-STARTED; `4cc9655f8` inventory
misses (fsd, init, beyond/image-cache, docs/README, manifest D3). New todo pages:
host-shell-scripts, stub-unsafe-budget.
Left for the owner: whether tenet 3 reaches the build host (TENETS residual + todo); the
vulnerability channel in CONTRIBUTING. A-22 and A-27 were ruled by the Architect (QA thread
DOC1-a22-a27) and are written as decided on fsd, files, init and beyond/image-cache. Not applied: beyond/README.md's title "Beyond M5"
(outside the pages named; the checker allows it); the inventory's borderline items.

## Switch-over notes (for the implementer)
- Checker C2 misses a `#[test]` followed by another attribute before the `fn`.
- Checker C4 flags `ed25519` as a commit hash.
- Checker C7 counts an ID inside a rule's name (I7 names R1) as a second ID in the Rule cell;
  SECURITY works round it by citing I7 with its name in the prose above and bare in its row.
- Delete `docs/.nojekyll` (the one remaining finding).
- Checker C5's definition pattern skips a heading that adds words after the short name
  (`### I13 (...), on the timer`, `### R10 (destruction) at a deadline: ...`, `### R12 (scheduling)
  for timer work` in `kernel/timer.md`), so their tests and gaps never reach C7; SECURITY's
  residual cells link them by hand until the checker merges them into the owning row.
- Delete `kernel/assemble.sh` and `kernel/assemble.ps1` (referenced nowhere;
  todo/host-shell-scripts.md).
- Code and case files cite testbench headings that no longer exist: "Debug assertions"
  (`Cargo.toml` and about ten tomls; now "Checked builds"), "Writing an attack case" (now "Rule F
  (trusted verdicts)") and "Peers" (now "Peers, dials and the capture").
- `libs/signing/src/lib.rs:3-4`: the comment is stale.
- Stale process comments in `stub/`, the `blkd`, `netd` and `consoled` bin docs,
  `image/boot.toml` and `libs/wire/tables/example.md`.
- Model mutation names `K5b*` (and `Mutation::rule()`'s labels) change; `kernel/model.md`'s
  Mutations paragraph and table and every status line naming them follow.
- The Python tour tools (`tools/gen_readme.py`, `tools/test_readme_links.py`) and graphviz in the
  `Dockerfile` go; the `Dockerfile` has no `mdbook`, `mdbook-mermaid` or `mdbook-svgbob` yet
  (GETTING-STARTED tells a local machine to `cargo install` them).
- Root `PROJECT.md` is deleted; Wash's plan document becomes `docs/plan/m1-separation.md` and the
  orchestrator's start-up read `docs/PROJECT.md`.
- Root README and GETTING-STARTED are already rewritten (section K step 2 is done).

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
