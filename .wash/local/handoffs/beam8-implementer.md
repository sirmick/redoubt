BEAM8 handoff #2 (beam8-implementer), 2026-10-06 — supersedes the first.

## Branch state (read first)
- Worktree /home/mcloonan/redoubt/.worktrees/BEAM8, branch wp-BEAM8 checked out, tree CLEAN, nothing running.
- wp-BEAM8 = 992d447ce (main, PACK1 merge) + ONE commit b692a3713 "beamlet: a module's code is 8-byte instructions over one operand array" (vm src module/interp/loader/memory/vm.rs + docs/userland/beamlet.md subsection). Its table, text and message are NOW RE-MEASURED ON 992d447ce (final; do not redo).
- The image (budget) commit is NOT yet redone. Local branch beam8-old (d63cf78ed) holds the old one on base fa08fe2c8 for wording only; delete it when done (never push).
- Nothing pushed. limits.rs fix dropped (PACK1 877f7f482 has it).

## Traps
- How to run things: read /home/mcloonan/redoubt/.wash/local/RESUME-q.md and the NEW resume note (B18/B19 change how cases run). Old way was `make -k -f /home/mcloonan/redoubt/.wash/local/jobs.mk -C <worktree> rv64/<case> rv32/<case>` and `q run --cores N -- cargo ...`; never bare cargo, never -j, never jobserver.
- NEVER plain `cargo fmt` (stable reformats the whole crate): `rustfmt +nightly --edition 2021 --config skip_children=true <file>`.
- Footprint console lines: target/testbench/run-*/beamlet-footprint-<w>-smp1.log (not target/jobs). Scan line `heap beamlet P of C pages` in target/jobs/<w>-<case>.log. (B19 may move logs: check.)
- Page table rows are held/4096 rounded to nearest.
- heap_pages = budget − 18 (stack 17 + 1); budget = roundup128(2*maxpeak + 18).
- tests/size-budget.toml has no VM entry: no Size budget line.
- vm.rs one-line change (Module::replace_body) GRANTED by orchestrator; record it.

## Measured on 992d447ce (final numbers)
- footprint before (main) rv64: instrs 1,944 ops 4,455 accounted 7,660, runtime held/peak 7,875/8,240, scan 9,368; rv32 993/2,313/4,502, 4,616/5,019, scan 5,511.
- after (b692a3713) rv64: 514/2,081/3,856, held/peak 4,073/4,537, unaccounted 217, scan 5,240; rv32 514/2,081/3,791, 3,906/4,373, 115, scan 5,067. (Peak above held is now mostly the boot pack, held until the prompt.)
- Six rounds, 512 MiB, all 48 PASS: max beamlet heap peak rv64 userland-read-only 5,428 (4x 5,427, 2x 5,428); rv32 userland-read-only 5,231; userland-boot 5,228/5,055; footprint 5,240/5,067; init-boot varies 2..3,396 (pack read at init-boot time; below the others).
- => budget 10,880, heap_pages 10,862 (6 over 2x5,428). FLAG thin margin: a peak of 5,431 breaks the cap rule.

## Next: the image commit on top of b692a3713
Edit (all currently 20864/20846 on main):
- image/manifest.json lines ~75-78: heap_pages 10862, budget pages 10880, args budget_pages=10880 (keep "endpoint=erofsd:system").
- tests/data/boot-profile/manifest-unverified.json lines 68-71 same (main already updated it to 20864; outside owned paths, mechanical; flag).
- tests/beamlet-footprint.toml line 56 budget_pages=10880.
- servers/init/tests/manifest.rs ~451-453: comment "beamlet 10,880 pages" and `10_880` in the sum.
- docs/testbench.md ~1119-1123 paragraph, row ~1144 `| beamlet | 33,768 | 17 | 5,428 | 10,862 |`, ~1148 "(5,231 pages on rv32)", ~1150 sixteenth "(680 pages)". Template text: `git show beam8-old:docs/testbench.md` (search "10,862 pages: at least") — adapt numbers: 6 pages over; spare numbers below.
- docs/kernel/budgets.md ~209-219: template from beam8-old; numbers: peaks 5,428 rv64 / 5,231 rv32; servers need = 31,243 − 20,864 + 10,880 = 21,259 (EROFS kept the sum: erofsd:system 1024 like littlefsd). TRAP: system's free pages at 512 MiB may have moved: init-boot's console now prints "Budgets: ... system 31635" (rv32) / "system 31677" (rv64) — that is system's budget, while the page's "31,626 / 31,672 free" is the fit's free; verify how BEAM6 got "free" (a SystemFit probe) or phrase from the page's own numbers; spare = free − 21,259 (old: 10,367 rv32; client case −257 → 10,110). Breaking peak: 5,431 (cap 10,862 < 2×5,431).
- Commit message: template `git show -s beam8-old`; update numbers (5,428 / 5,231 from 10,387 / 6,048; 6 pages over).
Then the short gate on the head (both builds, docs, formatting, size-budget, unsafe-budget, no-cruft; host tests beamlet-vm+beamlet-redoubt via q; cases both widths: beamlet-footprint, beamlet-boot, beamlet-console, beamlet-heap-flood, beamlet-budget-flood, beamlet-lookup-host, beamlet-lookup-cli-host, userland-read-only, verity-flipped-tree, verity-wrong-root, userland-boot, init-boot, bench-net-peer, ipc-outcomes, init-host-tests). Then /home/mcloonan/redoubt/.wash/local/BEAM8-report.md and member_update assignment_results complete for 53877bb0f791a22bfe8194c2f8a8c6e2 (<=1900 B): head, gate exits, before/after, budget, flags (vm.rs grant; boot-profile copy outside owned paths; boot-profile*.toml still 1 GiB with stale comment, not in brief; thin margin; const assert instead of host test (accepted at checkpoint); .args sites 8 + patch; max arity 8 -> u8; List indexed {start,len}), summaries checked (beamlet.md, testbench.md, budgets.md updated; image/README.md, README.md, GETTING-STARTED.md cite no number/representation: no change).
