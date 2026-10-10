# NSCR1 report: a native program's screen

Branch wp-NSCR1 in /home/mcloonan/redoubt/.worktrees/NSCR1. Base main a72e774b8, head d60fd03c9 (f8cfab47b plus a comment in each nscr toml naming B47 as the four-screen bound; comments only, cases not rerun; docs rerun rc 0). Four commits, each read in full by me before committing:

1. `cells: records, and the events a native program with a screen reads`
2. `shell: a native program's screen, its frames drawn by the session`
3. `tests: a native program's screen on the machine, on both widths`
4. `docs: a native program's screen, built`

Design: .wash/local/NSCR1-design.md (the orchestrator's go, 2026-10-09). Deviations are listed below.

## What was delivered

- **userland/native/cells** (Rust, no_std, no unsafe): `record`, `split` (refuses `Length`
  before reading the body), `max_frame(w, h) = 10 + w*h*47`, `Event` (`Size`, `Key` with a
  `Symbol` or a named key, F1-F24, shift/alt/ctrl), `encode_event`, `decode_event` (strict:
  `Kind`, `Key`, `Modifiers`, `Symbol`, `Size`, `Trailing`, `Truncated`), and `MAX_EVENT`. There
  is a new vectors file `events.json` (24 good events, 16 bad), held by `events_are_current`.
  `vectors.json` and the frame format are unchanged.
- **Redoubt.Term.Cells** (Elixir): `reduce/3` (the decoder a cell at a time; `decode/1` is built
  on it), `split/2`, `record/1`, `max_frame/2`, `event/1` (the session's
  encoder; it refuses a control character in a key's symbol, unknown keys and modifiers, and bad
  sizes). Its tests are held to events.json.
- **Redoubt.Screen.Native** (new): the host. It is one process, the screen in front for the
  driver, using the driver's existing protocol, and the driver's code is unchanged. For each
  record it checks, in order: the length bound before holding anything, the one decoder, the
  frame size (equal to the size given, or dropped if it is a size given before the last resize,
  otherwise `:size`), and that each cell is one grapheme and a wide one is not in the last column
  (`:cell`). Accepted cells are put into its own beamlet buffer, and the diff goes to the driver
  at most every 33 ms. The host acks reads at most 1 MiB a second, sends the size and keys as
  events (never Ctrl+\, and never Ctrl+C unless `ctrl_c: :key`), and drops input past 64 KiB
  unread. It keeps 64 KiB of stderr and draws it through the guard after the screen. A refusal
  kills the runner, so the owner destroys the budget; the line settles and returns
  `{:error, {:refused, why}}`. Frames are decoded and put a cell at a time (`Cells.reduce/3`).
  The interrupt kills the host, which takes the linked runner, and
  the line settles and returns `nil`.
- **Redoubt.Term.Frame.draw_bytes/1** and the driver: every screen's frame is drawn as it is
  decoded.
- **Redoubt.Pipeline**: new `{:records, host}` input and output modes. They are paced: the
  reader waits for `:more`, and it is killed at the job's end rather than waited for. Stderr is
  kept in this mode.
- **`screen(name, args)`**: a new session command.
- **tests/programs/src/bin/screen-stage.rs**: modes `draw`, `flood`, `stderr`, and the
  misbehaviours `raw`, `esc-symbol`, `key-symbol`, `big`, `outside`, `long`, `wide-edge`,
  `claim`. It is in the pipe recipe's /boot and its manifest's `public` list; the test-programs
  crate gains a `cells` dependency (Cargo.lock: +cells).
- **Cases**: nscr-interrupt, nscr-hostile-text and nscr-beyond-cells, on both widths.

## Attack cases and why their verdicts are the system's

Every verdict comes from one of three places: the line's value, which the session prints; the
session's budget usage read from the kernel (`c.() == before`) together with
`Redoubt.Pipes.usage()`; or the raw bytes the session wrote, judged by the bench's `forbid`.
None comes from screen-stage's own word.

- **nscr-interrupt** ("a frame or key sequence smuggling 0x1C", "No program swallows the
  interrupt"):
  - draw: the frame and a key are drawn. Ctrl+\ is then sent inside `ESC [1;5`; the value is
    nil and `A` reaches no one (forbid `key:(A|ctrl\+Up|d)`).
  - flood: a program writing whole frames non-stop is ended by Ctrl+\ inside a bracketed paste.
  - ctrl_c: :key — the program draws `ctrl+c`, and Ctrl+\ still ends it.
  - key-symbol: a frame with a 0x1C symbol is refused `:symbol`.
  - The budget is back to base after the first screen and after the last; forbid `\x1c` and
    `\x1b\]` in output.
- **nscr-hostile-text** ("a program's text reaching the terminal unescaped"):
  - raw (OSC 52, a title and ESC X sent as text) is refused `:length`.
  - claim (a 4 GiB length) is refused `:length`.
  - esc-symbol is refused `:symbol`.
  - stderr: the hostile bytes are drawn as `^[]52;...^G...^[X` after the screen, and the value is
    `{:exited, 0}`.
  - The budget is back to base; forbid `\x1b\]`, `\x1bX`, `\x07` and `XXXXXXXX`.
- **nscr-beyond-cells** ("a program drawing beyond its cells"):
  - big (200×100) is refused `:size`; outside (x = 80) `:position`; long (32 X's at column 79)
    `:cell`; wide-edge (界 at column 79) `:cell`.
  - The budget is back to base; forbid any cursor placement past row 24 or column 80,
    `XXXXXXXX`, and `界`.

Host tests: `test/redoubt/screen/native_test.exs` (16 tests through the driver, group, shell and
buffer on the terminal model) and `cells_test.exs` (3 new tests). The Rust cells tests gained 4.

## Gates (commands through scripts/q; exit codes), on the final tree unless noted

- `./test-shell` (full): exit 0. Formatting ok; native ok (cells: 10 tests); beamlet 338 passed;
  BEAM 278 passed, 60 skipped; entry point, terminal and fake kernel ok.
- `make -f scripts/jobs.mk docs`: rc 0 on the final head.
- `make -f scripts/jobs.mk prebuilt`: rc 0.
- `make -k -f scripts/jobs.mk set CASES="nscr-interrupt nscr-hostile-text nscr-beyond-cells
  job-interrupt-line job-interrupt-native job-interrupt-ssh job-kill pipe-carries
  pipe-hostile-output pipe-interrupted pipe-never-reads pipe-no-authority piped-build
  piped-host-tests beamlet-footprint size-budget formatting"`: 30 of 31 PASS. The other,
  job-interrupt-native rv64, timed out under the loaded parallel run waiting for the line typed
  after ^C. Rerun alone (`make rv64/job-interrupt-native`, twice): PASS 7.9 s and 7.8 s. Its
  rv32 run passed in the set. The tree for this run differs from the final head only in one
  sentence of shell.md, after which docs was rerun (rc 0).
- Not run: the full bench (not this package's gate). Unsafe: none added (cells is
  `forbid(unsafe_code)`; screen-stage has no unsafe). size-budget PASS; no ceiling raised;
  beamlet-footprint PASS on both widths.

## Found and fixed during the gates: a frame flood ended the session

The first nscr-interrupt with `flood` (a program writing whole 80×24 frames non-stop) panicked the
session's VM on both widths: "memory allocation of 2.7 MB failed". The session's pages went from
6,372 to 9,493 in 2 s while `:erlang.memory` showed 2–5 MB. On the host, beamlet holds the host
at a steady 1.2 MB heap, so the leak is not unbounded. The problem is that each whole frame
decoded into a list of 1,920 maps makes large transient heaps the VM's allocator could not keep
up with. Fix: `Redoubt.Term.Cells.reduce/3`, the decoder a cell at a time (decode/1 is reduce
collecting, and every vector still answers as before). The host checks and puts each cell as it
is read; a frame refused partway draws nothing, because its buffer's diff is never sent. The
driver draws every screen's frame the same way (`Frame.draw_bytes/1`). After the fix the flood is
ended by Ctrl+\ inside a paste on both widths, the session survives, and the budget returns to
its base. This changes the driver's path for in-VM screens too (the pager and friends), which
the shell's suite covers (screen, pager and frame tests passing). This is the first time screens
are drawn on the machine.

## Measured

- Decoding a whole 80×24 frame (1,920 cells, 30,730 bytes) with Redoubt.Term.Cells.decode under
  QEMU: rv64 0.50–0.71 s, rv32 0.45–0.65 s (5 runs each), about 20 ms a KiB. So on QEMU the
  decoder, not the 1 MiB/s ack bound, holds a flooding program back. shell.md says this, with
  nscr-interrupt's flood as evidence that the driver keeps its share. A follow-up worth
  considering: a beamlet-screen native that decodes a frame with the Rust crate straight into the
  buffer (Tier A, userland/otp/screen), which would make whole redraws of a native screen
  practical at SSH sizes.
- Case times: nscr-interrupt about 20 s, nscr-hostile-text about 18 s, nscr-beyond-cells about
  9 s, per width.

## Deviations from the design, and why

- The record cap is 10 + 47·cells, not 12 + …: the frame header is 10 bytes.
- No zero-width case: the session's width table (OTP's is_wide) gives every grapheme 1 or 2.
- `claim` moved from beyond-cells to hostile-text, and each case runs at most four screens,
  because of pre-existing bug 1 below.
- The cases cannot match a frame directly: the bench reads whole lines and expects are ordered,
  one per line. So the case line spawns a helper that writes a tag line to the console port
  (fd 2) every 0.5 s while a job runs, which ends the frame's line. The drawing checks are links
  in the input chain: a key is typed only after the frame that should precede it is seen. The
  helper is the case's own line, not the program's.
- `spin` was replaced by `flood` after measuring the decode cost, so the case attacks the host
  while it is busy decoding. That found the allocation failure described above.

## Problems found (not fixed; outside NSCR1)

1. **B47 (filed by the orchestrator):** a session's fifth `Redoubt.Pipeline.run` is refused and
   leaks a budget. The repro is in .wash/local/B47-repro.md. Each nscr case keeps to four screens
   a session, and each toml says the bound is B47's, not the design's.
2. **Depends on B46:** `Redoubt.Shell.Driver.of_group/0` gives group 1 s. Inside a ~400-character
   wrapped typed line, group took 3.3 s to answer, because the driver was redrawing the line at
   B46's console rate, so a screen raised "needs the shell's terminal". On the orchestrator's
   ruling the timeout is not raised, the symptom goes to B46's implementer, and the nscr cases
   keep their typed lines short. Until B46 lands, a screen started from a very long line can fail
   that way.

## Affected summaries checked

- **docs/userland/shell.md** — updated: "A native program's screen and the session's key"
  (planned → built, with status and tests); "The cell protocol" (records, events, events.json,
  status); "Full-screen programs" figure (the native path is solid, through the host's buffer);
  "Hostile text never drives the terminal" (status names bench:nscr-hostile-text; "not built"
  removed); "Paste, scrolling …" (the planned native-frames bullet is removed); "Native programs
  and pipes" (`screen`).
- **docs/userland/native.md** "Standard input and output, and pipes" — updated with a bullet
  saying a stage can draw a screen. "Killing a job" — no change needed: screens die through the
  same pipeline owner.
- **docs/plan/m2-usable-shell.md** Progress — step 5 added. The goal, attack lines and steps list
  need no change.
- **docs/userland/beamlet.md** "Screen natives" — no change: its "a native program's frames reach
  the encoder by one decoder" is still true (they reach it through the host's buffer and the
  decoder).
- **docs/servers/piped.md** — no change: pacing uses its existing parked writes.
- **README.md, GETTING-STARTED.md, docs/README.md** (M2 row: "screens") and **docs/SECURITY.md**
  — grepped; no claim about native screens, so no change needed.
- **cells crate docs** — the module doc gains records and events.

## Open risks

- With the decode cost above, a native screen's whole redraw at SSH sizes is slow on QEMU; diffs
  are small.
- A resize on the machine is host-tested only: the UART names no size.
- A refused screen's stderr is dropped, because the runner is killed.

## Fix round 1 (steward-red OK with notes on d60fd03c9)

- P3-1: `Redoubt.Term.Cells.event/2` refuses the interrupt as `{:error, :interrupt}`: Ctrl+\
  always, with any modifiers, and Ctrl+C unless `ctrl_c: :key`. The host's `interrupt?/2` is
  gone, and the host passes its `ctrl_c` to the encoder. There is a new test in cells_test.exs,
  and the vectors test encodes with `ctrl_c: :key`, since events.json has "Ctrl+C as a key".
  shell.md's sentence is now true as written. Folded into the cells, shell and docs commits.
- P3-2: shell.md, the module doc and the host's comment now say the 64 KiB counts events not
  yet written into the program's input pipe, which holds one page more.
- Rebased onto main 24d3016b7. No conflicts; the branch does not touch docs/testbench.md.
- Gates on the new head: `./test-shell` exit 0 (beamlet 339 passed; BEAM 279 passed, 60
  skipped; every stage ok). docs rc 0. prebuilt rc 0. `set CASES="nscr-interrupt
  nscr-hostile-text nscr-beyond-cells job-interrupt-line job-interrupt-native job-interrupt-ssh
  job-kill formatting"`: exit 0, all 15 runs PASS on both widths.
- New head: af2cd9e28.
