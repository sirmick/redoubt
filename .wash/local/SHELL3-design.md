# SHELL3: design checkpoint (before code)

Pages: docs/userland/beamlet.md "Screen natives"; docs/userland/shell.md "Full-screen programs";
the shell plan, section 4 (slices 1 and 2). Branch `wp-SHELL3`, worktree `.worktrees/SHELL3`, on
main `c144f28fc`.

## 1. The natives: `beamlet-screen`, module `redoubt_screen`

A new crate `userland/otp/screen` (`#![no_std]`, `#![forbid(unsafe_code)]`, depends on the VM and
`cells` only), exporting `NATIVES` as `beamlet-crypto` does, joined in the CLI's and
`beamlet-redoubt`'s native tables.

- **The buffer:** a resource holding `{width, height, owner: Pid, front: Vec<Cell>, back:
  Vec<Cell>, clear: bool}`; a cell is `{symbol: SmallString (<= 32 bytes), fg, bg, modifiers}`
  with `cells`' `Color` and `Modifiers`. `back` is what is being drawn, `front` what the last
  `diff` sent.
- `new(W, H)`, `resize(B, W, H)`, `put(B, X, Y, Text, Style)`, `fill(B, {X, Y, W, H}, Symbol,
  Style)`, `plot(B, Rect, Bits, Style)`, `width(Text)`, `diff(B)`, as the page's table says.
  `Style` is `{Fg, Bg, Modifiers}` with colours as `Redoubt.Term.Cells` spells them (`reset`,
  `{indexed, I}`, `{rgb, R, G, B}`). `diff` returns the `cells` frame's bytes (`cells::encode`)
  and makes `back` the new `front`.
- **Rules as the page states them:** a control character in `put`/`fill` is `badarg` (the
  `cells::forbidden` rule); 1024 a side, 65,536 cells; `put` reads at most the row's cells of its
  text; `width/1` at most 64 KiB; one pass a call; charged in reductions by cells and bytes
  touched (`bump_reductions`-style through `Ctx`); at most four buffers a process; only the owner
  may call (else `badarg`).
- **Counted toward the owner's heap limit** needs a small VM change: a resource may be made with a
  declared size (`Ctx::new_resource_sized(value, bytes)`), which counts in the heap that holds the
  term as a refc binary's bytes do (`offheap_bytes`). A buffer declares its two grids' bytes.
- **One width table**, generated, not hand-written: a generator reads OTP's pinned
  `unicode_util.erl` (Unicode 16.0, `is_wide_cp/1`, 123 ranges) and writes `screen/src/wide.rs`;
  a grapheme is 2 columns if it starts with a wide code point or carries U+FE0F, else 1, as
  OTP's `is_wide/1` decides. `Redoubt.Term.Width` then uses `:unicode_util.is_wide/1` itself
  (the same Erlang on both VMs), so the natives, the shell's width module and OTP's line editor
  measure alike; a vectors file holds the natives to the Elixir side.

## 2. On the BEAM: the same buffer in Elixir

The shell's suite must pass on the BEAM, which has no `redoubt_screen`. `Redoubt.Term.Buffer` is
the one API the shell uses; it calls the natives when `redoubt_screen` is loaded and an Elixir
implementation of the same rules otherwise. One `screen/vectors.json` (operations and the frame
bytes expected after each) holds the Rust natives (`cargo test -p beamlet-screen`) and the Elixir
buffer (ExUnit, on both VMs) to the same answers, as `cells/vectors.json` does for the decoders.
On beamlet the suite runs the natives; on the BEAM the Elixir buffer. **Question (a):** this, or
screen tests on beamlet only?

## 3. `Redoubt.Screen` and its layout

- **The behaviour:** `init(args) -> state`, `update(event, state) -> {:cont, state} | {:halt,
  value}`, `view(state, buffer, size) -> :ok`. Events: `{:key, key, modifiers}`, `{:resize,
  cols, rows}`, any other message.
- **`Redoubt.Screen.run(module, args)`**, from the line's process: spawns the screen's process
  (with the evaluator's `max_heap_size`), asks the shell's driver for the console (found through
  the group leader, as `group` answers `driver_id`), and returns the halt value, or `nil` when the
  interrupt ends it. The screen process: `new` buffer at the console's size, `view`, `diff`, sends
  the frame; then each event: `update`, `view`, `diff`, frame.
- **The driver, while a screen is in front:** enters the alternate screen and hides the cursor;
  decodes keys (below) and sends them to the screen; holds `group`'s output; decodes each frame
  with `Redoubt.Term.Cells.decode/1` (the strict decoder) and draws it with the encoder; on the
  end, leaves the alternate screen, shows the cursor, and draws what it held and the line again.
- **Layout:** pure functions over `{x, y, w, h}`: `split(rect, :rows | :cols, [constraint])`
  with `{:fixed, n}`, `{:percent, p}`, `:rest`; `centre(rect, w, h)`; `inset(rect, n)`.
- **Widgets for `pick` only** (the rest is SHELL5): `box` (border in box drawing, a title, a
  shadow), `list` (rows from an offset, the selection reversed, a scroll mark), `status` (a line).
  All text a widget draws goes through `Redoubt.Term.Text.visible/1` first, then `put`; so a
  hostile item is drawn as `^[...`, and a widget that forgot would raise `badarg`, not draw.
- **The encoder** gains `frame/2`: from a decoded frame, `CSI row;col H` per run of cells, SGR
  for colour (24-bit and indexed) and the attributes, `CSI 2J` for a clear, and the alternate
  screen and cursor visibility around a screen. The test terminal model learns exactly these.

## 4. Keys

`Redoubt.Term.Keys.decode(bytes)` -> `{[key], rest}`: printable graphemes, Enter, Tab,
Shift+Tab, Backspace, Esc, the arrows, Home, End, PgUp, PgDn, Delete, Insert, F1-F12, Ctrl+letter,
Alt+key, and the xterm modifier forms (`ESC [1;5C`); VT100, xterm and Linux console spellings. A
lone ESC waits 50 ms for more before it is Esc (a driver timer). Used only while a screen is in
front: `edlin` keeps decoding at the prompt. Bracketed paste is not in this package.

## 5. `pick(items)`

A commandlet (area "Screens"): a box centred, as wide as the longest item and as tall as the
screen allows, the list in it, a status line `↑↓ move · Enter choose · Esc leave`. Up/Down,
PgUp/PgDn, Home/End move; Enter returns the item; Esc returns `nil`. Items: any terms, shown as
`to_string` for strings and `inspect` otherwise, through `Text.visible`.

## 6. The interrupt while a screen is in front

**Question (b):** the page's Open item, the session's own key in a full-screen program. Until a
screen takes Ctrl+C as a key (none in this package; the editor will), Ctrl+C ends the screen with
`nil`. Recommendation for the reserved key, when one is needed: Ctrl+\ (0x1C), unused by edlin and
by common TUIs. I would leave the Open item open and say Ctrl+C ends a screen today.

## 7. Tests

- **Rust:** `cargo test -p beamlet-screen`: the vectors; refusals (a control character, an
  oversized buffer or text, a fifth buffer, another process's call); clipping of wide graphemes
  at the edge; `diff` sending only what changed and everything after `resize`; the width table
  against vectors.
- **VM:** a resource's declared size counts in its holder's memory, and a loop of `new/2` is ended
  by the heap limit.
- **ExUnit, on both VMs:** the Elixir buffer against the vectors; layout; the key decoder; the
  encoder drawing frames on the terminal model (extended); `Redoubt.Screen` with a test screen;
  `pick` through the whole stack (driver, group, screen) driven by keys and judged on the model:
  choose, leave, scroll, hostile items drawn visibly, Ctrl+C ending it, the prompt restored after.
- **A pty test** in `./test-shell`: `./shell`'s beamlet on a pseudo-terminal, `pick(["a", "b",
  "c"])`, Down, Enter: the line prints `"b"`, the alternate screen was entered and left, and the
  terminal's settings are back. A small Rust test in `userland/native` (rustix's pty), run by
  `./test-shell` with the beamlet and code path it builds.

## 8. Pages

beamlet.md "Screen natives" built (host). shell.md "Full-screen programs" split: built
(`Redoubt.Screen`, layout, `pick`, a screen's life, what it cannot draw) and planned (the other
widgets, focus, themes, a native program's screen, the reserved key with its Open item). The
terminal library's planned section gives up what is built (frames through one decoder, colour and
attributes, the key decoder without paste, the alternate screen).

## 9. Tier

The plan marks `beamlet-screen` and its natives `*`: Tier A review (red team) for the crate, the
VM's sized resources, and the encoder's frame path.
