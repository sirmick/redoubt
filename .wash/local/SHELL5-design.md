# SHELL5 design checkpoint: the widgets on Redoubt.Screen

Branch wp-SHELL5 off origin/shell (= main 36d1450f9), worktree .worktrees/SHELL5. No code yet.

## Shape

Widgets stay functions over plain data, never processes (docs/userland/shell.md, "Full-screen
programs"). A non-interactive widget is one function in `Redoubt.Screen.Widgets`, beside
box/list/status. An interactive one is a module under `lib/redoubt/screen/widget/` with a struct
and three functions:

    new(opts)                         -> widget
    key(widget, {:key, k, mods})      -> {:cont, widget} | {:done, value, widget} | :pass
    draw(widget, buffer, rect, theme, focused?)  -> :ok

`:pass` hands the key back to whatever holds the widget (focus ring, dialog stack, screen), so a
screen's `update` stays the one place state changes. Nothing is added to `Redoubt.Screen.run`.

## The widget set

| Widget | Module | Keys |
| --- | --- | --- |
| Text input, with a cursor | `Widget.Input` | graphemes insert; Left/Right, Home/End (Ctrl+A/E), Backspace, Delete, Ctrl+U, Ctrl+K; Enter -> {:done, text}; scrolls sideways by columns |
| List / checklist / radio list | `Widget.List`, `select: :none \| :one \| :many` | arrows, PgUp/PgDn, Home/End; Space toggles (many) or marks (one); Enter -> {:done, ...} |
| Buttons (a row) | `Widget.Buttons` | Left/Right, Enter or Space presses -> {:done, id} |
| Menu bar with drop-downs | `Widget.MenuBar` | F10 or Alt+hotkey opens; Left/Right between menus, Up/Down in the drop-down, Enter -> {:done, {menu, item}}, Esc closes; separators |
| Table | `Widget.Table` | header row bold, columns sized by `Width` as `table` does (shared helper extracted from `Redoubt.Shell.Table`), vertical scroll, optional selection |
| Braille canvas | `Widget.Canvas` | none: a dot grid of 2×4 per cell, `set/line/points`, drawn with the existing native `Buffer.plot` |
| Completion pop-up | `Widgets.popup/5` | a list box anchored under (or, without room, above) a point; the widget only: the prompt's completion stays group's list (SHELL4) |
| Focus ring | `Redoubt.Screen.Focus` | Tab / Shift+Tab move along named widgets; other keys go to the focused one |
| Modal dialogs | `Redoubt.Screen.Dialogs` | a stack; keys go to the top dialog, else `:pass` to the screen; `message`, `confirm` (yes/no -> boolean), `prompt` (Input + OK/Cancel -> text or nil); Esc closes with nil |
| Theme | `Redoubt.Screen.Theme` | a map of roles (normal, selected, border, title, shadow, menu, menu_selected, hotkey, button, button_focused, input, dialog, status) to styles; `plain` (today's look, the default, so pick is unchanged), `qbasic`, `menuconfig` |

Resize: the dialog stack and every widget are laid out in `view` from the size it is given, so a
resize lays them out again top to bottom with no extra state. `pick` is rebuilt on `Widget.List`
(behaviour and its tests unchanged). Optional, if you want it: `plot(values)`, a commandlet
drawing a series on the canvas as a screen, the end-to-end user of the canvas (plan section 4,
slice 3 names it).

The Esc timeout is already built (the driver's 50 ms, SHELL3); nothing to do.

## Drawing and escaping

- Every cell's text goes through `Widgets.label/5` -> `Text.visible/1` -> `fit`, never
  `Buffer.put` with raw text; fill symbols are constants in the source. Items, titles, menu
  labels, table cells, dialog messages and typed input alike.
- Input stores what was typed and draws it visible; the cursor's column is measured on the
  visible text of what precedes it, so a typed control (only reachable as a Ctrl key, which Input
  does not insert anyway) cannot misplace it.
- The cursor is drawn as a reversed cell. The terminal's own cursor stays hidden: showing it
  would change `Frame`/the driver, the drawing path, which I am not touching.
- Canvas hands `Buffer.plot` bytes, no text.

## Strict split

None needed as designed: no change to userland/otp, `Redoubt.Term.Text`, `Frame`, the encoder,
the driver, or anything with the session's authority (no file listing, exec, budgets). If one
turns up I stop and ask. Consequence: no new pty test (shell_pty.rs is under userland/otp).

## Tests (./test-shell: BEAM + beamlet)

- Key handling of each widget, focus ring and dialog stack: pure, run on the BEAM and beamlet.
- Drawing: draw into a `Buffer`, `Buffer.diff` -> `Cells.decode` -> `Frame.draw` ->
  `Test.Terminal`, judged on the model's text and styles (beamlet only, as screen_test).
- Hostile text: control characters and bidi controls in a menu label, a list item, a table cell,
  a dialog message and typed input are drawn as `^[`/`<U+202E>` and the model sees no sequence.
- End to end: one driver-level test (as screen_test.exs) of a test screen with a menu bar that
  opens a confirm dialog, Tab through its buttons, Enter returns the value; plus a resize under
  an open dialog.

## Docs

docs/userland/shell.md: "Widgets, focus, themes and a native program's screen" splits; widgets,
focus and themes go to built with their tests in the status line; a native program's screen and
the session's own key stay planned with their Open item. "Full-screen programs" bullet on
widgets updated. Summaries to check: docs/plan/m2-usable-shell.md, GETTING-STARTED.md, README.md,
the shell plan's slice list (local).

## Questions

1. The set and API above: OK?
2. `plot(values)` commandlet: in or out?
3. Completion pop-up as a widget only, not wired to the prompt: OK?
4. SHELL4 is unmerged on `shell`; I branch from origin/shell and touch only `widgets.ex`, `pick.ex`
   and `table.ex` among its neighbours (SHELL4 touches none of the three), and rebase when it lands.
