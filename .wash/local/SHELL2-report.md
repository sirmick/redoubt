# SHELL2: the assignment result, in detail

Branch `wp-SHELL2`, head `50972be70`, on main `495dc205c`; 8 commits, never pushed. Worktree
`/home/mcloonan/redoubt/.worktrees/SHELL2`. Design: `.wash/local/SHELL2-design.md`, approved.

## Commits, in order

1. `fa29e714b` beamlet: the terminal is raw while the VM reads the console and restored on every
   way out. `cli/src/tty.rs` (new), `cli/src/main.rs`, `cli/Cargo.toml` (+libc, rustix),
   `cli/tests/tty.rs`, fixture `cli/tests/src/tty_probe.erl` + `.beam`. Raw on the first
   `console_listening(true)` with a tty on stdin: ICANON, ECHO, ISIG, IEXTEN, ICRNL, IXON, BRKINT,
   INPCK, ISTRIP off; OPOST kept. Restored by a `Guard` dropped at main's end, a panic hook, and
   SIGINT/SIGTERM/SIGHUP handlers (restore, then re-raise with SA_RESETHAND). **Unsafe budget:**
   the CLI is under no budget and had no `unsafe`; it gains two documented blocks, `sigaction`
   and `raise` (no safe wrapper is vendored or in rustix). Said in the commit.
2. `dcd838f09` beamlet: a second console subscription is refused while the first reader lives
   (`{error, busy}`; the reader's own repeat is ok; its exit frees the console). `vm/src/bif/info.rs`,
   `vm/tests/console.rs`, fixture `vm/tests/src/console.erl` + `.beam`.
3. `652762b1f` beamlet: the console's size reaches Erlang code (`beamlet:console_size/0`, asked
   afresh, `{Cols, Rows}` or `unknown`). beamlet.md's console section: built, its 8 tests folded.
4. `761c5b2c2` beamlet: kernel_safe_sup runs as on BEAM, so OTP's group starts (approved).
   `vm/lib/application.erl` + `.beam`, differential test `tests/erlang/kernel_safe.erl`, a
   sentence in beamlet.md.
5. `bc01ddca5` beamlet: prim_tty's width native answers enotsup (approved). One table line in
   `vm/src/bif/mod.rs`, differential test `tests/erlang/char_width.erl`, beamlet.md's sentence on
   combining marks.
6. `8fa7a5364` shell: the beamlet the shell's tests run is the host's, not the machine's
   (`userland/shell/setup.sh` build order; approved).
7. `15db53b47` shell: Redoubt.Term draws a line being edited, and what is printed beside it,
   through the guard. `lib/redoubt/term.ex`, `test/redoubt/term_test.exs` (17 tests), the terminal
   model `test/support/terminal.exs` (raises on any sequence the encoder does not write; takes
   bytes untranslated: the no-OPOST test), `test_helper.exs`, `mix.exs`.
8. `50972be70` shell: lines are edited under the shell's own driver, with history, and Ctrl+C
   ends a line. `lib/redoubt/shell/driver.ex`, `lib/redoubt/shell.ex`,
   `test/redoubt/shell/driver_test.exs` (14 tests), `test-shell`, `shell`, GETTING-STARTED.md,
   docs/userland/shell.md, docs/plan/m2-usable-shell.md, two beamlet.md links.

## Tests and gates (exact commands, all through q)

- `q run --cores 8 --tenant SHELL2 -- ./test-shell` at `99ffc6823` (same code as the head; the
  head differs by two heading levels in shell.md): exit 0; formatting ok, native ok, on_beamlet
  116 passed, on_beam 116 passed, entry_point ok, on_fake_kernel ok.
- `q run -- cargo test -q -p beamlet` (userland/otp), head: exit 0 (4 unit, 4 pty tests).
- `q run -- cargo test -q -p beamlet-vm`, head: exit 0 (incl. the 3 console tests).
- `q run -- userland/otp/tools/difftest erlang`: 42/43, exit 1. The one failure, `erlang/ports`,
  is pre-existing and not mine: a beamlet built from main's tree (`git archive main`) fails it
  identically (`{'EXCEPTION',error,enoent}`). Cause: on this host `/bin/echo` is a symlink into
  `/usr/lib/cargo/bin/coreutils/` (uutils), out of the test's `/bin` mount; spawn_executable of
  `/bin/echo` is refused by the sandbox. `kernel_safe` and `char_width` pass on both VMs.
- `make -f scripts/jobs.mk prebuilt` (rv64 229 cases, rv32 215, 0 failed), then `rv64/docs
  rv64/formatting rv64/size-budget rv64/unsafe-budget rv64/no-cruft`: all PASS, exit 0, head.
- Not run: the whole bench, rv32 cases beyond prebuilt (Tier B; the CLI and the VM are host
  code; prebuilt built rv32), mutation tests.

## Beamlet bugs found by the rule "passes on BEAM, fails on beamlet"

1. kernel_safe_sup never started: group_history:load/0 loops until it exists; group never served
   a line. Fixed (commit 4).
2. `prim_tty:wcwidth/1` absent: group died at the first prompt (undef). Fixed (commit 5).
3. `setup.sh` (not the VM): since `caa94b5f8` target/release/beamlet held the machine's binary.
   **Exit code: no silent pass.** The beamlet stage judges ExUnit's last line, so with no output
   it reported `on_beamlet: FAILED` and `./test-shell` exited 1 (seen in my runs before the fix);
   so no guard was added.

## Pages and summaries checked

- docs/userland/beamlet.md, "The console on a host": built, 8 tests (folded); the resize residual.
  Natives: what the application stand-in starts; the width fallback and combining marks.
- docs/userland/shell.md: "The terminal library" built (host; its planned parts moved to a sibling
  "Screens, keys and the console's size", planned, holding the Open list; resize residual in one
  sentence); "Hostile text never drives the terminal" built (residual: the line editor hands the
  encoder text, made visible under the same rule; the logger's crash reports pass it); "Line
  editing and history" built, figure solid/dashed, planned sibling "Saved history, secret reads
  and pasting" with the paste Open; "The loop": IO.puts now guarded, logger not.
  ExUnit tests are named by file in the status gaps, as the page's other built sections do (the
  checker's test forms are host:/bench: only).
- docs/plan/m2-usable-shell.md Progress: the first host step listed.
- GETTING-STARTED.md "The shell": line editing, history, Ctrl+C; completion not yet.
- `shell` script header updated. Checked, no change needed: userland/otp/README.md (no console or
  tty claim), docs/userland/README.md (line editing listed under M2, still planned overall),
  docs/SECURITY.md lines 246-252 (still true: code can open the console's fd itself), agents.md
  and m2 links to `#the-terminal-library` (still the right section).
- Found, not fixed (outside the assignment): README.md:31 says M2 brings "a command mode", which
  the shell plan's decision 7 and shell.md rule out.

## Residuals and risks

- Ctrl+C during an evaluation is dropped (no `^C`): ending an evaluation is G3's.
- A resize is seen at the next prompt only (deferred to SHELL3 per the answer).
- The encoder redraws the line from its start on each edit (not a cell diff; the diff comes with
  the screen buffer). A line taller than the screen is drawn but its editing is not exact.
- History's cut uses `:sys.replace_state` on group's record (field named via Record.extract, so
  an OTP that moves it fails the build); it runs from a spawned process, so the cut can land just
  after the next read starts: that read may see one line over the cap.
- The driver tests' screen helper waits for 300 ms of quiet, or (where a test names the screen it
  waits for) up to 10 s; a much slower beamlet could still race the default.
- A mistake of mine, corrected: one commit went in with a stale application.beam (I chained the
  check and the commit with `;`); it was regenerated, `build-lib --check` passes, and the fix is
  folded into that commit.

## Demo: ./shell on a 72x20 pseudo-terminal (python pty), typed keys and the screen

Driver: `.tmp/SHELL2/demo.py`; full output `.tmp/SHELL2/demo.txt`. The screen at the end, after:
`1 + 1⏎`, `x = [1,⏎`, `2]⏎`, `IO.puts("\e]52;c;aGk=\a\e[2J‮evil")⏎` (twice: once typed, once
from history), `Enum.sum(x)` Ctrl+B Ctrl+B Ctrl+B Ctrl+E `⏎`, ↑ ↑ `⏎`, `1 +` Ctrl+C,
Ctrl+R `sum` `⏎` `⏎`, `⏎` (an empty line), Ctrl+D:

```text
/ (1)> 1 + 1
2
/ (2)> x = [1,
...(2)> 2]
[1, 2]
/ (3)> IO.puts("\e]52;c;aGk=\a\e[2J‮evil")
^[]52;c;aGk=^G^[[2J<U+202E>evil
:ok
/ (4)> Enum.sum(x)
3
/ (5)> IO.puts("\e]52;c;aGk=\a\e[2J‮evil")
^[]52;c;aGk=^G^[[2J<U+202E>evil
:ok
/ (6)> 1 +^C
/ (6)> Enum.sum(x)
3
/ (7)>
/ (7)>
ok
```

The shell ended with exit status 0 (the terminal's restoration is the pty tests' claim; the demo
did not check it). Raw bytes after Ctrl+D are
`\r\nok\r\n`: the second `/ (7)>` is the empty line's own prompt; `ok` is beamlet's result line.
The renderer shows any sequence other than the encoder's as `<?>`; none appeared.
