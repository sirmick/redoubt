# The shell track's rules

Owner's ruling, 2026-10-08. They apply to shell UI packages: SHELL5, SHELL6, SHELL7, SHELL9 and later.

## The `shell` branch

Shell UI packages merge into the integration branch `shell`, not into main.

Gates for a merge into `shell`:
- `./test-shell` (ExUnit on beamlet and the BEAM, the pty tests);
- the docs check and formatting;
- one reviewer: the beamlet red, or the simplifier for pure UI.

Pages must still be true.

Also gate on beamlet-footprint, on both widths. The prompt's heap headroom is small (37 pages on rv64 at 2026-10-08), and any module loaded at start costs pages there.

## Into main, in batches

`shell` merges into main every few packages, or about daily, through the full gate:
- `scripts/shell-cases` against main, which selects the shell set of 14 cases, on rv64 and rv32;
- `./test-shell`;
- the full difftest;
- `prebuilt`;
- the static checks (size, unsafe, no-cruft).

The orchestrator does that merge, with `--no-ff` and its trailers. A failure there is fixed on `shell` before the merge. `shell` rebases onto main, or merges main into it, after each batch.

## What stays strict, even on the shell track

These go to main directly, under the full rules: the red's Tier A review and the machine cases on their own commits.
- Anything under `userland/otp`: the beamlet VM, natives and platform.
- The escaping and drawing path: `Redoubt.Term.Text`, the encoder, and anything that writes to the terminal.
- Anything acting with the session's authority: completion's file listing, `exec`, budgets, files.

A shell package that needs one of these splits it out as its own commit or package for main.

## In flight when this was ruled

SHELL4 and SHELL8 touch the drawing path, so they finish under the full rules and go to main.
