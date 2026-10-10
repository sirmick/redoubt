# SHELL9 report

Branch `wp-SHELL9` (worktree /home/mcloonan/redoubt/.worktrees/SHELL9), off origin/shell
(36d1450f9), one commit: 0d3627d4d "shell tests: a 300-column prompt on a 120-column terminal
keeps the typing in view". Not pushed.

## Finding: neither Redoubt.Term nor edlin is at fault

- Swept cwd lengths 250, 255, 256, 257, 261 and 270 on a 400-column driver and model, typing
  `hexdump(` key by key: every case drew the prompt and the typing on one row, and the cursor was
  where it should be, on beamlet and the BEAM. A 300-column prompt on 120 columns: the prompt
  wraps over 3 rows, the typing follows it on row 2, Ctrl+A plus an insertion (a full redraw from
  the line's start) puts the cursor back at {2, 61}, and Enter evaluates the line.
- The cause is SHELL4's test setup. `driver_test.exs`'s `start/1` built
  `[input:, output:, size: {@cols, @rows}, shell:] ++ opts`, and the driver reads
  `Keyword.get(opts, :size)`, which returns the **first** value. SHELL4's Tab test passed
  `size: fn -> {200, @rows} end`, with a 400-column model in the case that failed, but the driver
  still drew for 120 columns. With a 261-character cwd the prompt is past 120 columns, so the
  encoder's wrapping and relative cursor moves did not match the model. Reproduced: the driver at
  120 columns and the model at 400, with `hexdump`, Ctrl+B and `(` typed, puts the model's cursor
  at {0, 34}. With the sizes matched it is at {0, 274}.

## Delivered (userland/shell/test/redoubt/shell/driver_test.exs only)

- `start/1` now builds the driver's options with `Keyword.merge(defaults, opts)`, so a test's own
  `size` really is the driver's size.
- New test "a prompt wider than the terminal wraps, and what is typed after it is shown": the
  cwd makes the prompt exactly 300 columns on the 120-column model. It checks rows 0 and 2, the
  cursor after typing, the cursor after Ctrl+A and an insertion, and the result `22`.
- No drawing-path code changed, so nothing needs to go to main under the strict rules.

## Gates

- `q run --cores 8 -- ./test-shell`: exit 0. "every stage passed": formatting ok, native ok,
  on_beamlet 150/150, on_beam 142 passed and 8 skipped, entry_point ok, terminal (pty) ok,
  on_fake_kernel ok.
- `q run --cores 4 -- cargo run -q -p redoubt-doccheck`: exit 0.

## Summaries checked

- docs/userland/shell.md, "The terminal library" and "Line editing and history": unchanged. They
  make no claim about prompt width. The line-editing status line already names driver_test.exs,
  and its topic list is not exhaustive (it omits wide characters and long lines too).
- Redoubt.Term's moduledoc: still true. It says a line taller than the screen is drawn but not
  edited exactly, which this package does not touch.
- README.md, GETTING-STARTED.md, userland/shell/help/terminal.md: none mentions prompt width.
  The help topic's staleness is SHELL4's finding 2, which is outside this package.

## Risks / coordination

- SHELL4 (wp-SHELL4, going to main) changes the same `start/1` and its Tab test relies on
  `size: 200`. Once SHELL4 and this branch meet, its Tab test runs at a real 200 columns for the
  first time. It should pass, since its prompt fits in 200 columns, but it is now really
  exercised. Expect a textual conflict in `start/1` when main and `shell` merge; keep the
  `Keyword.merge` form. SHELL4's note in its test ("The name is short: the test's directory ...
  is the prompt") can go once this is in.
