# SHELL4 report: the pager, help drawn in it, and completion (host)

Branch wp-SHELL4, worktree /home/mcloonan/redoubt/.worktrees/SHELL4, on main 2db31544b. Two
commits, never pushed:

1. `shell: the pager, and help drawn in it`
2. `shell: Tab completes commands, variables, modules and paths`

Design: .wash/local/SHELL4-design.md (approved with four conditions, answered below).

## What was delivered

- `Redoubt.Screen.Pager` (lib/redoubt/screen/pager.ex) and its pure model
  `Redoubt.Screen.Pager.Doc` (pager/doc.ex). The printer's `value(%Lines{})` goes through
  `Pager.show/2`; `out/1` (new commandlet, area Session; new param type `term`) prints without it.
- `Redoubt.Screen`: `serve/4` (the caller answers the screen's `call/1`, so the evaluator, owner
  of a cat's raw file, hands lines over a batch at a time and halts the stream at the end);
  `terminal_size/0` (group present and the driver says the size is known); a screen runs with its
  caller's heap limit and `include_shared_binaries: true`.
- Driver: `:size` may answer `:unknown` (beamlet's `console_size` unknown no longer becomes 80x24
  for this question; layout still assumes 80x24); answers `{:redoubt_screen, :size, pid}`.
- `Redoubt.Util.Lines` gains `style` (nil | :help); `help` sets :help; `Help.styled/1` (Markdown
  headings, one capitalized word, a usage line bold; list items' hanging indent). `h` is plain.
- `Redoubt.Shell.Completer` (lib/redoubt/shell/completer.ex); the shell sets
  `:io.setopts(expand_fun: ...)` before each read. No atom is made from typed text.

## The approval's conditions

1. **Known size only.** `Pager.show/2` pages only when the screen buffer exists, the group leader
   is OTP's group, and the driver's size is known. On the machine, `consoled` and `sshd` do not
   serve `consol` yet (the page's section is planned), so beamlet's `console_size()` is `unknown`
   on the UART and on every SSH channel: lines print whole. When `consol` lands, consoled answers
   its manifest's size and sshd the pty request's; a channel without a pty stays unknown.
   Shown by `tests/userland-read-only.toml`: `help()` (53 lines) typed at the UART prompt, the
   index's last line expected, `1 + 1` then `2`, and `\[\?1049h` forbidden.
2. **Path completion's authority and timeout.** The listing is `File.ls` of the session's working
   directory, run in a process spawned from `group`, in the session's own VM: the same authority
   and labels as `ls()`. `File.dir?` only on matching names. `bounded/1` runs it (and the code
   path's module list) in that spawned process; `group`'s process waits 300 ms, then kills it and
   returns `{:no, [], []}`: nothing inserted, nothing listed, the terminal's beep (`group` sends
   `beep` on `no`). Not covered by a test: a host directory that is slow to list.
3. **Heap limit.** Test "lines paged past the line's heap limit end that line's evaluation, not
   the shell" (pager_test.exs): shell `max_heap_words: 2 Mi`, an endless `%Lines{}`, `G`; the
   line prints `{:screen, :killed}`, the main screen is back, `1 + 1` gives 2. It failed before
   `include_shared_binaries` (beamlet, like BEAM, leaves refc binaries out of a process's own
   limit by default), so it guards that.
4. **Help's style.** The style field is an atom, not a function: only `nil` and `:help` mean
   anything, and `:help`'s rules are `Help.styled/1`'s. Only `help` sets it; `h` (module docs:
   data) is plain; grep and friends make plain lines. `Doc.push` runs `Text.visible` on the styled
   text of every line (test "help's style bolds its headings ... and is visible too").

## Tests and gates

All through q / jobs.mk on head e6e821586 (log: .tmp/SHELL4/final.log), every one exit 0:
- ./test-shell: every stage (beamlet 176; BEAM 159 + 17 skipped, the beamlet-only screens;
  terminal stage: 3 pty tests incl. the pager and Tab).
- cargo test beamlet-vm 84, beamlet-screen 23, beamlet 8; beamlet-redoubt riscv64 build + fake
  check; difftest erlang 43/43 (1 skipped by design).
- prebuilt rv64 232 / rv32 218 cases, 0 failed; rv64 docs, formatting, size-budget,
  unsafe-budget (no unsafe added), no-cruft PASS.
- Machine shell cases, rv64 and rv32: userland-boot, userland-read-only (now with help()),
  steward-vault-session, steward-ssh-two-principals: 8 PASS.
New tests: test/redoubt/screen/pager_test.exs (9, beamlet), screen/pager/doc_test.exs (9),
shell/completer_test.exs (9), driver_test.exs "Tab completes", cli/tests/shell_pty.rs
help_longer_than_the_terminal_is_paged_and_q_gives_the_screen_back and
tab_completes_a_command_on_a_terminal.
Tier A review per the orchestrator: beamlet red, then simplifier and editor.

## Summaries checked

- docs/userland/shell.md: pager (new built section), Session commands (planned sibling),
  Completion (built), Help (pager), Full-screen programs (heap limit, serve, pager), hostile text
  (pager_test), Commands (`term` type), layers figure (completion solid).
- docs/plan/m2-usable-shell.md: Progress; step anchors.
- GETTING-STARTED.md: Tab, pager, out, beamlet-only.
- README.md, docs/README.md, docs/userland/README.md: list the pager and completion as features
  or M2 goals; no status claim; no change.
- userland/shell/help/terminal.md: unchanged by SHELL4 (see findings).

## Findings (outside SHELL4, not fixed)

1. **A prompt over about 256 columns hides what is typed** (SHELL2's Term or edlin): with the
   working directory 261 characters long and a 400-column terminal, `hexdump(` typed by hand is
   never drawn. Found when the Tab test's tmp directory (named after the test) was long; the test
   name was shortened.
2. **The `terminal` help topic is stale:** userland/shell/help/terminal.md says code that prints
   for itself (IO.puts) is not yet guarded; since SHELL2 it goes through the driver's encoder.

## Review round 1 (folded; rebased onto main 415c86ad6)

Rebase conflicts: tests/userland-read-only.toml (main added two receive lines at prompts 2 and 3;
help() and 1 + 1 now at 4 and 5; both forbids kept; the description's anchor is "The pager") and
the hostile-text status line in shell.md (main added "what the VM logs"; the pager's part kept).

MUST:
1. Red P1, atoms: container_cursor_to_quoted ignores existing_atoms_only (checked: it interned a
   new call name and a new variable). The completer now blanks closed strings' contents and parses
   the head only when every name left in it is an existing atom. Test: "a line naming what is not
   an atom yet is not parsed, so completing it makes none" (an unknown call before a string, an
   unknown variable piped, an unknown call before :atom; none interned) and a closed string's
   unknown words still complete.
2. help/terminal.md: says IO.puts output and what the VM logs are drawn safely. The named
   exception for logger crash reports is not kept: main (415c86ad6) now draws them through the
   guard, and shell.md says so.
3. Wording: both taken as written.
4. Screen.call/1 monitors the serving process and exits {:caller, reason} on its DOWN. Test in
   screen_test.exs "a screen whose serving line ends while it asks ends too, and the session goes
   on"; it fails with the exit replaced by a wait.
5. existing/1 no longer strips "Elixir.": an unquoted atom is the atom it names, an alias gets
   "Elixir." added. No test can reach it by typing: Code.Fragment.cursor_context returns :none
   for ":Elixir.File.", ":Elixir.File.e" and ":\"Elixir.File\".", so that path only ever sees
   Erlang-style names.

TAKEN: first_screen by length(Doc.wrap(...)); the no-terminal path is Stream.chunk_every through
print (print_all gone); `with` in show/2; Buffer.style(bold:, reversed:) for the match and
String.slice(s, 0..-2//1); Doc.down/up step a line at a time; help's styled/1 one regex for
headings, heading?/1, indent/1 one regex; completer: generator match on ~c"Elixir." ++ name,
String.replace_prefix (rest/2 gone), the scan over the binary (no index), one by_parameter
dispatch (a path only in a string; a command or name in a string or after a colon); the red's
nit: shell.md says a line at the prompt can make lines of help's style, harmlessly.
Rejected: none.

Commit messages updated for the atom rule, call/1 and the terminal topic.

## Rebased onto main 5151adaf7 (the shell batch: SHELL7's index, SHELL5, SHELL9, SHELL10, B40-42)

Head aed7f4137. Conflicts: Redoubt.Screen (main's run/3 with :ctrl_c; serve is now serve/5 with
the same opts, run/3 goes through it), the driver (geometry for an unknown size kept with main's
ctrl_c open), screen_test.exs (main's Keys and SHELL4's Asking both kept), shell.md's screen
section, GETTING-STARTED, the M2 progress, shell.ex's moduledoc.

Off the prompt: the pager loads only when a long %Lines{} is shown (nothing loads it eagerly now
that the registry reads the index). The shell builds group's expand_fun closure itself from names
only and calls Completer.tab/4, so the completer loads at the first Tab; the lazy context stays.
Command names for completion come from a new Registry.names/0 over the index, loading no
command's module (Registry.all/0 would load them all).

Gates (log .tmp/SHELL4/gate3.log), all exit 0: ./test-shell every stage (beamlet 240, BEAM 199
+ 41 skipped); prebuilt 236/222, 0 failed; rv64 docs and formatting PASS; `make -j -f
scripts/jobs.mk set CASES="$(scripts/shell-cases origin/main)"`: 30 cases x 2 widths, 60 PASS.
beamlet-footprint: rv64 5,455 of 11,885 (baseline 5,447, +8), rv32 5,274 (baseline 5,265, +9).
The prompt-width finding is fixed on main (SHELL batch: a 300-column prompt).

## Steward red BLOCK (atoms) fixed: head bac823907

No typed text is parsed. call/1 finds the innermost unclosed `(` in the line with closed strings
and charlists blanked (`?(`/`?"` are characters; `exists?(` is a call), the plain name before it
(not after a letter, digit, `_`, `.`, `:` or `@`), its commas, and a `|>` before it; the command
is looked up by its name as a string (Registry.fetch/1 compares strings). Test "completing never
makes an atom: nothing typed is parsed": warm-up Tabs on existing names, then the atom count is
unchanged across Tab for each probe: zqxnosuchcmd9("no, zqxnosuchvar8 |> cat("oth,
zqxnosuchcmd7(:hexd, ñcat(", Ñcat.x(", help(:'cat cd', "oth, :'cat-cd', cat(~z"", "oth,
cat(~CAT"", "oth. With the parser call put back, it fails. shell.md Completion: "Nothing typed
becomes an atom", and that Mod.fu⇥ loads an existing-atom module from the code path, as a call
would.

Gates on bac823907, all exit 0: ./test-shell every stage (beamlet 240, BEAM 199 + 41 skipped);
prebuilt 236/222, 0 failed; docs, formatting PASS; shell-cases set 60/60 PASS; beamlet-footprint
rv64 5,455, rv32 5,275 of 11,885.
