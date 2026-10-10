# SHELL10 report

Branch `wp-SHELL10` (worktree /home/mcloonan/redoubt/.worktrees/SHELL10), rebased onto origin/main
ef2ad5d3a. Head b0ea0ff99 (shell d4e97c05a, sshd b0ea0ff99); was 031022e15 on 415c86ad6. Not pushed. Tier A, strict track.

## Commits

1. b44778980 `shell: Ctrl+\ is the session's own key, and a screen may take Ctrl+C`
   - `userland/shell/lib/redoubt/shell/driver.ex`:
     - `@session_key 0x1C`. At the prompt, `keys/2` scans for 0x03, 0x04 and 0x1C; 0x1C takes
       the interrupt path Ctrl+C takes (it draws `^C`, sends exit `:interrupt` to group, and
       drops the rest of the read). It is never sent to group.
     - A screen opens with `{:redoubt_screen, :open, pid, :interrupt | :key}` (the guard accepts
       only those two atoms). The screen's `interrupts` are `[0x1C]`, or `[0x1C, 0x03]` unless it
       takes Ctrl+C.
     - `screen_input/2` finds an interrupt byte in the raw bytes before `Keys.decode`. Keys
       before it reach the screen; then `interrupt_screen` runs, and the rest of the read is
       dropped. Escape sequences (0x20–0x7E after ESC [) and UTF-8 continuation bytes (≥0x80)
       never hold 0x1C or 0x03, so a pending ESC or a paste cannot carry one to the screen.
     - The old `send_keys` clause for `{:key, "c", [:ctrl]}` is removed; the raw scan replaces
       it. As a side effect, ESC followed by 0x03 (Alt+Ctrl+C) now interrupts a default screen
       too, where before it was forwarded.
   - `userland/shell/lib/redoubt/screen.ex`: `run(module, args, opts \\ [])`, with
     `ctrl_c: :interrupt | :key` (default `:interrupt`); any other value raises ArgumentError.
     `run/2` keeps working through the default.
   - Doc comments only: `shell.ex`'s moduledoc and interrupt comment.
   - Help: `userland/shell/help/elixir.md` gains "Stopping" (Ctrl+C, Ctrl+\).
   - Tests:
     - `screen_test.exs` (beamlet only):
       - Ctrl+\ ends `pick` with nil.
       - A screen that traps exits and takes Ctrl+C (`Keys`) is sent `{:key,"c",[:ctrl]}`, and
         Ctrl+\ still ends it (killed).
       - No screen can swallow Ctrl+\: after a pending lone ESC (esc_timeout 5 s), a read of
         `\x1Ccd` ends the screen, and the screen is sent no key after the ones before. This is
         the pasted-0x1C case.
     - `driver_test.exs` (both VMs): Ctrl+\ at the prompt drops the line (`^C`), the rest of
       its read goes with it, and no `^\` ever reaches the line.
   - Pages:
     - docs/userland/shell.md: "Interrupting and killing jobs" (the interrupt key list, With a
       full-screen program, Over SSH, What a job cannot do); "Full-screen programs" (A screen's
       life: `run/3` and `ctrl_c: :key`; Keep the interrupt from the session); "Widgets..." (the
       session's-key bullet replaced by "A key per principal": the same for every session,
       per-principal planned and not built; **Open: none**, the item settled); "Line editing"
       (three keys).
     - docs/plan/m2-usable-shell.md ("No program swallows the interrupt").
     - GETTING-STARTED.md (Ctrl+C or Ctrl+\ ends the line).
2. 031022e15 `sshd: a channel's INT and break reach the session as Ctrl+\`
   - `servers/sshd/src/console.rs`: `INTERRUPT` is 0x1C, with its module docs.
   - `servers/sshd/tests/console.rs` asserts 0x1C.
   - `docs/servers/sshd.md` ("A pty session").
   - Why: with 0x03, a screen that takes Ctrl+C would be sent INT/break as a key. With 0x1C
     they stay the interrupt, at the prompt and under any screen.

## Gates (exact commands, all through q or jobs.mk)

- `q run --cores 8 -- ./test-shell`, at the final shell code: exit 0, "every stage passed".
  Formatting ok, native ok, on_beamlet 159/159, on_beam 148 passed and 11 skipped (the screen
  tests, which need beamlet), entry_point ok, terminal (pty) ok, on_fake_kernel ok.
- `q run -- cargo test -p redoubt-sshd`: exit 0, before and after the rustfmt amend.
- `make -f scripts/jobs.mk prebuilt`: rv64 236/0 failed, rv32 222/0 failed.
- `make -k -j -f scripts/jobs.mk set CASES="$(scripts/shell-cases origin/main) <all 7
  sshd-loopback-* cases>"`: exit 0. That is 37 cases, 67 PASS lines (30 on rv64 and rv32, 7
  ssh-loopback cases once each, as they run rv64 only), including beamlet-footprint on rv64 and
  rv32 and sshd-loopback-interrupt. The case list is in $REDOUBT_TMP/SHELL10/cases.txt.
  shell-cases chose beamlet's set because servers/ changed.
- The set ran before the rustfmt amend of 031022e15, which only rewraps a doc comment.
- `cargo run -p redoubt-doccheck`: exit 0, after the last page edit.
- Formatting: `mix format --check-formatted` ok. `rustfmt +nightly --check --edition 2024` is
  ok on both sshd files.
- Not run: the full bench and difftest, which are not asked for this package. The unsafe count
  and size budget are untouched: no unsafe and no new crate.

## Summaries checked

- Updated: docs/userland/shell.md, docs/servers/sshd.md, docs/plan/m2-usable-shell.md,
  GETTING-STARTED.md, userland/shell/help/elixir.md.
- Checked, no change needed:
  - help/terminal.md names no keys.
  - docs/userland/native.md (lines 195–200, "Ctrl+C destroys the budgets ... in M2") still holds
    for jobs. Ctrl+\ does the same; that page names Ctrl+C as the interrupt with jobs planned.
  - docs/userland/beamlet.md:397 ("every byte typed reaches the VM, Ctrl+C included") is still
    true.
  - The sshd-loopback-interrupt case and tools/sshd-host: the host platform logs a break as
    `console interrupt` and a typed 0x03 as `console interrupt byte`, which are unchanged. Its
    description ("a 0x03 byte reach the console as its interrupt") is still what the host
    platform does.

## For the red

- Rule F: the verdicts come from the driver and screen tests on the terminal model, and from the
  screen's own reports of the keys it was sent. The test's screen is the would-be attacker
  (it traps exits and takes Ctrl+C). The verdict is the driver ending it plus the absence of any
  key after the interrupt, not anything the screen claims.
- The `:open` message is sent by the screen's own process, so a screen's code could send
  `:key` itself. That only gives it Ctrl+C, never 0x1C, which is the property.
- Open risk: per-principal key choice is unbuilt (stated on the page).

## Rebase onto ef2ad5d3a

- Clean, with no conflicts. shell.md "What a job cannot do" gains one sentence, folded into the
  shell commit: under a screen that does not take Ctrl+C, ESC then Ctrl+C is the interrupt,
  never Alt+Ctrl+C.
- `q run --cores 8 -- ./test-shell`: exit 0, every stage. Beamlet 159/159; BEAM 148 passed and
  11 skipped; pty and fake kernel ok.
- doccheck: exit 0.
- Not rerun after the rebase: the machine set, which ran on 415c86ad6.
