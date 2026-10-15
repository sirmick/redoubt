# NSCR2 report (nscr2-implementer)

Base: main db6575a3f. Branch wp-NSCR2, no commits yet.

## Early checkpoint (2026-10-12)

### Before: measured on the machine (main db6575a3f, smp=1, 1 GiB, the pipe recipe)

Scratch case `tests/nscr-measure.toml` (uncommitted), typed at the console session; times in µs,
5 runs each unless noted; a whole 80x24 frame, 1,920 cells, 30,730 bytes, every cell changed.
Rerun with `make -f scripts/jobs.mk prebuilt && make -f scripts/jobs.mk set CASES=nscr-measure`
(both PASS, rc 0: rv64 27.8 s, rv32 25.9 s).

| Stage | rv64 | rv32 |
| --- | --- | --- |
| `Redoubt.Term.Cells.decode/1` (the Elixir decoder alone) | 434,164–495,529 | 401,743–475,281 |
| The host (`Redoubt.Screen.Native.run` with a `start:` stand-in feeding 10 whole frames, each changing every cell, as the reader does): decode, checks, `Buffer.put` a cell at a time, with the driver drawing in parallel | 7,898,922 for 10 = **0.79 s a frame** | 7,445,328 for 10 = **0.74 s a frame** |
| `Buffer.diff/1` of a whole changed screen (already native) | 7,677–25,823 | 6,126–12,079 |
| `Redoubt.Term.Frame.draw_bytes/1` of that diff (the driver: Elixir decode again, then encode) | 690,097–719,246 | 646,610–665,218 |

So about 1.3 whole frames a second reach the buffer today, and the driver needs a further ~0.7 s
for each whole frame it draws. A first attempt with the per-cell callback typed at the prompt
(interpreted by erl_eval) read 1.0 s and 27 s: an artefact, discarded.

### Proposal

- **Native:** `redoubt_screen:frame(B, Body, From)`, resumable. With `From = 0` it reads and checks
  the header in the Elixir decoder's order (version, flags, size, count, truncated); a size other
  than B's returns `{size, W, H}` before anything is put (the host still decides stale or
  `{refused, size}`); a clear blanks the back grid. Each cell is checked in the decoder's order
  (position, twice, symbol, colours, modifiers), then the host's (`cell`: a wide symbol in the
  last column), and put. A symbol of more than one code point returns
  `{joined, Next, X, Y, Symbol, Style}`: the host checks it with OTP's segmentation exactly as now
  (`String.graphemes`, `Width.grapheme`), puts it with `Buffer.put`, and calls
  `frame(B, Body, Next)`. Ends `ok` or `{error, Why}`, `Why` the decoder's atoms or `cell`.
  So no grapheme segmenter in Rust, and a frame of plain symbols makes one call.
- **The buffer:** the host's existing `redoubt_screen` buffer (Grid, owned by the host, at most
  65,536 cells, bytes counted toward the host's heap limit). The only new memory is the resume
  state in the Grid: a seen bitmap of W×H bits (at most 8 KiB), counted in the buffer's declared
  bytes. Nothing scales with the frame.
- **Vectors:** one set, `userland/native/cells/vectors.json`. (1) beamlet-screen host test: every
  vector into a buffer of its size, the same answer, and for a frame its cells back from `diff`.
  (2) ./test-shell on beamlet: every vector, and a seeded differential of mutated frames (wide,
  edge, joined, twice, truncations), through the native and through the Elixir reference
  (`Cells.reduce/3` with the host's checks and `Buffer.put`): the same answer and the same diff.

### Questions

1. Rust reader: add a cell-at-a-time `cells::reduce` (callback) to `userland/native/cells`, with
   `cells::decode` built on it, so Rust keeps one decoder (recommended). That needs
   userland/native/cells as an owned path. The alternative is a second strict reader inside
   beamlet-screen.
2. Drawing path: with the host fast, the driver's `Frame.draw_bytes` (~0.7 s per whole frame)
   bounds frames drawn to about 1.4 a second. Should NSCR2 also add `redoubt_screen:draw/1`
   (cells frame bytes to escape sequences in Rust, held to `Frame.draw/1` by a differential
   test, `Frame` kept as the reference)? Recommended: without it the floor a case can gate is
   on frames decoded, not on frames drawn.
3. The floor case: a new screen-stage mode `frames N` (N whole frames, each changing every cell,
   then `frames-done`, then waiting for `q`), with the line timing `screen` in the VM. The 1 MiB/s
   pace caps ingestion at about 34 whole frames a second, which sets the ceiling on the floor.

## Rulings (orchestrator, 2026-10-12; steward-red no objection, four conditions)

1. One Rust decoder: `cells::reduce` in userland/native/cells, `decode` on it (path added).
2. `redoubt_screen:draw/1` in Rust, its own commit after the decoder, held to `Frame.draw/1`.
3. The floor case: screen-stage `frames N`, timed in the VM, counting drawn frames.
steward-red's conditions: (1) draw from Grid contents only, only the escapes Frame.draw emits,
a second control guard, a differential over every code point below U+3000, the planes' edges,
hostile symbols forced past the decoder, every modifier and colour, clear/no-clear, byte for
byte. (2) Width is OTP's. (3) A joined symbol never reaches the Grid before the host's verdict;
the seen bitmap per frame; a resumed frame neither re-puts nor double-counts. (4) The floor
counts drawn frames at the 30/s tick, guest time, both widths, margin stated.

## Commit 1: the decoder, 6aa039fc1 (base main db6575a3f)

`screen: a native program's frames are read into the screen buffer in Rust, a cell at a time`

- cells: `header`, `Cells` (a cursor holding no borrow: `at`, cells left, a bit a cell of
  positions given, made at the first cell), `reduce` (Step::Frame / Step::Cell, ControlFlow),
  `decode` built on `reduce`; `CellRef`, `Header`. Native-program-side tests unchanged and green.
- beamlet-screen: `Buffer::read(bytes, from) -> Read::{Done, Size, Joined} | Refused::{Frame,
  Cell, Resume}`; native `redoubt_screen:frame/3` -> `ok | {size,W,H} | {joined,Next,X,Y,Symbol,
  Style} | {error,Why}`, badarg on a stale From / non-binary / non-owner. `Buffer::bytes` now
  counts the bit-a-cell record. Frame bytes read in place (Bits::to_bytes borrows).
- Shell: `Redoubt.Term.Buffer.frame/3`; the host calls `Redoubt.Screen.Native.read/3`; the Elixir
  reading kept as `reference/3` (Cells.reduce + the host's checks + Buffer.put).
- Steward-red condition 3: a joined symbol is returned unput; the host puts it only after
  `one_cell?` (OTP's segmentation); a refusal there ends the screen with no diff, tested by
  native_test's new row "a last cell of two graphemes after cells to draw" (refused :cell,
  no XXXXXXXX drawn). The position record is made per frame (`Cells::new` per header) and a
  resume goes on with it: `the_positions_given_are_a_frames_own_and_a_resumed_frame_neither_
  puts_again_nor_forgets_them`. Condition 2: width is the generated table (OTP is_wide over
  0..0x10FFFF, `tools/gen-width.escript --check` in ./test-shell's native stage); joined symbols
  go back to OTP for segmentation and Width.grapheme.
- Tests: cells +2 (reduce; cursor vs decode over good and bad lists). beamlet-screen +8: every
  vector; a seeded differential (3 targeted + 4,000 mutations of a whole 80x24 frame with wide,
  joined, every colour, edge) against a reference built on `cells::decode`, reaching Ok, Cell,
  Twice, Position, Symbol, Color, Modifiers, Trailing, Truncated; size; joined/resume; refusal
  keeps earlier cells; clear; per-frame positions; and the VM-level probe (`read/0`, fixture
  rebuilt with the pinned erlc, the old source reproduces the old .beam byte for byte). Shell:
  read_test.exs (beamlet only): every vector through `read/3` and `reference/3`, same answer and
  same buffer; 404 inputs (4 targeted + 400 seeded mutations) same answer and same buffer, even
  after refusals; a joined symbol of two graphemes refused.

### After (same scratch case, smp=1, PASS both widths)

| Stage | rv64 | rv32 |
| --- | --- | --- |
| the host, 10 whole frames each changing every cell | 188,689 µs = **19 ms a frame** | 144,559 µs = **14 ms a frame** |
| (before) | 0.79 s a frame | 0.74 s a frame |
| driver `Frame.draw_bytes` of a whole diff (unchanged; commit 2) | 0.70-0.88 s | 0.63-0.65 s |

### Gates on 6aa039fc1 (env from launch-env-2026-10-12.md, all through scripts/q)

- `make -f scripts/jobs.mk prebuilt` rc 0
- `make -k -f scripts/jobs.mk set CASES="nscr-interrupt nscr-hostile-text nscr-beyond-cells
  shell-contexts beamlet-footprint size-budget formatting"` rc 0, every run PASS on both widths
- `make -f scripts/jobs.mk docs` rc 0
- `./test-shell` rc 0: beamlet 355 passed; BEAM 291 passed, 64 skipped; every stage passed
- `cargo test -p beamlet-screen -p beamlet-redoubt --features beamlet-redoubt/fake` (userland/otp,
  tools/env.sh) rc 0
- `tools/difftest` rc 0: 527/527 passed, 21 skipped by design
- `cargo test -p cells` (userland/native) 12 passed
- beamlet-footprint: heap beamlet **5,491 of 11,092** pages rv64 (rerun alone, `make
  rv64/beamlet-footprint`, PASS; the set's rv64 log was not kept), **5,311 of 11,092** rv32.
  Main's peak not re-measured; `frame/3` allocates only while a screen reads a frame, never at
  the prompt.

### Summaries checked

- docs/userland/shell.md: the cell protocol (status + one decoder a cell at a time), a native
  program's screen (status +3 tests, Frames, Pace with the new numbers): updated.
- docs/userland/beamlet.md, Screen natives: status (was "no boot draws a screen", stale since
  the nscr cases), +10 tests, `frame` row, Bounded, Counted, segmentation bullet: updated.
- docs/plan/m2-usable-shell.md: progress "frames the session checks, paces and draws" and the
  attack-suite bullets: still true, no change (the floor's numbers land with the case commit).
- README.md, GETTING-STARTED.md, userland/otp/README.md: no claim about frame decoding; none.
- docs/testbench.md: no new case yet (the case commit).

## Head gates at bf01f9828, run by the orchestrator (2026-10-14)

steward-red P2-1: the head's gates were not recorded. Run in .worktrees/NSCR2 through scripts/q, logs under .tmp/NSCR2-gates/:

- `make -f scripts/jobs.mk prebuilt`: rc 0 (rv64 and rv32 prebuilt, 0 failed).
- `make -f scripts/jobs.mk set CASES="nscr-frame-rate nscr-hostile-text nscr-beyond-cells shell-contexts beamlet-footprint size-budget formatting docs"`: rc 0, every run PASS on both widths:
    PASS  size-budget                     
    PASS  docs                               1.8s
    PASS  nscr-beyond-cells [rv64, smp=1]    4.8s
    PASS  beamlet-footprint [rv64, smp=1]    5.2s
    PASS  nscr-beyond-cells [rv32, smp=1]    5.5s
    PASS  nscr-hostile-text [rv64, smp=1]    6.0s
    PASS  beamlet-footprint [rv32, smp=1]    6.1s
    PASS  nscr-hostile-text [rv32, smp=1]    6.7s
    PASS  nscr-frame-rate [rv64, smp=2]     13.7s
    PASS  shell-contexts [rv64, smp=1]      15.9s
    PASS  nscr-frame-rate [rv32, smp=2]     16.0s
    PASS  formatting                        23.4s
    PASS  shell-contexts [rv32, smp=1]      15.4s
- `./test-shell`: rc 0.
