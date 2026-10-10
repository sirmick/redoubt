# NSCR1 design checkpoint: a native program's screen

M2 "On Redoubt" step 5. Base: main a72e774b8 (JOB1 merged). No code written.

Read: docs/plan/m2-usable-shell.md (goal; attack lines "A screen writes no control sequence",
"The screen natives hold under hostile arguments", "No program swallows the interrupt");
docs/userland/shell.md "A native program's screen and the session's key", "The cell protocol",
"Full-screen programs", "Interrupting and killing jobs", "Native programs and pipes", "The
console's size"; docs/userland/native.md "Standard input and output, and pipes", "Killing a job";
docs/userland/beamlet.md "Screen natives"; userland/native/cells (lib.rs, tests.rs, vectors.json);
userland/shell/lib/redoubt/{term/cells.ex, term/frame.ex, term/keys.ex, term/buffer.ex,
pipeline.ex, jobs.ex, job.ex, screen.ex, shell/driver.ex, shell/session.ex (exec, pipe)};
tests/beamlet-programs/src/bin/pipe-stage.rs; tests/{job-interrupt-native,pipe-hostile-output}.toml,
tests/data/pipe/boot.toml; .wash/local/{PIPE1,JOB1}-design.md.

## 0. Shape in one paragraph

A native screen is an ordinary one-stage foreground pipeline whose two session-side ends are held
by a **screen host**: one Erlang process in the session's VM, `Redoubt.Screen.Native`, that is the
screen the driver sees (it speaks the driver's existing `{:redoubt_screen, ...}` protocol, exactly
as `Redoubt.Screen`'s process does). The host reads the program's stdout as length-prefixed
`cells` frames, decodes each with `Redoubt.Term.Cells.decode/1` (the one decoder), checks it
against the screen it gave the program, applies its cells to a screen buffer of its own
(beamlet's natives), and at most 30 times a second hands the driver the buffer's diff, which the
driver decodes again and draws with `Redoubt.Term.Frame` (the one encoder), unchanged. Keys and
the screen's size reach the program as input records on its stdin. The driver's code does not
change: Ctrl+\ is found in the raw bytes before any decoding and kills the host, which takes the
job's stage budget with it, as an interrupted line takes its pipelines.

```
program --stdout pipe--> [sink reader] --chunks, acked--> HOST --diff (cells frame)--> driver --Cells.decode, Frame.draw--> /dev/cons
program <--stdin pipe--- [feeder]      <--input records-- HOST <--{:key,..},{:resize,..}-- driver <-- raw bytes (0x1C / 0x03 stripped first)
```

## 1. How a native program declares a screen: it does not; its caller does

- The line asks for it: `screen(name, args)`, a session command beside `exec` and `pipe`
  (`Redoubt.Screen.Native.run(name, args, opts)` underneath, `opts` `ctrl_c: :interrupt | :key`
  as `Redoubt.Screen.run/3`'s). The same program run with `exec` or `pipe` has its stdout drawn as
  text through the guard, as today.
- Not a first frame, not a served file: sniffing the first bytes would let a program's output pick
  the mode its own output is read in; a served file would be a fourth namespace entry. With the
  caller choosing, a program's stdout is either text through the guard or cells through the
  decoder, never something it chose.
- Ctrl+C as a key is the caller's option, never the program's: no byte from the program changes
  what the interrupt is.

## 2. Reading frames: framing and bounds

**Framing (both directions): a record** is `u32 length` (LE) then exactly `length` bytes. On
stdout each record's body must be exactly one `cells` frame (the decoder's own "exactly its
bytes" rule). The frame format itself does not change, so vectors.json's frames and both frame
decoders stand; the record is new protocol in the `cells` crate (Rust `record(&Frame)` for
programs; Elixir `Cells.record/2`-style splitting in the host), with its own vectors (section 3).

**Checks, in order, each a refusal that ends the screen (never repaired):**
1. `length` > the bound for the screen the program was given (`12 + cols*rows*(15+32)`, i.e.
   every cell with a 32-byte symbol): refused `:length` **before any of the body is held**, so a
   header claiming 4 GiB costs nothing.
2. The body through `Cells.decode/1`: any of its errors (`:symbol` for a control character in a
   symbol, `:position`, `:twice`, ...).
3. The frame's size against the screen's: equal to the size last given, drawn; equal to a size
   given before a resize the program has not yet answered, **dropped** (counted, not drawn, not
   an error: a frame in flight when the terminal changed size); any other size refused `:size`.
   So "a cell outside its screen" is the decoder's `:position` against a size the session gave.
4. Each cell must be one cell's worth as the session lays it out (the decoder leaves this to "the
   session's width tables"): its symbol exactly one grapheme (`String.graphemes/1`, OTP's
   segmentation, as `put` uses), of width 1 or 2 (`Redoubt.Term.Width`, OTP's table, the buffer's),
   and a wide one not in the last column. Otherwise refused `:cell`. Without this a 32-byte
   symbol `XXXX...` at column 79 would draw 32 columns from one cell, past the screen's edge.
5. Applied with `Buffer.put(buffer, x, y, [symbol], style)` and `clear` as a full blank fill. The
   natives then hold the same rules every screen's buffer holds (they would `badarg` a control
   character; checks 2 and 4 mean they never repair anything here).

**Per second:** the host acks each stdout chunk the sink reader hands it (4 KiB reads), and
acks at most 1 MiB a second: past that the reader waits, the pipe's one page fills and the
program's write parks (piped's backpressure). It draws at most 30 frames a second: frames
arriving faster are applied to the buffer as they come, and one diff goes at the next tick, so a
flooding program costs the driver and the terminal at most 30 coalesced frames a second.
(1 MiB/s and 30/s are first numbers; I will measure decode cost per KiB on beamlet on both widths
and report them, and lower the byte rate if one second's decoding starves the prompt.)

**Memory:** the host runs with its caller's heap limit (the evaluator's share), as a
`Redoubt.Screen` does; the buffer and a record's body count toward it. A record in progress is at
most check 1's bound. Stderr is kept, not drawn, while the screen is in front: 64 KiB, the rest
counted (`Pipeline`'s `{:kept, n}` path), drawn through the guard after the screen ends. (The
driver's `held` list for group output is unbounded today; a native screen adds nothing to it.)

## 3. Keys and size to the program: input records on its stdin

Each event is one record (u32 length, body), body defined in the `cells` crate:

```
u8 kind   1 size: u16 cols, u16 rows            (the first event; again on each resize)
          2 key:  u8 modifiers (bit0 shift, bit1 alt, bit2 ctrl; no other bit)
                  u8 key: 0 = a symbol, then u8 length 1..=32 and a Symbol (the frame rule: no
                          control character); 1 enter, 2 tab, 3 backspace, 4 esc, 5 up, 6 down,
                          7 left, 8 right, 9 home, 10 end, 11 page_up, 12 page_down, 13 insert,
                          14 delete; 0x80+n Fn, n 1..=24
```

- The session encodes (Elixir, from `Redoubt.Term.Keys`' `{:key, key, mods}`); the program
  decodes (Rust `cells::decode_event`, strict, refusing as the frame decoder does). A new vectors
  file, `userland/native/cells/events.json`, written by the Rust tests (good events with their
  bytes, bad bytes with their reason, plus record splitting) holds the Elixir encoder to the Rust
  decoder's bytes; `vectors_are_current`-style check for it. vectors.json is untouched.
- Why events, not the raw bytes typed: the program gets exactly what an in-VM screen gets (the
  driver already decodes keys for screens), the size travels the same way, and the program needs
  no terminal parser. A key symbol is a Symbol, so 0x1C or ESC cannot be a key's text; Ctrl+\ is
  `{"\\", ctrl}` in Keys' terms and is never sent because the driver removes 0x1C before
  decoding; the host's encoder also refuses it (and 0x03 when Ctrl+C is the interrupt), so a
  future driver change cannot leak it.
- Keys the program does not read: the feeder acks each write; the host holds at most 64 KiB of
  unacked input and drops keys past it (counted). A paste of a megabyte cannot grow the host or
  the feeder's mailbox.

## 4. When frames stop, the program exits, or crashes

- **Frames stop, program alive:** the last frame stays; no watchdog (a program waiting for a key
  is normal). Ctrl+\ always ends it.
- **Program exits or faults** (its exit notice, through the pipeline's owner): the host closes the
  screen (driver shows the main screen as it was and draws what group held), the line gets
  `{:exited, code}` / `{:faulted, cause}` and, after the main screen is back, the kept stderr is
  drawn through the guard. A partial record at the end of stdout is not drawn.
- **A refused record:** the host kills the job (as `Job.kill`: the owner destroys the stage's
  budget), closes the screen, and the line gets `{:error, {:refused, reason}}`.
- **The host itself dies** (heap limit, a bug): it is linked to the pipeline's runner, whose owner
  watches it, so the budget is destroyed; the driver's monitor shows the main screen; the line
  gets `exit({:screen, reason})` as `Redoubt.Screen` does today.

## 5. Ctrl+\ (and Ctrl+C) ends it

Unchanged driver: the byte is found in the raw read before Keys.decode, so neither an escape
sequence begun before it, nor a bracketed paste, nor a UTF-8 sequence carries it to the host;
`interrupt_screen` sends the host `:interrupt` then `:kill`. The host runs the pipeline through a
linked runner process (`Redoubt.Pipeline.run/2` with two new modes, `input: {:records, host}` and
`output: {:records, host}`), so the kill takes the runner, whose owner (watching it, as it watches
an interrupted line) destroys the stage's budget. `Native.run` on the line sees `:interrupt`,
waits for `Redoubt.Jobs.settle/1` (the job whose caller ended; at most 2 s, saying so if not), and
returns `nil`. Ctrl+C does the same unless `ctrl_c: :key`. The job is listed by `jobs()` as a
foreground job while it runs; `Job.kill` from elsewhere ends it the same way.

## 6. What a hijacked program can do at most

Draw wrong cells inside the screen it was given; draw nothing for as long as it likes; send
frames at up to the rate bound; read the keys typed while it is in front (that is its job); write
64 KiB of stderr that is shown as visible text after; exit, fault or spin in its own budget. It
cannot: write a control sequence (decoder, then `Text.visible` in the encoder); place a cell
outside the screen, or make one cell draw more than its width (checks 3 and 4); hold the console
(no `/dev/cons`, stdin is the session's records); take Ctrl+\ (or Ctrl+C unless the caller
gave it); grow the session beyond the host's heap limit; outlive the screen.

## 7. The cases (both widths, UART console session, `tests/data/pipe/boot.toml` with the program)

The program: `screen-stage` in `tests/beamlet-programs` (where `pipe-stage` lives; the
assignment's `tests/programs` holds kernel-level programs on the raw wrapper, not stdio stages),
depending on `cells`. Modes: `draw` (a script: a box, coloured text, then echoes each key it is
sent as text on row 2, exits 0 on `q`), `spin` (draws once then spins, reading nothing), and the
misbehaviours below. Added to the pipe recipe's bundle and its manifest's `public` list. (Or a
`screen` mode of `pipe-stage`, if a second /boot entry is unwelcome: say which.)

1. **`nscr-interrupt`** ("a frame or key sequence smuggling 0x1C"; "No program swallows the
   interrupt"): `screen("screen-stage", ["spin"])` then Ctrl+\ sent inside a CSI (`ESC [1;5` 0x1C
   `A`); `draw` with a bracketed paste holding 0x1C; `draw` with `ctrl_c: :key`, Ctrl+C drawn by
   the program as a key, then Ctrl+\ ends it; a frame whose symbol holds 0x1C, refused `:symbol`.
   Verdicts from the session: each line's value (`nil`, `{:error, {:refused, :symbol}}`), and
   the session's budget usage read from the kernel back to what it held before, piped gone
   (`Redoubt.Pipes.usage()`), as job-interrupt-native judges.
2. **`nscr-hostile-text`** ("a program's text reaching the terminal unescaped"): `raw` writes
   OSC 52 / OSC 8 / title / ESC X straight to stdout, unframed (refused `:length` or `:version`);
   `esc-symbol` frames a symbol holding ESC (refused `:symbol`); `stderr` writes the same to
   stderr and draws a good frame (shown, after the screen, as visible `^[`). Verdict: the bytes
   the session wrote, forbidding each sequence raw (`\x1b\]`, `\x1bX`, `\x1b\[21t`), as
   pipe-hostile-output does.
3. **`nscr-beyond-cells`** ("a program drawing beyond its cells"): `big` (a frame of 200x100 on
   the 80x24 screen: `:size`), `outside` (x = 80: `:position`), `long` (a 32-byte symbol of 32
   `X`s: `:cell`), `wide-edge` (a wide symbol at column 79: `:cell`), `zero` (a lone combining
   mark: `:cell`), `claim` (a record length of 0xFFFFFFFF: `:length`). Verdicts: each line's
   value, and the bytes the session wrote: no cursor placement past row 24 or column 80, no run
   of `XXXXXXXX`.
4. **`nscr-draws`** (the behaviour): `draw` shows its box and text (expected as the encoder's
   CUP/SGR bytes), a typed key echoed, `q` ends it with `{:exited, 0}`, the main screen back.

Host tests (`./test-shell`, ExUnit on beamlet): the host with a fake transport (record splitting,
every refusal and its reason, stale frames after a resize dropped and a third size refused, the
1 MiB/s and 30/s bounds with an injected clock, input dropped past 64 KiB, stderr kept), the
event encoder against events.json. Rust: `cells` tests for records and events and
`events_are_current`. Resize on the machine is host-tested only (the UART has no size), unless
you want an SSH case.

## 8. Pages

- docs/userland/shell.md: "A native program's screen and the session's key" planned -> built
  (status with the four cases and the host tests; the bullets as built, with the bounds); the
  figure's dashed native path solid; "Full-screen programs" and "The cell protocol" (records,
  input events, events.json); "Native programs and pipes" (`screen` beside `exec`/`pipe`).
- docs/userland/native.md "Standard input and output, and pipes": a stage run as a screen.
- userland/native/cells/src/lib.rs module doc (records, events); the shell's `screen` help.
- docs/plan/m2-usable-shell.md Progress: step 5 built.
- Checked, expected unchanged: README.md, GETTING-STARTED.md, docs/userland/beamlet.md "Screen
  natives", docs/servers/piped.md, the security register (will grep for "screen" claims).

## 9. Questions for the orchestrator

1. "A key per principal" (a principal choosing another session key) is in the same section and
   planned. I propose to leave it planned: the section's status becomes "built · partly tested:
   ... a key per principal is not built". Or does NSCR1 build it?
2. `screen-stage` as a new /boot entry in tests/beamlet-programs (proposed), or a mode of
   `pipe-stage`?
3. Four boot cases on both widths (about 8 boots); I can fold `nscr-draws` into `nscr-interrupt`
   to save two boots if the queue is tight.
