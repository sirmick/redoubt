# B49 report: rv32 nscr-hostile-text

Base: main 050357e8a. Head: wp-B49 80df38d36 (one commit, tests/nscr-hostile-text.toml only).

## Cause

Nothing splits the line. The case has an ordering race, and rv32 loses it.

- The helper `up.("up-stderr")` writes its first tag about 50 ms after the job is running, then
  one every 500 ms. The case sent `q` at the first tag.
- On rv64 the program's frame is usually drawn before that first tag; on rv32 the tag comes
  first. The `q` then ends the program before a second tag can end the frame's line.
- When the screen ends, the session (driver.ex `close_screen`) writes `Frame.leave()` (CSI 0m,
  ?25h, ?1049l), then the held standard error as visible text. Leave ends no line, so the frame,
  the leave and `^[]52;c;…^[X` share one console line. With its CSI removed, that line shows as
  `+---…|screen-stage-ready|+---…key:none^[]52;…`.
- The expect `(^|\] )\^\[\]52…$` can never match that line. after_all is then printed and the
  case runs to its 450 s deadline.
- Evidence: B47's failing log .worktrees/B47/target/testbench/run-2323062-*/ (lines 80-85), and my
  own repro on main 050357e8a: FAIL 450.1 s with the same line shape
  (target/testbench/run-2716540-1791595058538990757). B47's one rv32 pass
  (run-2321651-*) passed only because a host `[!] Terminating process` line broke the frame's
  line before the leave.

The writers are right. On a terminal, leaving the alternate screen restores the main screen's
cursor, which is at the start of a line, so the text is shown at column 0. The bench reads a
byte stream and models no alternate screen. Not involved: the testbench reader, UART or relay
chunking, the line width, and Term/Width (B46's files: untouched).

## Fix

The `q` input now waits for `screen-stage-ready` (the frame's line, which only a tag written
after the frame can end) instead of the first `up-stderr` tag. nscr-interrupt already does this
for its draw step. What the session draws as the screen ends now always starts a line of its own.
A comment in the case says why.

## Gates (prebuilt from 80df38d36)

| gate | result |
|---|---|
| make prebuilt | rc 0 |
| rv32/nscr-hostile-text ×5, alone | PASS, 12.6–13.1 s |
| rv64/nscr-hostile-text ×5, alone | PASS, 14.1–15.5 s |
| load: `set CASES="nscr-hostile-text nscr-interrupt nscr-beyond-cells"` (6 boots at once, on a host also running train-16) | rc 0, all 6 PASS (hostile-text rv32 21.6 s, rv64 17.7 s) |
| make docs | rc 0 |
| repro on main 050357e8a, rv32 | FAIL 450.1 s (expected) |

The passing rv32 log's line 86: `[con …] ESC[0m ESC[?25h ESC[?1049l^[]52;c;cHduZWQ=^G^[]0;pwned^G^[X`,
shown as `[con …] ^[]52;…`.

## Summaries checked

- docs/userland/shell.md (lines 437-439, 616): lists `bench:nscr-hostile-text` only; the case's
  description is unchanged, so no change needed.
- docs/testbench.md, "What a case passes on": the shown-line rule is unchanged and is what this
  relies on; no change.
- No formatting gate covers case TOML; the docs gate passes.

## Note (not changed)

nscr-interrupt's ctrl_c step sends `\u0003` at the first `up-ctrl-c` tag. Its following expect
(`key:ctrl\+c`) is drawn by the program after the keys, and nothing there is anchored to a line
start, so it does not have this race. It passed under load.
