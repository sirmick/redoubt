# BENCH2: the formatting gate walks every .ex and .exs

Branch `wp-BENCH2` in `.worktrees/BENCH2`, base `24507bec1` (origin/main), head `c32178bd8`.
Two commits, each read in full by me before committing:

1. `1a6635c63` workspace: every hand-written Elixir file is formatted, under a .formatter.exs
   at the root for the files outside the shell (14 files + `.formatter.exs`, whitespace only)
2. `c32178bd8` testbench: the formatting case runs mix format over every .ex and .exs in the
   tree, from the deepest root holding a .formatter.exs above each (gate, case file, host test,
   pages)

Scratch and logs: `.tmp/BENCH2/` in the worktree.

## 1. What the gate formatted today, and what it did not

The found cause: **the bench's `formatting` case ran no Elixir formatter at all** — only
`cargo +nightly fmt --all --check` per cargo workspace root (`tools/testbench/src/fmt.rs`,
`tests/formatting.toml`). The one `mix format` in the tree is `./test-shell`'s `formatting()`
stage, run from `userland/shell` over that project's `.formatter.exs` inputs
(`{lib,test}/**/*.{ex,exs}`), and never by the bench. So `make -f scripts/jobs.mk set
CASES=formatting` passed with `echo.ex` unformatted, and with every Elixir file outside the
shell unformatted.

Measured on `24507bec1` (`.tmp/BENCH2/shell.log`, `rest-98.log`, `rest-110.log`):

- Checked by `./test-shell` only (not by the bench): `userland/shell/**/*.{ex,exs}`, 1 file
  unformatted: `userland/shell/lib/redoubt/agents/echo.ex`.
- Checked by nothing: the 41 tracked `.ex`/`.exs` outside `userland/shell`. At 110 columns
  (the shell's and rustfmt's width) 38 were unformatted; 25 of them are generated
  (`libs/wire/elixir/proto/*.ex`, `libs/wire/elixir/client/*.ex`, `libs/steward/elixir/gen/*.ex`)
  and 13 hand-written: `libs/steward/elixir/{effects,guards,render,steward,trace}.ex`,
  `libs/wire/elixir/lib/wire.ex`, `libs/wire/elixir/test/{client_test,vectors_test}.ex`,
  `userland/otp/tests/elixir/{compiler_test,io_test,otp,processes,regex_test}.ex`. Already
  formatted: `userland/otp/tests/elixir/{basics,iex_test,percent_test}.ex`,
  `tools/testbench/src/userland.exs`, the three doccheck fixture `.ex` files.

The gate run on the unformatted tree with the new walk (`.tmp/BENCH2/gate-before.log`) names
exactly the 14 hand-written files.

## 2. The gate's walk

`tests/formatting.toml` gains an `[elixir]` table: `roots = [".", "userland/shell"]` (each a
directory holding a `.formatter.exs`) and three `[[elixir.skip]]` with reasons. `fmt.rs`:

- lists every `.ex`/`.exs` and `.formatter.exs` with `git ls-files --cached --others
  --exclude-standard` (tracked and untracked-not-ignored, so a file about to be added counts;
  `.worktrees/`, `_build/`, `target/` are ignored), dropping deleted and skipped files;
- assigns each file to the deepest root above it and runs `mix format --check-formatted
  <files>` in that root under the pinned toolchain (`. userland/otp/tools/env.sh`, as the
  `elixir` kind does), so each project's own `.formatter.exs` applies; explicit files are
  checked whatever the root's `inputs` say;
- findings: a file not formatted (one line per file, `mix` names them on stderr in colour,
  stripped), a root with no `.formatter.exs`, a `.formatter.exs` in no root, a file under no
  root, a skip with no reason or over no Elixir; `mix` missing → the case fails unless
  `--allow-skip`, as for nightly rustfmt.
- Stated default for the files outside the shell: a new root `.formatter.exs` — 110 columns,
  inputs naming the hand-written roots, `subdirectories: ["userland/shell"]` so a plain
  `mix format` at the root formats the whole tree (verified: an explicit shell file is held
  to the shell's config from the root, `.tmp/BENCH2/subdir.sh`).
- Skips: the generated dirs. Both generators have `generated_files_are_current` tests holding
  the checked-in Elixir to their output (libs/wire/gen/src/lib.rs:1577,
  libs/steward/gen/src/lib.rs:1142), the same reason `libs/wire/src/proto/` is in
  `rustfmt.toml`'s ignore; formatting them by hand would fail those tests, and changing the
  generators is outside this node (and a hotspot). Flagged to the orchestrator as the one
  judgement call.

Host test `fmt::tests::every_elixir_file_is_walked_from_its_deepest_root` (a temp git repo:
tracked, untracked, deleted, ignored, skipped files, a nested root, a stray `.formatter.exs`,
a root without a config, a skip over nothing) and `colour_codes_are_stripped`. The existing
`a_case_out_of_the_whole_run_runs_only_by_name` builds a minimal fmt case and now gives it the
table.

## 3. The formatting commit is whitespace only

`git diff --stat 24507bec1 1a6635c63`: 15 files, +296/−88 (the `.formatter.exs` is 15 of the
added lines). Token check (`.tmp/BENCH2/same_tokens.exs`): for each of the 14 files,
`Code.string_to_quoted!` of HEAD's text and the tree's, with every metadata entry (line,
column) dropped via `Macro.update_meta`, compared equal: `14 files, 0 with a token changed`.
One visible non-blank change is the formatter's: `inspect([a: 1])` → `inspect(a: 1)` in
`userland/otp/tests/elixir/io_test.ex` (a trailing keyword list loses its brackets; same AST),
and `Process.sleep(1); wait_restart(old)` split onto two lines in `otp.ex`. The formatter is
idempotent on the result (the gate passes on the head).

## 4. Negative

Scratch `userland/shell/lib/redoubt/agents/scratch_unformatted.ex` (untracked, unformatted),
`make -f scripts/jobs.mk set CASES=formatting` → `make rc=2`,
`FAIL formatting … userland/shell/lib/redoubt/agents/scratch_unformatted.ex: not formatted
(mix format in userland/shell)`. Log: `.tmp/BENCH2/negative-formatting.log`
(`negative.make.log`). File removed; `git status` clean after.

## 5. Gates on the head (c32178bd8)

- `make -k -f scripts/jobs.mk set CASES="formatting docs host-tests"` → make rc=0:
  `PASS formatting 24.1s`, `PASS docs 1.7s`, `PASS host-tests 63.6s` (234 testbench tests incl.
  the new ones). Logs `.tmp/BENCH2/gate-{formatting,docs,host-tests}-head.log`.
- `q run --cores 4 -- cargo test -p testbench -- a_case_out_of_the_whole_run fmt::` → 5 passed.
- `q run --cores 4 -- ./test-shell` (the formatted files include the shell's echo.ex):
  `test-shell rc=0`; every stage ok (native, on_beamlet 385 tests 0 failures 1 skipped,
  on_beam, entry_point, terminal, on_fake_kernel). Log `.tmp/BENCH2/test-shell.log`.
- `q run --cores 4 -- userland/otp/tools/difftest elixir` (the formatted OTP test programs are
  this differential suite's inputs; the bench does not run it): `8/8 passed, 0 failed`,
  `difftest rc=0`. Log `.tmp/BENCH2/difftest-elixir.log`.

No boots; no bare cargo/mix (every build and test through `scripts/q run` or `jobs.mk`).

## Summaries checked

- `docs/testbench.md` "The formatting gate": status line gains the host test; a paragraph for
  the Elixir walk, the roots, the skips and the gap that was. Updated.
- `CONTRIBUTING.md` "Formatting": the `mix format` commands and what the case covers. Updated.
- `GETTING-STARTED.md:253` (`./test-shell # formatting, …`): still true, test-shell keeps its
  stage. No change.
- `README.md`, `tools/testbench/README.md`, `userland/shell/README.md`,
  `userland/otp/README.md`, `docs/userland/shell.md`, `docs/userland/development.md` (names
  `compiler_test.ex`, path unchanged): no formatting claims affected. No change.
- `tests/formatting.toml` description: says Elixir too. Updated.
- No dates on pages; no package ids in case files or comments (docs case passes).

## Open points

- Generated Elixir is skipped rather than formatted (reason above); if the owner wants the
  generators to emit formatted Elixir, that is a follow-up on `redoubt-wire-gen` and
  `redoubt-steward-gen`.
- `./test-shell`'s own `formatting()` stage is now redundant with the bench's case for the
  shell; left as the shell's quick local check.
- A worktree without `BEAMLET_TOOLCHAINS` set has no `mix` on `env.sh`'s path (no
  `toolchains/` of its own): the case then fails (or skips under `--allow-skip`), as the
  Elixir oracle cases already do.
