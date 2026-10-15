# AGT2: the shell's non-interactive mode

Tier B (the shell, inside one session's budget, with that session's authority; it holds no
capability and renders no approval). Size S-M. Shell track: host gates (`./test-shell`) on the
shell branch, plus the one bench case below, which the train runs. Needs nothing; AGT1 is only
needed to run the bench case as `agent`, so write it for `alice` and switch the user if AGT1 has
landed.

Why: a program driving a session over a pty (the owner's host-side agent loop, step AGENT0)
needs to send a block of Elixir and know exactly where its result ends and whether it failed.
Today the only signal is the next prompt, which a result can contain, and the line editor echoes,
completes and redraws, all noise to a program.

Run everything natively on this host through `q run`, under the job pool's rules
(docs/testbench.md "On a shared host"). Scratch under `$REDOUBT_TMP`.

## Context rules (read these first)

- **Don't read whole files.** `userland/shell/lib/redoubt/shell/driver.ex`, `evaluator.ex`,
  `printer.ex` and `shell.ex` by function; shell.md by section.
- **Don't open `.wash/qa/*.md` or other packages' reports.**
- **Pipe test output.** `./test-shell` prints a lot; grep for failures.
- **Read a file right before you Write it,** and prefer Edit.
- **Report under 1900 bytes,** detail in `.wash/local/AGT2-report.md`.
- **A handoff ends with "what consumed my context".**

## Reading list (only these)

- `docs/userland/shell.md`: "The loop", "Line editing and history", "Hostile text never drives
  the terminal", "Interrupting and killing jobs" (the driver's `{:redoubt_eval, ...}` protocol).
- `userland/shell/lib/redoubt/shell/driver.ex`: `run/1`, `loop/1`, `input/2`, `typed/2`,
  `open_feed/0` and `feed_*` (the feed is the nearest thing to this mode already there: read it
  first and reuse it if it fits).
- `userland/shell/lib/redoubt/shell/evaluator.ex` whole (short).
- `tests/shell-commands.toml`: how a bench case drives a session and tags answers.

## What to build

A mode a session enters by a command at the prompt, `machine(token)`, with a caller-chosen
token (a short string of `[A-Za-z0-9_-]`, 8 to 64 bytes; anything else is refused), and leaves
with `machine(nil)` or at the end of the session. In the mode:

- **Input is lines, unedited.** No echo, no completion, no history recording, no redraw. Bytes
  up to a newline are a line. A line that is exactly the token ends an **input block**, and the
  block is parsed and evaluated whole, so the driver can send a multi-line `defmodule` without
  the loop's "reads another line" heuristic deciding where it ends. Ctrl+C and the session key
  keep their meaning (the interrupt still ends the line being evaluated).
- **Every block's end is marked.** After the block's output, one line of the form
  `<token> <n> ok`, `<token> <n> error` or `<token> <n> exit`, where `n` is the line counter the
  prompt shows, and the status is what the evaluator returned. The mark is written by the shell
  itself through the same guard as everything else; since the token is plain ASCII the guard
  changes nothing in it. No prompt is drawn in the mode.
- **Output is otherwise unchanged:** results and errors print as they do today, through
  `Redoubt.Term.Text`, so hostile text in a result is drawn visibly and can never forge a mark
  (a forged mark would have to be a line starting with the token, which the driver chose and the
  evaluated code cannot read: keep the token out of the bindings, the environment and the
  process dictionary of the evaluating process).
- **Screens, the pager and the editor are refused in the mode** with an error, not entered: a
  program driving the pty cannot answer them. Settle this on the page's Open list if you find a
  reason to allow one.

Decisions the page's Open list may take (Tier B: no Architect unless you change what a session
holds): the exact mark format; whether a block may be ended by EOF (Ctrl+D) as well as the token
line; whether `machine/1` should also set the terminal's width to a large fixed value so the
printer never wraps.

## Tests

- **Host tests** in the shell's ExUnit suite, on beamlet and on the BEAM through `./test-shell`:
  entering and leaving the mode; a one-line block; a multi-line `defmodule` block; a block whose
  evaluation raises (`error`), one that calls `exit()` (`exit`), one interrupted (the mark says
  `error`); a result containing a line that begins with the token is drawn with the token's
  bytes visible and is followed by the real mark; a token the rule refuses; an attempt to open
  a screen in the mode.
- **One bench case,** `tests/shell-machine.toml`, on rv64, with the image's recipe: a session
  over SSH enters the mode, sends a three-line `defmodule` block and a call to it, reads the
  value and the `ok` mark, sends a block that raises and reads the `error` mark, leaves the mode
  and gets a prompt again. Verdicts: the printed values and marks (rule F). It runs in the train
  with the shell set.

## Pages

- `docs/userland/shell.md`: a new section "The machine mode" after "The loop", with its status
  line naming the host tests and the case; "Commands" gains `machine/1` in the session commands
  table; the "Why" says why a mark and not a prompt (a result can contain a prompt), and why the
  evaluated code never sees the token.
- `docs/plan/m2-usable-shell.md` "Progress": one clause.

## Owned paths

`userland/shell/` (the driver, the evaluator, the commands, the suite), `tests/shell-machine.toml`,
the pages above. Nothing under `servers/`, `libs/rt`, `userland/otp` or `tools/testbench`.
