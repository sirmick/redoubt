# AGT2 report: the shell's machine mode

Base: wp-AGT1's head 0771a430f, which sits on 3a80e5af0 (twelve commits behind main 32a448875, not on it as the train plan said; `git rebase --onto 0771a430f 32a448875` replayed my one commit). Head: wp-AGT2 a1c21d0f3 (one commit; earlier heads 4af218700, 936dc4760, 565b9cd37, cb28d2d75 on 32a448875). Worktree: /home/mcloonan/redoubt/.worktrees/AGT2.

Train fold (docs only) at a1c21d0f3: AGT1's numbers folded to the new session size in `image/README.md` (11,392; two sessions 22,786; margins 1,025 alice per share, 9,981 bob, 1,789 agent; two sessions per share kept) and `docs/kernel/budgets.md` (the conflict with AGT1's agent line resolved keeping both; the second passage's 22,530 → 22,786); `docs/userland/beamlet.md`'s share history trimmed to what is (the step rule, the 5,610 bound, the mode's 19 pages). Docs checker rc 0. The measured gates stand from cb28d2d75: the code and manifests are byte-identical, only these three pages changed.

## Train 36 follow-ups at cb28d2d75

### beamlet-footprint: measurements and the rule applied

- Measured alone in this worktree, `make set CASES=beamlet-footprint WIDTHS="rv64 rv32"`, three runs at my head: rv64 **5,542** ×3; rv32 5,360, 5,358, 5,362. Baseline, main 32a448875's shell files checked out in this worktree over the same prebuilt recipe, two runs: rv64 **5,523** ×2; rv32 5,343, 5,338. So main itself is 31 over the pages' 5,492 (older drift, not measured again by B72's successors), and the machine mode adds **+19 on rv64, +17–22 on rv32**. The VM's own rows (rv64, mine vs main): code.instrs 2,190,080 vs 2,177,792 B (+3 pages, 1,394 instructions), code.operands 8,881,152 vs 8,827,904 (+13 pages, 3,233 operands), literal table +352 B, accounted 3,968 vs 3,951 pages. All of it is the driver's and the loop's code at the prompt; nothing is eagerly loaded that the mode could load on entry (Control and its help are not loaded at the prompt: the index holds names only). Logs: `.tmp/AGT2/footprint/`, `.tmp/AGT2/footprint-base/`.
- Why the rule and not a trim: a `Redoubt.Shell.Machine` module loaded on entry would recover perhaps 12 of the 19 pages and leave train 36 (5,556 with AGT1) about 3 pages under the 5,546 step: the next package moves it anyway. Applied: 2 × 5,556 + 44 = 11,156 → 11,264 (88 × 128) for the VM, cap 11,220 (108 over twice 5,556; the peak may reach 5,610), session 11,392.
- What moved: `image/manifest.json` sizes.session.pages 11264 → 11392; the same in `tests/data/steward/idle.manifest.json`, `tests/data/steward/vault-launch.manifest.json`, `tests/data/pipe/manifest.json`, `tests/data/boot-profile/manifest-unverified.json`; `tests/data/beamlet-footprint/manifest.json` heap_pages 11092 → 11220 and budget 11136 → 11264. The steward has no constant: it takes `sizes.session.pages` from the manifest and derives the cap (11,392 − 128 relay − 44). Pages: `docs/kernel/budgets.md` (session 11,392, VM 11,264, cap 11,220, peaks 5,556 / 5,542 / 5,523 and 5,362, bound 5,610, the 11,136 step in the history), `docs/userland/beamlet.md` (the footprint table refreshed from this measurement on both widths; the share paragraph, with the step's history), `docs/testbench.md` (11,220 / 11,264 / 11,392, 5,556). Checked and unchanged: `docs/servers/steward.md` "shares each hold two sessions" (Alice's halves ≈ 23,812 and Bob's 32,768 hold 2 × 11,392 = 22,784 and not three; `init`'s contexts cap stays 2); Alice 47,624 / Bob 32,768 top budgets.
- Gates on the rule, both widths: beamlet-footprint PASS (rv64 5,542 of 11,220; rv32 5,356), steward-home-quota PASS (215.7 s / 173.7 s), init-boot PASS, shell-machine PASS (rv64), size-budget PASS, formatting PASS; prebuilt rv64 318 / rv32 306, 0 failed; docs checker rc 0; whole `./test-shell` every stage passed (beamlet 377, BEAM 308). Logs `.tmp/AGT2/gates6/`, `.tmp/AGT2/test-shell-5.log`.

### job-interrupt-native: not the driver change

- Alone, three runs on both widths of job-interrupt-native, job-interrupt-line, job-interrupt-ssh, nscr-interrupt, pipe-interrupted and job-kill: 36 of 36 PASS. job-interrupt-native 5.7 / 5.6 / 5.4 s on rv64, 5.4 / 5.5 / 5.4 s on rv32, against the 450 s deadline; the slowest of the set job-interrupt-ssh at 11.3 s. Logs `.tmp/AGT2/interrupts/`.
- The case drives the feed path (`exec` of a spinning stage, Ctrl+C at the console). Outside the mode that path is unchanged: the machine clause of `keys/2` matches only a map, `interrupt_line/1`'s new arm is `state.machine != nil`, and the feed's own host test ran green in every `./test-shell`. The train's 450 s timeout coincided with my prebuilt builds and case runs on this host (11:00–11:30): a load flake to note.

## Renewal fold (steward-red-2, P2-1 reopened, P3-2) at 565b9cd37

- P2-1 (authentication): the driver matched the mode's messages on the pid in the body, so a block knowing the shell's pid (`:monitored_by`) could forge `:next`, `:mark`, `:close`. Now every `:open`, `:next`, `:mark` and `:close` carries a key: a reference the driver mints once for the first process that asks (`Driver.machine_key/1`, the shell at `run/1`'s start, before any line) and keeps with that pid; messages without it are dropped from the mailbox unanswered and not counted (said on the page). Minted at the shell's start rather than at `:opened`, as the reviewer suggested, because `:open` needs it too: a block at the plain prompt could otherwise send `:open` and wedge the session. The key is in the shell's loop state (`state.key`, not in `limits`, which the evaluator gets) and the driver's `keyed`. Test: a block forges `:next`, `:mark` ("FORGED 99 ok") and `:close`, each with the shell's pid and with a fresh ref: no line pulled (`{:messages, []}`, the next block's token honoured), nothing drawn, the mode intact.
- P3-2: the Why now says the token and the key are in no process a block can read, the driver's own mailbox the one residual; the section's residual sentence matches.
- Gates on 565b9cd37: docs checker rc 0; whole `./test-shell`: every stage passed (beamlet 377 passed / 0 failures, BEAM 308 passed; `.tmp/AGT2/test-shell-4.log`); prebuilt rv64 300 / rv32 285, 0 failed; bench:shell-machine rv64 PASS 5.8 s (`.tmp/AGT2/run4-shell-machine.log`).

## Review fold (steward-red-2, P2-1, P2-2, P3-1) at 936dc4760

- P2-1: lines are now pulled, not pushed. The driver queues each line (`:queue`) and the shell asks for one at a time (`{:redoubt_machine, :next, shell}`) in `read_block/3` only, so during a block no later line is in the shell's mailbox. No race: the hold does not depend on `evaluating`. Residual on the page: the driver's own queue and unread console bytes in its mailbox, readable as any mailbox is; a program wanting none sends the next block after the mark. Test: two blocks in one write, the first reads its shell's mailbox (`Process.info(self(), :monitored_by)` names the shell) and prints `{:messages, []}`.
- P2-2: the mark is drawn by the driver for the shell (`{:redoubt_machine, :mark, shell, text}` → `:marked`), with a newline first when `term.origin != 0` (output left the line open), through `draw/2` and so the guard. Test: `IO.write("partial")` then the interrupt, and `IO.write("bye"); exit()`: the mark is on its own line each time.
- P3-1: `@machine_line` 1 MiB; a longer line is dropped whole to its newline, the shell told `{:redoubt_machine, :long, bound}`, and the block is marked `error` with "a line of the block was longer than 1048576 bytes, so the block was not evaluated". Test: a 1 MiB + 1 line, then a normal block runs.
- Gates on 936dc4760: docs checker rc 0; whole `./test-shell`: every stage passed (formatting, native, beamlet 376 passed / 0 failures, BEAM 307 passed, entry_point, terminal, fake kernel; `.tmp/AGT2/test-shell-3.log`); prebuilt rv64 300 / rv32 285, 0 failed; bench:shell-machine rv64 PASS 5.8 s (`.tmp/AGT2/run3-shell-machine.log`).

## Paths

- `userland/shell/lib/redoubt/shell/driver.ex`: the mode's state and messages (`{:redoubt_machine, :open|:close, shell}`, `:opened|:refused|:closed`, `:line|:interrupt|:eof` to the shell); bytes cut into lines past `group`; interrupt and Ctrl+D in the mode; screens, the feed and the screen size refused; the entering line cut from `group`'s history.
- `userland/shell/lib/redoubt/shell.ex`: `read_block/2`, `block/2`, the mark (`ended/2`), `enter_machine/2`, `leave_machine/1`.
- `userland/shell/lib/redoubt/shell/evaluator.ex`: `{:machine, token}` from the `:redoubt_shell_machine` throw.
- `userland/shell/lib/redoubt/shell/control.ex`: `machine/1` (token rule, needs the shell's terminal).
- `userland/shell/lib/redoubt/screen.ex`: `{:redoubt_screen, :refused, :machine}` → `ArgumentError` in the caller; `terminal_size/0` takes `:machine` as `:none`.
- `userland/shell/test/redoubt/shell/driver_test.exs`: 7 tests (entering/leaving/history, multi-line block with tab and CRLF, error/too-soon/interrupt/exit, forged mark, refused tokens, screen refused + long value whole, interrupt mid-block + Ctrl+D).
- `tests/shell-machine.toml` (rv64).
- `docs/userland/shell.md`: "The machine mode" after "The loop"; `machine/1` in the session commands table; a Why paragraph. `docs/plan/m2-usable-shell.md` Progress: one clause.

## Design: why not the feed or group's echo-off reader

The brief pointed at the feed. Neither it nor `group` with `echo: false` fits: `group`'s dumb reader (`edit_line_dumb`, OTP 28 `group.erl`) drops every control byte under 0x20 (tabs), edits backspaces and still draws the prompt; and `{'EXIT', Drv, interrupt}` with no read pending kills the shell process. The feed echoes, edits and drops controls too. So the driver reads the mode's bytes itself (~60 lines), and the shell receives lines; the feed is untouched.

## Open-list decisions taken (written as rules in the section)

1. Mark format: `<token> <n> ok|error|exit`, one line, after the block's output. The entering line is marked under the token it set (so the program knows the mode is on and the counter); the leaving block under the token it was sent with; an empty block counts and is marked `ok`; `exit`, Ctrl+D and the console's end mark `exit`.
2. EOF: a block ends only by its token line. Ctrl+D at a line's start ends the session (as at an empty prompt). Reason: a program has the token; two ways to end a block buy nothing.
3. Width: unchanged; the program sets its pty's size. Reason: the output stays what a person would see; no hidden resize.
4. The pager prints whole (not an error): the lines are what the program asked for, and the pager already prints whole where no size is known. Screens, the editor and the feed are refused with an error.
5. Beyond the brief: the entering line is cut from `group`'s history, since `:sys.get_state(group)` reads it from any process; otherwise "the evaluated code cannot read the token" would not hold.

## Gates

- `scripts/q run -- cargo run -q -p redoubt-doccheck`: rc 0 (three runs; the last on the committed pages).
- `scripts/q run -- ./test-shell` (whole): formatting ok (after a two-blank-line fix), native ok, on_beamlet 373 passed / 1 skipped / 0 failures, on_beam 304 passed / 70 skipped, entry_point ok, terminal ok, on_fake_kernel ok. Log: `.worktrees/AGT2/.tmp/AGT2/test-shell-2.log` (its formatting failure was fixed; `mix format --check-formatted` rc 0 after; the driver file then ran 38/38 twice on the BEAM).
  - First run only: `a crash report of a process a line spawned...` failed on both VMs with different assertions while beamlet was being built for the fresh worktree; passes alone and in sequence (3 + 2 runs). Load flake, pre-existing test.
- `make -f scripts/jobs.mk prebuilt`: rv64 301 cases 0 failed, rv32 285 0 failed. `make -j4 -f scripts/jobs.mk set CASES=shell-machine WIDTHS=rv64`: PASS 5.7 s (run-2479436-1792088732348355388; logs copied to `.tmp/AGT2/run2/`).

## Finding: defmodule at the prompt faults the VM on the machine (not mine; blocks the brief's block as written)

First shell-machine run with the brief's three-line `defmodule` block: the session ended silently after the first mark; console: `PROGRAM HALT: CPU Exception on PID 420: Store page fault of 0x7ffecae8 at 0x002e2b70`. A probe case (not committed) typing the same `defmodule` at the ordinary prompt over SSH faults identically (PID 211, same addresses); `{:one, 1 + 1}` before it is fine. No existing machine case compiles a module at the prompt. On host beamlet (piped console and the driver tests) the same block compiles. Likely beamlet's 18-page scheduler stacks on Redoubt vs the compiler's recursion (userland/otp, not my path). The bench case therefore binds a three-line `fn` instead; the section records the residual. Logs: `.tmp/AGT2/run1/` (first run), `.tmp/AGT2/run2/agt2-probe-*` (probe). Follow-up for the orchestrator: a beamlet package (stack size or a compile-at-prompt case), then the case can send a `defmodule` again.

## Summaries checked

- `README.md`, `GETTING-STARTED.md`: no claim about the shell's modes or the agent loop; no change.
- `docs/plan/m2-usable-shell.md` Progress: clause added. "Remaining work": no item for this; no change.
- `docs/plan/m3-agents.md`: no mention of the loop's transport yet; no change (linked from the section as the milestone).
- `docs/userland/shell.md` "The loop" status (host only, no bench case): still true of the loop itself; the new section carries the case. "Session commands" table: row added. "Hostile text" section: the mark is a line through the same guard; no claim changes.
- `docs/userland/beamlet.md`: the fault is a new observation; not my page (hotspot: docs; one writer); named in the residual and here.
- Crate/app READMEs: `userland/shell` has none beyond the moduledocs, which I updated (Shell, Driver, Evaluator, Control's help).

## Risks

- A block's lines typed ahead during an evaluation are kept (pipelining); a Ctrl+C typed then ends the evaluation and does not drop them (documented).
- `drop_newest_history` runs in a spawned process (as `trim_history`), so for a few microseconds after entering, the entering line is still in `group`'s state.
