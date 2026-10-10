# B32 report

Branch wp-B32 at 6318394c6, one commit on main 2e3057e64: "image: a shell session is 11,904 pages,
for the line editor under the shell's driver". The measurement is in .wash/local/B32-measure.md;
the commit message carries the breakdown and a `Memory budget:` line.

## What changed

- image/manifest.json: sizes.session 11,008 → 11,904; alice's top budget 44,040 → 47,624. Her
  share per label set is 47,624 / 2 − 1 = 23,811, which holds two 11,905-page sessions with one
  page spare, as before (libs/steward/src/manifest.rs:152).
- The image manifest's copies, changed the same way: tests/data/boot-profile/manifest-unverified.json
  and tests/data/steward/vault-launch.manifest.json. The second arrived with 2e3057e64 during
  the work.
- tests/data/beamlet-footprint/manifest.json: budget 11,904, heap_pages 11,885, the budget_pages
  arg; tests/beamlet-footprint.toml: budget_pages=11904.
- docs/userland/beamlet.md "What the VM holds at its prompt": a new table (rv64 and rv32 rows from
  this run), and the budget paragraph names the line editor's modules and the growth (+420 rows,
  +617 peak, seven steps from 11,008).
- docs/kernel/budgets.md: 11,904; peaks 5,904 / 5,705; the cap under twice the peak from an rv64
  peak of 5,943; alice 47,624.
- docs/testbench.md memory section: stack peak 36,136 bytes (twice fits the 18-page stack), cap
  11,885 of 11,904, 77 pages over twice 5,904, process/ETS limits 744 pages.
- image/README.md: 47,624 pages, sessions of 11,905.

## Re-measured after SHELL3 (rebased onto 2db31544b, then 2e3057e64)

The rv64 peak is 5,904 (one run 5,905) and rv32's 5,704–5,705, against 5,902 / 5,703 before
SHELL3. Under the rule: 2 × 5,905 + 18 = 11,828, so 11,904 still. The number did not move.

## Gates (from .worktrees/B32, env exported, after the final rebase)

- `cargo testbench --exact init-host-tests` and `steward-host-tests` (q run --cores 4): PASS.
  The init tests check the manifest copies.
- `make -f scripts/jobs.mk prebuilt`: 0 failed on both widths.
- `make -k -f scripts/jobs.mk rv64/<c> rv32/<c>`: all PASS on both widths: beamlet-footprint (heap
  5,904 / 5,704 of 11,885), userland-boot, userland-read-only, steward-boot,
  steward-ssh-two-principals, steward-vault-session, steward-sub-budget-flood (B31 is not merged;
  passes now), steward-vault-launch (added: its manifest is a copy).
- docs: PASS. formatting (`cargo testbench --arch rv64 --exact formatting`): PASS, run before
  the last rebase; no Rust changed.
- ./test-shell: not run. Neither the shell nor beamlet changed.

## Docs check

Changed: beamlet.md, budgets.md, testbench.md, image/README.md. `git grep` finds no other 11,008 /
10,989 / 44,040 / 11,009 in docs, image, servers or the steward's library. steward.md states no
sizes.

## Notes

- The Enumerable cut and consolidated protocols are left to SHELL7, as directed. beamlet.md names
  the reason those three modules load.
- Committed files: I read the changed regions of the large pages (testbench.md and beamlet.md
  sections, budgets.md's session paragraph), not those pages whole.
- Scratch: .worktrees/B32-pre removed. .tmp/B32 stays until the merge.
