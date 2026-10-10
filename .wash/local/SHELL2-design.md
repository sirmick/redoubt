# SHELL2: design checkpoint (before code)

## 1. Structure

- **`Redoubt.Shell.Driver`** (new, `userland/shell/lib/redoubt/shell/driver.ex`): one process.
  It subscribes to the console (`:beamlet.console_subscribe/0`; in tests and on the BEAM the
  input arrives as `{:beamlet_console, bytes}` messages from the test), starts OTP's `group`
  with `{Redoubt.Shell, :run, [opts]}` as its shell (group becomes the shell's group leader, so
  `Redoubt.Shell.run/1` and every existing test stay as they are), answers group's queries
  (`tty_geometry` from the console's size, `get_unicode_state` true, `get_terminal_state`), and
  hands every draw request (`put_chars_sync`, `insert_chars`, `delete_chars`, `move_rel`,
  `move_line`, `move_combo`, `put_expand`, `move_expand`, `redraw_prompt`, `new_prompt`,
  `delete_line`, `delete_after_cursor`, `beep`, `requests`) to `Redoubt.Term`.
- **`Redoubt.Term`** (new, `lib/redoubt/term.ex`): the encoder. It owns the edit-line model
  (prompt, text before and after the cursor, wrapping at `cols`, as `prim_tty` keeps it) and is
  the one module holding escape literals; every grapheme it draws goes through
  `Redoubt.Term.Text`'s rule (`control?/1`, caret and `<U+XXXX>` forms), widths from
  `Redoubt.Term.Width`. group's one sequence of its own, the bold `search:` prompt
  (`group.erl:918`), is matched exactly at `redraw_prompt` and drawn as bold; any other byte in
  any request is drawn through the guard.
- **Keys.** The driver reads two keys itself: 0x03 → `exit(group, :interrupt)` (group answers
  the pending read `{:error, :interrupted}`, the shell prompts again, the line is dropped, the
  session stays); 0x04 on an empty edit line → `eof` to group (the shell ends, as the pages say).
  Everything else goes to `edlin`, which decodes its own keys (arrows, Ctrl+R, Alt+F...).
- **History:** group's in-session list; the driver caps it (edlin's stack is otherwise
  unbounded), and nothing is saved.

## 2. Raw mode (the CLI only, `userland/otp/cli/src/main.rs`)

On `console_listening(true)` with a tty on stdin, the saved termios go in a static and the tty
gets ICANON, ECHO, ISIG, IEXTEN, ICRNL and IXON off; OPOST is kept, so a writer outside the
encoder (OTP's logger, `standard_error`) still lands on the host, while the encoder emits CR LF
itself as it must on Redoubt. Restored by: the guard's `Drop` at the end of `main` (normal and
error results), a panic hook that restores before the default hook runs, and SIGTERM, SIGHUP
and SIGINT handlers that restore (`tcsetattr` only) and re-raise with the default disposition:
one documented `unsafe` for `libc::sigaction` (rustix has no safe one; the CLI is under no
unsafe budget and has no `unsafe` today). `console_size` from `tcgetwinsize`.

## 3. Tests

- **G1:** `cargo test -p beamlet`, `userland/otp/cli/tests/tty.rs`: a pty (`rustix::pty`),
  beamlet on it with a fixture module (`.erl` + `.beam` checked in, as `vm/tests` does), and the
  slave's termios equal to the original after a normal exit, an error exit and SIGTERM; the
  panic path as a unit test of guard + hook under `catch_unwind` on a pty.
- **G2:** ExUnit in `userland/shell`. `test/support` models the screen by interpreting the
  encoder's output (a grid and a cursor); tests send `{:beamlet_console, bytes}` to the driver
  and judge the model: editing, history, Ctrl+R, the interrupt, Ctrl+D, hostile text through
  `put_chars`, a wide character. A disagreement between BEAM and beamlet is reported as a beamlet
  bug.

## Questions

- (a) Resize over SIGWINCH needs a VM change (`ConsoleInput::Resize` → `{:console_resize, c, r}`),
  outside my paths: I defer it unless told otherwise.
- (b) beamlet.md says a second `console_subscribe` is refused; the VM's BIF says the last caller
  wins (`vm/src/bif/info.rs:899`). Implement the refusal (a few lines in the VM) or leave the
  claim as a residual on the page?
- (c) OPOST kept, as above, or full raw as the page's wording suggests?
