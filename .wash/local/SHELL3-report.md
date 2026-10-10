# SHELL3 report

Branch wp-SHELL3, worktree /home/mcloonan/redoubt/.worktrees/SHELL3, head 5f349a851 on main
3b049cacc. Six logical commits, nothing uncommitted, never pushed.

## Commits (oldest first)

1. ca9258f42 beamlet: a resource can declare its size, and its holders count it as their own memory
   (Resource::sized, Ctx::new_resource_sized, Ctx::resize_resource; held bytes count toward a
   process's max_heap_size).
2. f67066dd6 beamlet: the screen buffer's natives, primitives over cells (new crate
   userland/otp/screen, module redoubt_screen: new/resize/put/fill/plot/diff; wide.rs generated
   by tools/gen-width.escript from OTP unicode_util:is_wide/1, Unicode 16.0, `--check` in
   test-shell).
3. b54158281 shell: a grapheme's width is OTP's (Redoubt.Term.Width on :unicode_util.is_wide/1).
4. 2c83ec6f5 shell: a key decoder (Redoubt.Term.Keys, pure; the Esc timeout is the caller's).
5. 8a988e5cf shell: the encoder draws a cells frame, on the alternate screen (Redoubt.Term.Frame).
6. 5f349a851 shell: screens: Redoubt.Screen, its layout, and pick (Term.Buffer, Screen, Layout,
   Widgets, Pick, driver screen mode, pty test, docs).

## Checks on 5f349a851 (bash .tmp/SHELL3/final.sh; all through q)

| Check | Result |
| --- | --- |
| ./test-shell | exit 0: every stage passed; beamlet 146 passed; BEAM 140 passed, 6 skipped (screen tests, beamlet only, reason stated); terminal (pty) stage ok; gen-width --check ok |
| cargo test -q -p beamlet-vm | exit 0, 84 passed |
| cargo test -q -p beamlet-screen | exit 0, 23 passed |
| cargo test -q -p beamlet | exit 0, 8 passed |
| cargo build -p beamlet-redoubt --target riscv64imac-unknown-none-elf; cargo check --features fake | exit 0 |
| userland/otp/tools/difftest erlang | exit 0, 43/43 passed, 1 skipped by design |
| make -f scripts/jobs.mk prebuilt | exit 0; rv64 230 cases 0 failed; rv32 216 cases 0 failed |
| make -k -f scripts/jobs.mk rv64/docs rv64/formatting rv64/size-budget rv64/unsafe-budget rv64/no-cruft | exit 0, all PASS |

Not run: the whole bench (train's job) and rv32 gates beyond prebuilt's rv32 build. No new
`unsafe` (unsafe-budget passes unchanged). No attack cases: no capability or cross-label input.
Logs: /home/mcloonan/redoubt/.tmp/SHELL3/{test-shell,cargo-*,machine,difftest,gates}.log.
(final.log shows each line twice: the earlier, interrupted run finished alongside the rerun;
both runs were green.)

## Demo (the pty test, host:beamlet::pick_on_a_terminal_takes_the_screen_and_gives_it_back_with_the_choice)

At the prompt `pick(["apple", "banana", "cherry"])`, then Down, then Enter: the shell enters the
alternate screen (`ESC[?1049h`), draws a box with the three items, the cursor moves to banana,
Enter leaves the alternate screen (`ESC[?1049l`), `"banana"` is printed at the prompt, `exit`
ends the shell with status 0, and the terminal's termios are restored.

## Summaries checked

- README.md, docs/README.md, docs/userland/README.md: list screens/full-screen programs as
  features or M2 goals; no status claim; no change needed.
- GETTING-STARTED.md: updated (the terminal stage of test-shell).
- docs/plan/m2-usable-shell.md: Progress updated.
- docs/userland/shell.md: terminal library and full-screen programs split into built and planned
  sections; hostile-text status updated.
- docs/userland/beamlet.md: Process memory (sized resources) and Screen natives built.
- userland/otp/README.md, DESIGN.md: no crate listing; no change needed.

## Findings (outside SHELL3, not fixed)

1. beamlet gap: a `receive` typed at the shell prompt fails: erl_eval calls prim_eval:receive/2;
   prim_eval is in vm/src/vm.rs RUNTIME_MODULES (never loaded) and no native answers it. Tests use
   module functions instead.
2. Atomics (up to 2^24 cells) and zlib streams (up to 256 MB queued) are resources not counted
   toward any heap limit; Ctx::new_resource_sized (commit 1) could close it.
3. Review: Tier A per the orchestrator: red team, then simplifier, then editor.

## Open risks

- Reserved screen key stays Open on shell.md (candidate Ctrl+\); Ctrl+C ends a screen until one
  takes it as a key.
- Screens run on beamlet only; the BEAM suite skips them.

## Review round 1 (red, simplifier, editor: OK with notes on 5f349a851)

Folded into the owning commits; no fix-up commits. New head: see the question sent to the
orchestrator.

- Red P3 (driver `{:EXIT, ^group, _}`): writes Frame.leave() when a screen is open. Test:
  screen_test "a shell that ends under a screen leaves the main screen shown" (fails with the
  line removed).
- Simplifier #1, taken with a change: the driver sends Process.exit(pid, :interrupt) and then
  Process.exit(pid, :kill). run/2 maps a DOWN with :interrupt to nil; the :interrupted word and
  the caller in the open message are gone. Why the :kill stays: a screen that traps exits gets
  :interrupt as a message; if its update swallows it, the screen never ends, its line hangs and
  Ctrl+C (no line open) is dropped: the page's rule that a screen cannot keep the interrupt from
  the session would break. Signals between two processes arrive in order, so a non-trapping
  screen dies with :interrupt (nil, no race); a trapping one is killed and its line fails with
  {:screen, :killed}. Test: "Ctrl+C kills a screen that traps exits, and the session goes on"
  (fails without the :kill). shell.md's interrupt-key bullet says so.
- Red cosmetic (second screen killed silently): a second screen now exits with
  :another_screen_in_front (before any of its module's code runs, so it cannot trap), and a
  refused frame ends the screen with :refused_frame then :kill; the line's exit names the cause.
  No new test: a second screen needs a concurrent line.
- Simplifier #2 (start_timer) taken; #3 taken (modifier `for`, Buffer.plain in Frame folded into
  the screens commit, since Buffer comes there; widgets uses Buffer.style(reversed: true)'s bit);
  #4 taken; #5 taken (and the now-unreachable nil branch of next_grapheme removed); #6 taken;
  #7 taken (SPACE const, partition_point, nth(1), hoisted empty skip); #8 taken (u16_of: a put
  coordinate past 65535 is now badarg, as a Rect's already was; count checked before
  allocating); #9 taken; #10 taken (one map_reduce yields the rects).
- Editor 1-4 taken as worded (and pick.ex's moduledoc "The first screen" -> "A screen").
- Red beamlet.md note: Process memory bullet now says only a heap counts a sized resource, not
  a queued message or ETS table, bounded for the screen buffer by four a process and the
  mailbox limit; any other sized resource needs its own bound.
- Commit 6's message updated for the interrupt, the shell's end and the new tests.

Focused runs: beamlet screen_test 8/8, layout 13/13; BEAM suite 140 passed, 8 skipped;
cargo test beamlet-screen 23, beamlet-vm sized 3+; clippy clean on both crates.
