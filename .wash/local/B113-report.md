# B113 report: an interrupt discards the input queued when it is taken

Branch `wp-B113`, base main f977dd514, head c1837585b (one commit:
`shell: an interrupt discards the input queued when it is taken`).

## What changed

`userland/shell/lib/redoubt/shell/driver.ex`
- The driver gives `group` what is typed only while `group` reads a line (`gate: :open`), and only to
  that line's end (`\r` or `\n`) or a Ctrl+D. Then it sends `group` a no-op `{:setopts, []}` request,
  the same mechanism the old Ctrl+D check used (`gate: {:sync, ref, what, interrupted}`). Once the
  reply arrives:
  - the line is still open (a key that ended no line, such as Enter taking a history search): the
    gate opens again;
  - the line has ended: `:closed` until `group` sends its next `{:requests, _}` draw. Only a read
    sends that; a program's output is `put_chars_sync`. Edlin's prompt, dumb mode's prompt
    (`put_chars`) and the restore of a nested read all send `{:requests, _}`.
  - Input that arrives in the meantime waits in `typeahead`, and the console's end in `ended`.
- The interrupt (outside the machine mode) is found in the raw bytes before UTF-8 decoding. The
  rest of its read goes, and so do `typeahead` and `held` (a cut UTF-8 sequence). Then it acts:
  - at once (under a feed: `interrupt_line`; line open: drop the line, `Process.exit(group,
    :interrupt)`, gate `:closed`; else `interrupt_line`);
  - or, while a line end is unconfirmed (`:sync`), once `group` has taken it.
- The screen path's interrupt drops `typeahead` and `held` too. The machine mode is unchanged, by
  its documented rule.
- `eof_check` and `key/3` are gone, folded into the gate. The Ctrl+D judgement is unchanged.

## Bugs on main this closes (shown by the new tests run against main's driver)
1. A line typed ahead while another ran, and the input before the interrupt in its own read, ran
   at the next prompt.
2. A non-UTF-8 byte after the interrupt let the rest of the read through (`typed/2`'s error branch
   went on after `keys/2` had taken the interrupt): `:same_readÿ:latin1_tail` was run.
3. A paste `":first\r:queued\r:queued_too\x03:tail\r"` in one read: the driver still thought
   the line open, sent `group` the interrupt exit after `group` had finished `:first`, and
   `group` (no read pending) did `exit(Shell, interrupt)`. **The session died.** Now the
   interrupt waits for the sync.

## Tests
`userland/shell/test/redoubt/shell/driver_test.exs`, two tests:
- "the interrupt while a line runs drops the lines typed ahead and the rest of its read, and a later
  read is the next line": typed-ahead line, same-read tail with `\e[201~` and a 0xFF byte, a later
  read kept.
- "a paste holding the interrupt leaves no line after the one being read, and no tail, for the next
  prompt".

Against main's driver (scratch worktree at f977dd514 with the new test file):
- beamlet: 42/45, the two new tests failing, plus `a crash report of a process a line spawned …`;
  that one is unrelated, and the same test passed on the branch's runs.
- BEAM: 43/45, the two new tests failing.

## Gates (exact commands, from the worktree, env per launch-env)
- `scripts/q run --cores 4 -- ./test-shell test/redoubt/shell/driver_test.exs`: rc=0, 45/45 on
  beamlet and on the BEAM.
- `scripts/q run --cores 4 -- ./test-shell` (every stage): rc=0. beamlet 386 passed, 1 skipped;
  BEAM 317 passed, 70 skipped; formatting, native, entry_point, terminal and on_fake_kernel ok.
- `make -f scripts/jobs.mk set CASES="docs formatting"`: rc=0, docs PASS, formatting PASS.
- `make -f scripts/jobs.mk prebuilt`: rc=0 (rv64 376, rv32 361 built, 0 failed).
- `make -j4 -k -f scripts/jobs.mk rv64/{job-interrupt-line,job-interrupt-native,job-kill,nscr-interrupt}`
  and the same for rv32: 8/8 PASS.
- Shell set (orchestrator's scope): `make -k -j6 -f scripts/jobs.mk set CASES="<96 cases>"` (the script's shell set today, 79 cases, plus every boot case typing at the prompt or sending ^C, ^\\, ~B or a paste, 17 more; list in .tmp/B113/set.txt): DONE rc=0, set: 96 cases, 201 PASS lines (cases with several smp values count once per value), 0 FAIL, 0 SKIP, both widths, each job at most 4 cores. Log .tmp/B113/set.log. The orchestrator's "14-case shell set" has grown: `scripts/shell-cases`' shell pattern matches 79 boot cases now.

## The input change in three cases (for the reviewer)
Each of these used to send its next line in the same write as the interrupt
(`"{:after_one, x + 1}\n\u0003"`). Under the rule, that line is input queued when the interrupt is
taken and goes with it. Each case now sends the interrupt alone and the next line in a later
write.

The bench fires every pending `[[input]]` whose `after` matches a line (tools/testbench/src/qemu.rs
around line 788), so two inputs cannot both wait on `^C`:
- job-kill: one interrupt. The `{:after_fg …}` line is sent on `(^|\] )\^C$`.
- job-interrupt-line: first interrupt as job-kill. The second loop's line first spawns an unlinked
  watcher, `w = self(); spawn(fn -> r = Process.monitor(w); receive do {:DOWN, ^r, _, _, _} ->
  IO.puts("loop two" <> " ended") end end)`. The `{:after_two …}` line is sent on
  `(^|\] )loop two ended$`. The words are joined at run time, so the echo of the typed line can
  never match.
- job-interrupt-native: first interrupt unchanged (it already sent on `^C`). The pipeline's line
  spawns the same watcher ("pipe ended"), and `{:after_pipe …}` is sent on it.

What they prove is unchanged:
- The expect lists are the same, in the same order: `^C` drawn, then the next line's value, which
  still sees `x` or the session's budget restored, and Pipes `:not_running`.
- The next line is still typed while the interrupted line may be ending. It waits in the driver
  until the next prompt, as it waited in group before, so the budget and Pipes verdicts are still
  taken after the shell's settle.
- The watcher is an Erlang process, not a native stage: it holds no budget, and a docs rule says
  unlinked processes are not killed by Ctrl+C. It ends after printing.
- The verdicts are still the session's own output. The only thing that changed is when the bench
  types.

## Pages
- docs/userland/shell.md, "Interrupting and killing jobs": the rule, beside "What a job cannot do".
  It covers the machine mode's own rule (scoped out) and the residual: a paste's later chunks
  delivered after the interrupt is handled are new input until bracketed paste. The status line
  now says what the interrupt leaves of the input runs in the driver suite.
- docs/userland/shell.md, "Line editing and history": the driver hands group input a line at a time.
- docs/userland/shell.md, "Saved history, secret reads and pasting" Open: the pointer to the
  residual.
- tests/nscr-interrupt.toml comments state the rule and the residual (inputs unchanged).
- Three case inputs as above. The job-interrupt-line description is corrected; it had said the
  next line was typed ahead of the interrupt.

## Summaries checked
- README.md, GETTING-STARTED.md: no claim about interrupt or typeahead; no change.
- docs/plan/m2-usable-shell.md, the attack suite ("An interrupted line loses only itself", "No
  program swallows the interrupt") and the progress paragraph: still true; no change.
- docs/SECURITY.md, "The session" residuals: no paste item. The broader pre-existing gap (a
  pasted newline runs a line, bracketed paste not built) is not listed either. Raised here, not
  added: it is not this package's change.
- driver.ex moduledoc: updated. shell.ex moduledoc ("The interrupt … ends the line being read,
  or the line being evaluated"): still true; no change.

## Open risks / notes
- An interrupt that arrives after a line is taken but before the evaluator says it is
  `:running` still ends nothing (as before); the input queued with it is dropped.
- If a line turns off group's echo (`:io.setopts(echo: false)`, dumb mode) and then makes a
  multi-line read (`IO.read(:eof)`), the driver sees no second `{:requests}` and holds the input.
  Ctrl+C still ends the line.
- `typeahead` is unbounded in the driver, as group's mailbox was before; it is under the driver's
  heap limit.
- The machine mode's interrupt rule is unchanged: the same-read tail is the next block's.

## Fold after review (servers-red OK with notes), head 248b9c92d (amended; was c1837585b)
- P2: the gate is now decided by group's own state, not by a drawn prompt. At a line's end or a
  Ctrl+D, and whenever the gate is closed and group draws for a read or input arrives, the driver
  sends group `{:system, {sink, ref}, {:replace_state, fun}}`. The fun runs inside group and sends
  the driver `{:redoubt_reading, ref, input != undefined}`, and the state is returned unchanged.
  group answers in order after the input it was given, so the answer doubles as the sync the old
  no-op setopts gave. The system reply goes to a process that has ended (`sink`), so group's
  state is never copied. One asking is in flight at a time, with an ask-again flag. An interrupt
  that drops the line makes any asking in flight stale (its ref is forgotten).
  New test: "a read that goes on past a line's end with echo off is given what is typed after it"
  (`:io.setopts(echo: false); IO.read(10)`, "abc\r" then "defghi"). Red on c1837585b, beamlet
  and BEAM (the read never completed). Green on 248b9c92d.
- P3 cap: typeahead is at most 64 KiB (`@typeahead`). What a read would add past it is dropped
  from a character's start, and the bell rung (`:beep`). The rest of a read given at the prompt is
  capped the same way. New test: "type-ahead past 64 KiB while a line runs is dropped with the
  bell, and the interrupt drops the rest" (exactly one bell; the queue is still dropped by the
  interrupt). The cap is stated on shell.md.
- P3: shell.md now says an interrupt typed after a line is entered and before it begins to run
  ends nothing, but still drops the input queued with it.
- Pages: shell.md's rule paragraph, "Line editing and history" and the driver moduledoc now say
  that the gate follows group's pending read.
- Note: "a crash report of a process a line spawned …" failed once on the BEAM during the red
  run and once on beamlet against main's driver. It is unrelated to the driver's input path,
  passes in the full runs, and looks load-dependent.
- Gates on 248b9c92d: `./test-shell` (every stage) rc=0, beamlet 388 passed + 1 skipped, BEAM 319
  passed + 70 skipped; `jobs.mk set CASES="docs formatting"`: docs PASS, formatting PASS;
  prebuilt rc=0; job-interrupt-line, job-interrupt-native, job-kill and nscr-interrupt PASS on
  rv64 and rv32 (8/8). The 96-case shell set ran on c1837585b (rc=0); it was not rerun after the
  fold.

## Fold, round 2: head fffbbe491 (amended; was 248b9c92d)
- P2: the 64 KiB cap applies only to input that arrives while a line is evaluated (`evaluating`
  set) and no read is pending. The rest of a read given at a prompt waits whole (`sync/3`
  appends uncapped), and so does input between the reads of a multi-line paste.
  New test "a multi-line paste of more than 64 KiB into a waiting prompt arrives whole" (71 items,
  69,580 bytes). Red on 248b9c92d (the tail was cut). Green on fffbbe491, and green on main
  except for one bell: main rings it too on that paste, from group/edlin, so the test asserts the
  value, not the bell.
- P3: group is asked again when a line's evaluation ends (`:done`, or the evaluator's DOWN).
  New test "keys typed while a line runs, before its silent read begins, are given to that read"
  (sleep, then echo off and IO.read(5); "abcde" typed during the sleep). It was already green on
  248b9c92d: a read with echo off draws its prompt as `{:requests, [put_chars ""]}`, which made
  the driver ask. Asking at the line's end is the backstop the review asked for.
- P3: the ask/1 comment names the coupling to group's record as extracted from
  kernel/src/group.erl. On a mismatch the fun raises, sys keeps group's state, no answer comes,
  the gate stays and typed input waits; the interrupt still drops it.
- shell.md's cap sentence and the driver moduledoc now say the cap is for what is typed while a
  line runs, and that a paste into a waiting prompt is kept whole.
- Gates on fffbbe491: `./test-shell` (every stage) rc=0, beamlet 390 passed + 1 skipped, BEAM 321
  passed + 70 skipped; docs PASS; formatting PASS; prebuilt rc=0; job-interrupt-line,
  job-interrupt-native, job-kill and nscr-interrupt PASS on rv64 and rv32 (8/8).
- Flake noted: "a crash report of a process a line spawned …" failed in some single-file driver
  runs, against main's driver too (Task's report not shown within the wait). It passes in every
  full `./test-shell` run.
