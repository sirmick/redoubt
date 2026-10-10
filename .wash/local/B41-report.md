# B41 report

Branch wp-B41, head 470adb679, one commit on origin/main 5f66a4963. Worktree .worktrees/B41.

## Bisect

Script: init-refuses-bound on rv64, run with `q run --cores 8 -- cargo testbench` in a scratch
worktree (since removed), reading the bound init printed. `git bisect --first-parent`, good
fdafcf2cb (the first merge carrying the case's last change, prints 1,047), bad 0abd7b1cb. Every
step printed 1,047 or 1,061, nothing else.

- 311b0b22b (BOOT2 merge's first parent): 1,047, PASS.
- **0abd7b1cb (BOOT2's merge): 1,061, the first bad.**
- 3fef5d657 and its parent 0abd7b1cb: 1,061.

## Cause: the expectation is wrong, not the code

BOOT2 changed `servers/init/src/bound.rs` `LEND_PAGES` from 2 to 16 (and init.rs's CHUNK from 1
to 8 pages), so the session image moves through 16-page lends. The bound counts that lend with its
tables, so every manifest's bound grew by 14 pages:

- the image's bound went from 550 to 564. BOOT2 updated docs/kernel/budgets.md and init's host
  test (tests/manifest.rs, `+ 5` → `+ 19`).
- init-refuses-bound's manifest (tests/data/init/bound.json, the image's plus eight servers)
  went from 1,047 to 1,061 on both widths. The case was not in BOOT2's gate list.

The refusal still holds, since 1,061 is over the 1,023 root keeps. What the case tests is
unchanged.

## Fix

tests/init-refuses-bound.toml: the expected line says 1061. The comment now gives init's own costs
as 994 pages (was 980), the 16-page lend among them.

## Gates (from .worktrees/B41, env exported)

- `make -f scripts/jobs.mk prebuilt`: rv64 236, rv32 222 cases, 0 failed.
- `make -k -f scripts/jobs.mk rv64/init-refuses-bound rv32/init-refuses-bound` × 3: 6/6 PASS.
- `q run --cores 4 -- cargo testbench --exact init-host-tests`: PASS.
- docs (`jobs.mk docs`): PASS. formatting (bench case): PASS.
- size-budget: not run; no code changed.

## Docs

No page states 1,047 or 980. budgets.md (564, the 16-page lend) and init.md, which list the
case in their status lines, already match the code. No change needed.
