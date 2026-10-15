# CONW2 report (b57-implementer-3, 2026-10-15)

Node CONW2: a kernel line never leaves a session's line bare. Worktree
`/home/mcloonan/redoubt/.worktrees/CONW2`, branch `wp-CONW2` from main 691646841. Ruling:
QA thread B57-split-line, kernel-red-2's last answer (event 36d485a3669c01cc916f5ce26df9c9b1), and
the node body. B57's code and docs are unchanged (wp-B57 at cd40d1eb2).

## Delivered

One bit each way on `console_hold`; the kernel never waits.

- **Wire** (`libs/sys`): `Hold::ReleaseOpen = 3` (give back, the caller's line unfinished); the
  call's result is `Return::Wrote(bool)`, a take's answer (false for a give-back), one register
  bit; a value other than 0/1 is a malformed result. `hold` 4 is `InvalidArgument` as 3 was. Round
  trips and malformed-result tests cover the new value and the new shape (`libs/sys/src/tests.rs`).
- **Hold** (`libs/conhold`): `line_open`, set by `release(pid, true)` only, cleared by
  `release(pid, false)`, by `died` (the kernel ends that line itself) and by the kernel's own
  `\r\n`; `wrote`, set by a free line that found the line open, cleared only by the `take` that
  answers it (`take` returns `Ok(wrote)`). `line()` returns `Line::Ending` once for a free line
  with the line open. Held lines, `Line::Full` and the 4 KiB bound unchanged. Two host tests.
- **Kernel** (`kernel/src/debug/console.rs`, `redoubt.rs`): `print` writes `\r\n` first on
  `Line::Ending`; `hold(pid, Hold)` takes (answering `wrote`) or gives back with `open` by the
  value; the dispatcher returns `Return::Wrote`. The state is `Output`'s `Hold`, under `PRINTER`,
  so a take, a give-back and a line on another hart serialize as before. No unsafe.
- **Runtime** (`libs/rt/src/handle.rs`): `console_hold` answers the bit; `console_held` passes
  `wrote` into the write and takes `open` out of it, giving back with 2 or 3. A refused take
  writes as before (told `false`, gives back nothing). `init` writes whole lines (`((), false)`).
  The fake kernel answers `Wrote(false)`, logs each hold with its answer
  (`Fake::console_holds`), and a test can say a kernel line ended the holder's line
  (`Fake::console_line_ended`), answered once.
- **consoled** (`servers/consoled`): `Lines::is_open` and `Lines::kernel_ended` (`open = None`:
  the next bytes start a line, prefix first); `Console::write` forgets the open line when the
  take says the kernel wrote, and gives back `ReleaseOpen` while a line is unfinished. Nothing
  else in `Lines` changes. At start `consoled` takes and gives back the hold once; a refusal is
  exit code 8, `NO_HOLD`. Host tests: `a_line_the_kernel_ended_starts_again_with_its_prefix`
  (tests/lines.rs: the prefix comes back, and only then);
  `an_unfinished_line_is_given_back_as_such_and_starts_again_once_the_kernel_ended_it`
  (tests/consoled.rs: the probe at start; `half` → Take, ReleaseOpen; after the kernel ended it,
  Take answered true then Release; the answer consumed by the next take).
- **Model** (`model/src/kernel.rs`): `hold` 3 accepted as a give-back; the generator already
  draws 0..3 for the hold (`gen.rs:324`, no new draw); the holder rule unchanged. The bits are
  about lines, which the model does not model (its page says so).
- **Case** `console-kernel-line-mid-line` (tests/, tests/data/init/kernel-line-mid-line.json,
  tests/init-programs `half-rest` and `faultee`): under the real init at 2 harts on both widths,
  `half-rest` writes `half` with no newline, calls `faultee`, which faults serving it (the call
  ends `Dead`, R4b, before the kill line), then calls again: that call is answered only by the
  instance init restarts, after the exit notice, which the kernel produces after the kill line
  (`terminate`, ptable.rs). Then `rest\n`. Expected, anchored: `[con N] half`, the kill line,
  init's fault and restart lines, `[con N] rest`, `[con N] half-rest TEST PASSED`; forbidden
  `^rest$`, `half` followed by anything but `-rest`, `half[!]`. The bytes are the kernel's and
  consoled's (rule F). faultee is labelled: its panic's words cannot reach the console (R69), so
  the kill line is the first thing after `half`. The first run, with faultee unlabelled, PASSED
  on both widths without exercising the kernel path: its panic line (`[con N] panicked: ...`)
  ended `half`'s line by consoled's writer rule before the kill line came (seen in the log, fixed
  before the report). PASS rv64 and rv32 at 2 harts, with the bytes
  `[con N] half` / `[!] Terminating process with PID n` / init's fault and restart lines /
  `[con N] rest` / `[con N] half-rest TEST PASSED`
  (target/testbench/run-2825346-1792024074861109907/console-kernel-line-mid-line-rv64-smp2.log:42-47,
  run-2825347-1792024074864856001 for rv32).
- **Recorded negative** (asked by the orchestrator): main 691646841's tree with only the case's
  files copied in (the toml, the manifest, `faultee`, `half-rest`, their Cargo bins and
  constants), a scratch worktree `$REDOUBT_TMP/CONW2/neg` (never in the CONW2 tree), prebuilt
  and run at 2 harts: FAIL rv64 and FAIL rv32, both on the first forbidden line,
  `[con ee73619157030960] half[!] Terminating process with PID 364` (rv64,
  neg/target/testbench/run-2878452-1792024602861708714/...-rv64-smp2.log:42) and
  `[con d17051c51f2c3365] half[!] Terminating process with PID 502` (rv32,
  run-2878453-1792024602861343651:42): the kill line inside the program's line, which is the
  defect. The bench stops the boot at that line, so the bare `rest` that would follow is not
  reached; `^rest$` is the case's second net, for a kernel that ends the line but a consoled
  that does not re-prefix. The positive at CONW2's head shows the same bytes split into
  `[con N] half` / the kill line / `[con N] rest`.
- Round 2 (kernel-red-2's P1/P2): the held path, where a kernel line is queued during the chunk
  that leaves the line unfinished and printed at the give-back, is covered by conhold's host
  test `lines_that_waited_end_an_unfinished_line_at_the_give_back_and_the_next_take_is_told`,
  not by a bench case: forcing a kill line inside consoled's 256-byte hold window from a program
  is a race, not a verdict. P2 (the answer is the releaser's) by
  `only_the_holder_whose_line_was_ended_is_told`.
- **Pages**: docs/kernel/devices.md "The console's one writer" (the new bullet: the line left
  unfinished between writes; errors line; status 10 → 13); docs/servers/consoled.md "Started by
  `init`" (the promise restated: never bare, not even after a kernel line broke it; status
  14 → 17), `/dev/cons` write, "Failure and restart" (`NO_HOLD`, 8); docs/kernel/abi.md's two
  rows. Crate docs in conhold, consoled's lib.rs.
- **Size budget** (tests/size-budget.toml): kernel 10652 → 10656, libs/sys 1033 → 1040,
  libs/conhold 70 → 79, with `Size budget:` lines in the commit that raises them. libs/rt is
  kept under its ceiling (see Gates).

## Gates (env per launch-env-2026-10-12; every command through scripts/q, 4 cores)

- `cargo test -p redoubt-conhold -p redoubt-sys -p redoubt-fake-kernel`: 0 (conhold 7, sys 14
  + result_edges 11).
- `cargo test -p redoubt-consoled`: 0 (consoled.rs 15, lines.rs 5, vectors 1).
- `cargo test -p redoubt-rt`: 0 (87 + the integration tests, all ok).
- `cargo check -p redoubt-rt -p redoubt-init -p redoubt-model -p redoubt-init-programs`: 0.
- `./build --arch rv64 --programs`, `--arch rv32`: 0, 0 (kernel, loader, servers; the unused
  `Hold` import it warned of is removed).
- `make -f scripts/jobs.mk docs`: PASS (after rewording "answers 1", which C4 reads as a QA
  reference).
- `rustfmt +nightly --check` on every changed .rs file: clean (after formatting two).
- `cargo testbench --exact --arch rv64 size-budget`: PASS at the head (libs/rt trimmed back to
  its ceiling by a one-line `console_hold`; the five raises are in the commit's trailers).
- `make -f scripts/jobs.mk prebuilt`: rc=0 (rv64 292 cases, rv32 278, 0 failed).
- `make -f scripts/jobs.mk set CASES=console-kernel-line-mid-line`: PASS rv64, PASS rv32 (above),
  beside train 23's bench.
- NOT RUN (wait for the orchestrator's word, per its instruction during train 23):
  beamlet-reduction-rate, beamlet-schedulers-mttcg, beamlet-schedulers-one-hart-mttcg at CONW2's
  head, and B57's three reruns. A scratch worktree `$REDOUBT_TMP/CONW2/b57` holds B57's two
  commits cherry-picked onto 11663e224 (the head before the case's two amendments; the kernel
  ceiling resolved to 10674 = 10656 + B57's 18) and is prebuilding; I will remake it on the
  final head before running them.
- The unsafe ratchet: no `unsafe` in any changed line.
- Files read in full before committing: every new file, conhold's lib.rs and tests.rs,
  consoled's server.rs, lib.rs and bin, tests/lines.rs, init-programs' lib.rs and Cargo.toml, the
  case toml and manifest, size-budget.toml, console.rs; by region (the changed functions and
  their neighbours): libs/sys call.rs, ret.rs, tests.rs; libs/rt handle.rs; the fake's lib.rs;
  kernel redoubt.rs; model kernel.rs; init.rs; consoled tests/consoled.rs; the three pages.

Head after round 1: 65a34fec4.

## Round 2 (kernel-red-2 BLOCK on 65a34fec4: P1 held path, P2 the answer's owner)

Amended into the one commit; head **dd46525b2** on 691646841.
- P1: `Hold::release(pid, open)` returns `Option<bool>`: `Some(end)` if it held; `end` when the
  line was left unfinished and lines waited, so the kernel prints `\r\n` before them and the
  releaser's next take is told (`wrote_for = pid`, `open_by = 0`). `died` the same, finished.
  Test `lines_that_waited_end_an_unfinished_line_at_the_give_back_and_the_next_take_is_told`.
- P2: `open_by` (whose line is unfinished) and `wrote_for` (whose line the kernel ended), pids
  not bools; `take(pid)` answers true only when `wrote_for == pid`, and only then clears it.
  Another holder's take is told nothing and consumes nothing. The console has one line: a holder
  that gives back finished finishes it for everyone (physically true; the two-holder mixing is
  not CONW2's). Test `only_the_holder_whose_line_was_ended_is_told`.
- Kernel: `hold()` prints `flush("\r\n")` on `Some(true)`; `died()` on `is_some()`. devices.md:
  "or before the lines that waited and go out at that give-back", "that holder's next take, and
  only its"; status 13 → 15. libs/conhold's ceiling 79 → 86 (the trailer stands).
- Gates at dd46525b2 (through q): conhold 9 tests ok; docs PASS; size-budget PASS; prebuilt rc=0
  both widths; console-kernel-line-mid-line PASS rv64 and rv32 at 2 harts, bytes `[con N] half` /
  `[!] Terminating process with PID 410` / init's two lines / `[con N] rest` (rv64 log lines
  42-47). sys, rt, consoled host tests unchanged since round 1 (their files did not change).
- The B57 scratch worktree was remade on dd46525b2 then; superseded by round 3 below.

## Round 3 (kernel-red-2 OK with notes on dd46525b2: P3 folded in) and the timing runs

Final head **a40d75bd9** on 691646841, still one commit.
- P3: `Hold::died(pid) -> bool`: a process that dies with its line unfinished (not the holder)
  has that line ended before the kill's own, and `open_by` / `wrote_for` naming the dead PID are
  cleared, so a later process given the PID is told nothing. The kernel's `died` prints `\r\n`
  then what waited whenever `died` is true (the page already said a dying holder's line is
  ended; the code printed the `\r\n` only with a non-empty queue). Test
  `a_process_that_dies_with_its_line_unfinished_is_owed_nothing_after`; devices.md's "A holder
  that dies" bullet extended; status 16; ceilings kernel 10656 → 10658, libs/conhold 86 → 95.
- consoled's end-to-end test raced the start-up probe (`boot()` returns at the UART's set-up,
  the probe comes after): it now waits for the two probe calls; 6 of 6 runs green. It had failed
  once inside the round-2 chain, which went on past it (the grep pipeline masked the status);
  caught and fixed before any timing run.
- Gates at a40d75bd9 (q): conhold 10, sys 14, rt 87, consoled 15 + 5 + 1 host tests ok; docs
  PASS; size-budget PASS; prebuilt rc=0 both widths; console-kernel-line-mid-line PASS rv64 (0.3
  s) and rv32 (0.4 s) at 2 harts.
- **Timing cases at CONW2's head, alone through q** (the bench done; the orchestrator's prebuilt
  and three light cases beside): beamlet-reduction-rate rv64 PASS 44.0 s rc=0, rv32 PASS 43.1 s
  rc=0; beamlet-schedulers-mttcg rv64 PASS 20.8 s rc=0, rv32 PASS 20.6 s rc=0;
  beamlet-schedulers-one-hart-mttcg rv64 PASS 23.6 s rc=0, rv32 PASS 22.9 s rc=0.
- **B57 scratch** `$REDOUBT_TMP/CONW2/b57` = a40d75bd9 + 1850a2b85 + ed5b2d1c3 (B57's two
  commits cherry-picked unchanged, the kernel ceiling resolved to 10676 = 10658 + 18), prebuilt
  rc=0 both widths; then, alone through q: beamlet-reduction-rate rv64 PASS 43.9 s rc=0, rv32
  PASS 43.0 s rc=0; beamlet-schedulers-mttcg rv64 PASS 21.1 s rc=0, rv32 PASS 20.3 s rc=0;
  beamlet-schedulers-one-hart-mttcg rv64 PASS 22.6 s rc=0, rv32 PASS 22.2 s rc=0; launch-idle
  rv64 PASS 66.4 s rc=0, rv32 PASS 66.3 s rc=0.
- **The bytes around the fall line** (the failure's own case, now with the fix):
  reduction-rate rv64 (b57/target/testbench/run-2947021-1792025857332465568/...-rv64-smp1.log):
  `[con 6e787df40e9b7ce5] ` (the `\r` write, ended by the kernel) / `mem: frames in flight 0, at
  most 24439` / `[con 6e787df40e9b7ce5] :timed` / the next prompt; rv32
  (run-2947022-1792025857335197626): the same with `at most 24701`. Each log has 3 `frames in
  flight` lines, all bare. launch-idle rv64: `mem: frames in flight 0, at most 35113` then
  `[idle] drain: back to 0 frames in flight 0.35 s after the step began ... 0.570 cores`, the
  window's `[idle] 60.0 s: QEMU 0.003 host cores`; rv32: `at most 35694`, `0.41 s`, `0.737
  cores`, `0.003 host cores`.
- B57 itself is unchanged at cd40d1eb2; when it rebases onto CONW2 its kernel ceiling line
  becomes 10676 (the only conflict, tests/size-budget.toml).

## Pages and summaries checked

- docs/kernel/devices.md, docs/servers/consoled.md, docs/kernel/abi.md: updated (above).
- docs/kernel/model.md: says the hold's lines are not modelled; the bits are the lines'; no change.
- docs/kernel/README.md, README.md, GETTING-STARTED.md, docs/README.md, docs/servers/README.md:
  no claim about the console's hold or bare lines (grep `console_hold`, `bare`, `one writer`).
- docs/testbench.md: case list is not enumerated there; no change.
- docs/plan/m2 progress: CONW2 is a follow-up of a merged package; nothing to move.

## Open risks

- The case's ordering rests on init restarting `faultee` only after the kernel's exit notice,
  which `terminate` produces after its kill line. If a run shows the kill line after `rest`, the
  case has found a kernel ordering worth a finding, not a flaky expectation.
- `NO_HOLD` at start: a system whose device tree marks another device as the kernel's console
  now refuses to start `consoled` where it used to write unprotected (ruled: lines the kernel can
  break are not served).
- A line longer than `HOLD_CHUNK` is still split across holds by design (kernel-red-2's note);
  the page says so, unchanged.

## Round 4: rebased onto today's main

`git rebase --onto 92b84a5b8 691646841 wp-CONW2` (K33 had raised the kernel ceiling to 10757 in
between, the one conflict): the ceiling set to the gate's count at the new head, **10763**
(CONW2's own 6 lines over K33's count); libs/sys 1040, libs/conhold 95, servers/consoled 408,
servers/init 2546 and libs/rt 3924 are still exactly their counts. The `Size budget:` lines are
unchanged. Head **d9123b273** on 92b84a5b8, still one commit.

Light gates at d9123b273 (through q): conhold 10, sys 14 (+ result_edges 11), rt 87 and its
integration tests, consoled 15 + 5 + 1: all ok; size-budget PASS; docs PASS; rustfmt --check
clean; prebuilt rc=0 both widths; console-kernel-line-mid-line PASS rv64 (0.3 s) and rv32
(0.4 s) at 2 harts. The three beamlet timing cases ran on the identical code at the old base
(Round 3); the train's whole bench covers the new one.
