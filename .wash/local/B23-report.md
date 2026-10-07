# B23 report: beamlet's heap peak crossed half its cap

Branch wp-B23 in /home/mcloonan/redoubt/.worktrees/B23, on main b60c7cc5c, head aca3e5c18, clean:
1. 47e93dbcc `image: beamlet budgeted 11,008 pages` (B23)
2. aca3e5c18 `beamlet page: two residuals of the natives named` (BEAM4's page delta a4e57aaf9 over
   b25abdee5, rewrapped to the page's width; the words unchanged)

## Finding

The growth is real and wanted. BEAM4's five shell modules (Redoubt.Shell.Session with `ns`,
`ns_lookup`, `bind`, `exec`; Redoubt.Namespace, Budget, Process, Keys under them) are loaded at
the prompt by the commandlet registry like every non-wire module of the application (the page's
rule "Loading stays eager"). beamlet-footprint's breakdown against the page's table, rv64:
instructions +4 pages, operands +14, the shared literal table +18, atoms +2, held at the prompt
4,073 → 4,119 (+46), the runtime's peak 4,537 → 4,584 (+47). Nothing of BEAM4's platform side is
in the record: thread stacks come from `map_anon`, not the heap (libs/rt/src/thread.rs), and the
VM core's diff adds no allocation on the prompt or command path. The wire's 22 modules are
excluded from the walk and not loaded.

Pre-BEAM4 (orchestrator, c425bf4d7): rv64 userland-read-only 5,430 of 10,861, one page inside
twice the peak. Train 8 and this tree: 5,475. The budgets page's rule (twice the largest peak
across the memory cases plus the stack, rounded up to 128; "the budget then moves up a step of
128") gives 11,008 pages; heap cap = 11,008 − 18 (stack) − 1 (the budget's own page) = 10,989,
39 pages over twice the peak. The next step comes at an rv64 peak of 5,495.

## Peak table (this tree, heap beamlet, pages; the cap after the change)

| Case | rv64 before | rv64 after | rv32 before | rv32 after |
| --- | ---: | ---: | ---: | ---: |
| userland-read-only | 5,475 of 10,861 (FAIL, ×3) | 5,475 of 10,989 (×3) | 5,274 of 10,861 (×2) | 5,274, 5,274, 5,275 of 10,989 |
| userland-boot | 5,275 | 5,275 | 5,097 | 5,097 |
| beamlet-footprint | 5,287 | 5,287 | 5,108 | 5,108 |
| init-boot | 2 | 2,750 | 2 | 464 |

init-boot's beamlet figure is whatever the heap holds when the scan finds every record (no
command is typed); it varies with the moment and is far under the cap either way. The read-only
client: 13 pages uncapped (rv64), 12 (rv32).

## Files

- image/manifest.json, tests/data/boot-profile/manifest-unverified.json: heap_pages 10989, budget
  pages 11008, budget_pages=11008.
- tests/beamlet-footprint.toml: budget_pages=11008.
- servers/init/tests/manifest.rs: the fit test's sum (11_008) and its comment.
- docs/kernel/budgets.md: 11,008; peaks 5,475/5,274; need 21,392; spare 10,218 (rv32) and 9,961
  (rv64 with the client); the margin sentence (5,495; the step taken from 10,880 at 5,475).
- docs/testbench.md: cap 10,989, 39 over twice the peak; budget 11,008; the beamlet row
  5,475/10,989; rv32 5,274; the sixteenth 688 pages.
- docs/userland/beamlet.md: the prompt table remeasured on both widths from this tree's
  beamlet-footprint runs; the budget paragraph names the peak, the budget and what the 45 pages are.

## Gates (make -f scripts/jobs.mk -C .worktrees/B23 <target>, after `prebuilt` rc 0 on the tree
with both changes; every case rc 0)

PASS both widths: userland-read-only (rv64 ×3, rv32 ×3), userland-boot, beamlet-footprint,
beamlet-files, boot-profile-unverified (the consumer of the unverified manifest), init-boot.
PASS rv64: docs (also before the page delta), formatting, size-budget, unsafe-budget, no-cruft.
Host: `q run --cores 4 -- cargo test -p redoubt-init --test manifest` 54/54.
The gates ran on the working tree whose content is HEAD's; the prebuilt index is stale against
the new commit hashes only.

## Summaries checked

README.md, GETTING-STARTED.md, docs/plan/m1-separation.md, docs/userland/README.md,
docs/servers/init.md, docs/kernel/boot.md: no figure of beamlet's budget, cap or peak; no change.
The three pages that carry the figures are in the commit.

## Not done / queue

BEAM9 and BEAM10 not started, as instructed.
