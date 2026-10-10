# shell batch 1: main merged into shell, and the full gate

Worktree /home/mcloonan/redoubt/.worktrees/shell-batch1, detached, not pushed. Commit
**c01440ea4** "merge main into shell": parents 80c703534 (origin/shell) and 5f66a4963 (origin/main:
SHELL10, containing B40 ef2ad5d3a and BEAM17). Redone from 80c703534; the earlier merges
(e5452c597 on 415c86ad6, 9b95e9da3 on ef2ad5d3a) are superseded, and the run on 9b95e9da3 was
stopped before any gate reported. Logs: /home/mcloonan/redoubt/.tmp/shell-batch1/r3/.

## Conflicts (3 files, 4 hunks)

1. GETTING-STARTED.md, the line-editing paragraph: both. Main's "Ctrl+C or Ctrl+\ ending the
   line" and shell's sentence on building a screen program from `Redoubt.Screen.Widget` with
   dialogs, focus and themes; rewrapped.
2. docs/userland/shell.md, interrupting, "With a full-screen program in front" and "Over SSH":
   main's text (Ctrl+\ ends it; Ctrl+C too unless the program takes Ctrl+C; SSH INT and break
   become 0x1C). Shell's bullet pointed to "a native program's screen and the session's key" for
   the session's key; that section, as auto-merged, already names Ctrl+\, so main's bullet
   replaces the pointer.
3. docs/userland/shell.md, full-screen programs: shell's widget sentence (a box, a status line,
   the widgets that take keys, linking #widgets-focus-and-themes), then main's "A screen's life"
   (`run(module, args, opts)`, the interrupt not sent as a key, `ctrl_c: :key`).
4. userland/shell/test/redoubt/shell/driver_test.exs, the `start` helper: main's B40 `setup`
   (the shell runs in /), main's `rows` argument (`start([], 200)`), and shell's
   `Keyword.merge(defaults, opts)` with its comment. Main's callers pass no option the defaults
   set, so `++` against merge changes nothing for them.

## SHELL10 against shell's SHELL5/SHELL7 (auto-merged code)

- `Redoubt.Screen.run(module, args, opts \\ [])`; `ctrl_c` defaults to `:interrupt`. Shell's
  screens call `run/2`: `pick` (SHELL5's list widget) and `top` (SHELL7), so Ctrl+C and Ctrl+\
  both end them, as before. Ctrl+C never reaches pick, so its dropping of modifiers does not
  touch it.
- No widget binds Ctrl+C (Widget.Input uses Ctrl+A, E, U, K); a screen run with `ctrl_c: :key`
  gets `{:key, "c", [:ctrl]}` and its widgets pass it on.
- The driver's interrupt bytes, `[0x1C]` with `:key` and `[0x1C, 0x03]` otherwise
  (driver.ex `open_screen`), came from main unchanged. Main's screen_test `ctrl_c: :key` cases
  pass under ./test-shell on beamlet and on the BEAM.

## Gates on c01440ea4

| Gate | Result |
| --- | --- |
| prebuilt | rc 0 |
| docs (doccheck) | PASS |
| formatting, size-budget, unsafe-budget, no-cruft (rv64) | all PASS, rc 0 |
| `scripts/shell-cases 5f66a4963` set (30 cases, beamlet's set), rv64 and rv32 | 60/60 PASS, rc 0 |
| `./test-shell`, whole | every stage passed, rc 0 (on_beamlet 210 passed; on_beam 179 passed, 31 skipped) |
| full difftest | 527/527, 21 skipped by design, rc 0 |
| beamlet-footprint rv64 | PASS; beamlet heap peak 5,447 of 11,885 pages; stack 35,880 B of 18 pages |
| beamlet-footprint rv32 | PASS; beamlet heap peak 5,265 of 11,885 pages; stack 28,960 B of 18 pages |

No reruns. The first attempt's ./test-shell failure (the BEAM prompt's path wrapping a typed
line past 120 columns in a deep worktree) is gone: B40 runs driver_test in /.
