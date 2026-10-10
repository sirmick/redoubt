# SHELL5 report: the widgets on Redoubt.Screen

Branch `wp-SHELL5` (worktree `.worktrees/SHELL5`), one commit `1ccc914fe` on `origin/shell`
(b96e8b125, SHELL7 merged). It is for merging into `shell`, not main. Not pushed. The latest
state is the last section, "Rebase onto SHELL7"; the footprint section below it describes the
registry prefix skip that the rebase dropped.

## Delivered

`userland/shell/lib/redoubt/screen/`:
- `theme.ex`: roles to styles; `plain` (today's look, the default), `qbasic`, `menuconfig`;
  `get/1` takes one of those three atoms and raises on anything else.
- `widget/list.ex`: a list, a radio list (`select: :one`) and a checklist (`select: :many`).
- `widget/input.ex`: a one-line input; the cursor is a cell in `:cursor`; scrolls sideways.
- `widget/buttons.ex`, `widget/menu_bar.ex` (F10, Alt+hot key, modal while open, separators),
  `widget/table.ex` (header, selection, scrolling), `widget/canvas.ex` (Braille dots on
  `Buffer.plot`).
- `focus.ex` (Tab and Shift+Tab ring) and `dialogs.ex` (message, confirm, prompt; a modal stack;
  Esc gives nil; laid out from the size it is given).
- `widgets.ex`: `box` takes a shadow style; `status` takes a style; `label` returns the columns
  it drew; new `popup` (the completion pop-up, widget only) and `wrap`. `list/6` is gone:
  `pick` now draws with `Widget.List` and looks the same.
- `shell/table.ex`: `cells/1`, `widths/1` and `row/2` are public, so the table widget lays out
  as `table` does (imports at the prompt are `only:` commandlets, so nothing new is imported).

The API in every interactive widget: `key(w, key) -> {:cont, w} | {:done, value, w} | :pass`
and `draw(w, buffer, rect, theme, focused?)`.

## The prompt's footprint (gate before the merge into `shell`)

`make -f scripts/jobs.mk prebuilt rv64/beamlet-footprint rv32/beamlet-footprint`, run in the
worktree with the RustSBI paths exported. Results are below; the cap is 11,885 pages, and the
limit is half of it, 5,942.

| Tree | rv64 peak | rv32 peak | Modules at the prompt | Verdict |
| --- | --- | --- | --- | --- |
| First commit (`0101fb278`) | 6,008 | 5,806 | 128 | rv64 FAIL (66 pages over), rv32 PASS |
| Final commit (`ae06f7e58`) | 5,895 | 5,695 | 118 | both PASS |

The orchestrator's rv64 baseline is 5,905, so the final commit leaves 47 pages of headroom.

At the final commit, the VM's own report on rv64 is: footprint total 4,307 pages, runtime
held_now 4,550, peak 4,986. On rv32: 4,233, 4,362 and 4,802.

The fix:
- Theme, Focus and Dialogs moved under `Redoubt.Screen.Widget.`.
- The registry's existing skip of `Redoubt.Wire.` became a prefix list,
  `["Elixir.Redoubt.Screen.Widget", "Elixir.Redoubt.Wire."]`. That covers `Widgets` and every
  `Widget.*`, so `Widgets`, which was loaded at start before this package, is off the start
  path too.
- `pick` is still loaded at start because it is a commandlet. It references the widgets only
  when it runs.
- New test `registry_test.exs` "the modules the registry does not load to ask declare no
  command": every module under those prefixes, once loaded, has no `__commandlets__`.
- This is a one-line touch in SHELL7's file; SHELL7's lazy loading supersedes it.

## Strict split

None. No change under `userland/otp`, to `Redoubt.Term.Text`, `Frame`, the encoder or the
driver, and nothing that acts with the session's authority. The text input's cursor is drawn as a
cell, so the terminal's cursor stays hidden. There is no new pty test (`shell_pty.rs` is under
`userland/otp`).

## Styles come only from code (the orchestrator's check)

Widgets take a style only from a theme role. Text goes through `Widgets.label` and then
`Text.visible`. Tests:
- `drawing_test.exs` "every widget draws a control character visibly, in its role's style and
  no other". In qbasic, the list, table, input, buttons, open menu and status are each given
  `"\e]52;c;aGk=\a\e[1;31m‮evil"`. Each shows it as `^[]52;...^G^[[1;31m<U+202E>evil`, and
  the cell under `evil` has exactly the role's style: no red, no bold.
- "a dialog's title and text are drawn visibly in the dialog's style" (menuconfig).
- "control characters typed into it are drawn visibly, and the cursor stays after them".
- `widget_test.exs` "every theme has a style for every role, and only the built names are
  themes". `Theme.get(:"\e[31m")` and `Theme.get("plain")` raise.

The terminal model (`test/support/terminal.exs`) raises at any sequence the encoder does not
write, so every drawing test is an injection check too.

## Tests run

- Final commit, `q run --cores 8 -- ./test-shell` (whole): exit 0. 193/193 on beamlet; 166
  passed and 27 skipped on the BEAM; every stage ok, the fake kernel included. doccheck: exit 0.
- First commit, `q run --cores 8 -- ./test-shell` (whole): exit 0. 192/192 on beamlet, 165 passed and 27
  skipped on the BEAM; formatting, native, entry point, terminal (pty) and fake kernel all ok.
- After a final small change (a guard on the drop-down and pop-up size, and the hot-key draw
  tidied), the same command exited 1. Every stage passed except `on_fake_kernel`, where
  `beamlet-redoubt`'s `console` test `an_end_of_input_already_waiting_ends_the_idle_that_takes_it`
  failed (Eof vs Nothing). That Rust test is untouched by this package, and it also failed once
  in a partial run earlier. Rerun alone, 5 times:
  `q run --quiet -- cargo test -q -p beamlet-redoubt --features fake --test console` passed
  5/5 (exit 0). So it is a timing flake under load. It may deserve its own bug: it failed 2 of
  4 times beside other work.
- `q run --cores 4 -- cargo run -q -p redoubt-doccheck`: exit 0.
- `mix format --check-formatted`: clean.

New tests:
- `test/redoubt/screen/widget_test.exs`: 24 pure key tests (BEAM and beamlet).
- `test/redoubt/screen/drawing_test.exs`: 17 drawing tests (beamlet).
- `screen_test.exs` "a screen of widgets: the menu bar opens a prompt, Tab moves to its buttons,
  a confirm ends it". This one goes through the real driver: F10, then Enter, typed text, Tab,
  Enter, then Alt+F, Down, Enter, and Yes.
- The existing pick tests pass unchanged.

## Docs checked

- `docs/userland/shell.md`:
  - "Widgets, focus, themes and a native program's screen" is split. "Widgets, focus and
    themes" is now built, with its status naming the three test files. "A native program's
    screen and the session's key" stays planned with its Open item.
  - "What is not built" names `plot(values)` (as asked) and a live resize reaching a screen in
    front. The driver only sends the first size, and `Redoubt.Screen` has no later resize; it
    waits on the console's `resize`.
  - The "Full-screen programs" widgets bullet and its status line are updated, and the
    interrupt section's link points at the new anchor.
- `docs/plan/m2-usable-shell.md`: the step 4 and native-screen links now use the new anchors,
  and Progress lists the widgets.
- `GETTING-STARTED.md`: one sentence on building screens from `Redoubt.Screen.Widget`.
- `README.md`: it does not mention screens, so no change.
- `docs/userland/beamlet.md:754`: "widgets, layout and focus are Elixir" is still true, so no
  change.

## Rebase onto SHELL4

SHELL4 touches `screen.ex`, `shell.md` and `m2-usable-shell.md`, not my code files. Conflicts
are expected only in the docs:
- `shell.md`: the "Full-screen programs" bullets next to its pick/pager bullet.
- `m2-usable-shell.md`: Progress, and the step 3 link.

## Risks and notes

- A dialog keeps any key it does not take, so the screen under it gets none until it closes.
  This is by design.
- `MenuBar` is modal while open.
- A screen draws the menu bar last, so a drop-down covers the dialogs. This is documented.

## After the red's review (head `9b76ff3f9`)

The fold: `pick` takes a key with any modifiers held, as the old `pick` did. Alt+Esc leaves,
Shift+Down moves, Ctrl+Enter chooses. The test is widget_test.exs "pick: a key counts with any
modifiers held".

The branch was rebased onto `origin/shell` 41c9cbef3 (SHELL9) with no conflicts.

Results on the rebased head:
- `./test-shell` (whole): exit 0. 195/195 on beamlet; 168 passed and 27 skipped on the BEAM;
  every stage ok.
- `beamlet-footprint`: rv64 PASS at 5,894 pages, rv32 PASS at 5,695 (limit 5,942), with 118
  modules at the prompt.
- doccheck: exit 0.

## Rebase onto SHELL7 (head `1ccc914fe`)

`origin/shell` moved to b96e8b125 (SHELL7: commands load when first called, from a compile-time
index). The branch was rebased onto it and the one commit amended. Per the ruling, SHELL7's lazy
loading is kept and SHELL5's registry prefix skip (`Registry.declare_none`, the prefix list in
`commandlet/registry.ex`) is dropped with what only it needed: `registry_test.exs` "the modules
the registry does not load to ask declare no command", and the report's claim about it. The
commit touches 18 files, none of them `registry.ex`; `grep declare_none` finds nothing. The
widgets stay out of the prompt because they declare no commands, so SHELL7's index leaves them
out; the page and the commit message say so.

Gates on `1ccc914fe`, every command through q in the worktree:
- `q run --cores 8 -- ./test-shell` (whole): exit 0. 200/200 on beamlet; 172 passed and
  28 skipped on the BEAM; formatting, native, entry point, terminal (pty) and the fake kernel
  all ok. (Up from 195 and 168+27: SHELL7's tests are in.)
- `q run --cores 4 -- cargo run -q -p redoubt-doccheck`: exit 0, no findings.
- `make -f scripts/jobs.mk prebuilt` (rv64 236 cases, rv32 222, 0 failed), then
  `rv64/beamlet-footprint rv32/beamlet-footprint`: both PASS (limit 5,942).

| Tree | rv64 peak | rv32 peak | Modules at the prompt |
| --- | --- | --- | --- |
| SHELL7 alone (`origin/shell` b96e8b125, SHELL7's report) | 5,432 | 5,253 | 90 |
| SHELL5 on it (`1ccc914fe`) | 5,433 | 5,253 | 90 |

The VM's own report on rv64: footprint total 3,867 pages, runtime held_now 4,088, peak 4,490;
on rv32: 3,802, 3,918 and 4,326. Logs:
`target/testbench/run-1009896-1791481395632419120/beamlet-footprint-rv64-smp1.log` and
`run-1009899-1791481395634322642/beamlet-footprint-rv32-smp1.log` in the worktree.

So the peak rises by one page on rv64 and not at all on rv32, with the same 90 modules at the
prompt: no widget module is loaded at the prompt that was not before. The one page is not
traced to a cause; with the module count unchanged it is not a module loaded.
