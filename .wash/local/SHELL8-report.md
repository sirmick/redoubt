# SHELL8 report: what the VM logs goes through the shell's guard

Branch wp-SHELL8, worktree /home/mcloonan/redoubt/.worktrees/SHELL8, on main 80400a1d4, clean,
not pushed. Two commits:

- **a2134dad5**, a fix of SRV1's, not SHELL8's. `userland/otp/redoubt/Cargo.toml`'s `fake`
  feature now also takes `redoubt-fileserver`, and `userland/otp/Cargo.lock` gains the one
  dependency line (my question said the lockfile would not change; it gains that line). Without
  it, ./test-shell stops before its first stage on main: the fixture builds littlefsd's program
  from source, and that program now names the crate. Placement is the orchestrator's call: it is
  its own commit at the branch's base, and can move to a branch of its own.
- **a5a8e066c**, SHELL8:
  - `userland/shell/lib/redoubt/shell/log.ex` (new): `Redoubt.Shell.Log`, the handler and its
    relay.
  - `driver.ex`: installs the handler after `:group.start` and uninstalls it in an `after`, and
    draws `{:redoubt_shell_log, line}`.
  - `driver_test.exs`: four tests, and `start/2` takes the terminal's rows.
  - `docs/userland/shell.md`: "The loop", and "Hostile text never drives the terminal" (its status
    line, a new paragraph, and "What it does not cover").

## The design, per the approval's conditions

- **It never blocks.** `log/2` runs in the process that logs. It checks a counter, formats with a
  bound, and sends to the relay; nothing in it waits. Only the relay calls
  `io:put_chars(group, ...)`.
- **The caller is group or the driver.** The event is never written through group: the relay
  sends the driver `{:redoubt_shell_log, fixed line}`, and the driver draws it. Tested from inside
  group's own process (`:sys.replace_state` on group, called by the line, which waits on group
  meanwhile). The driver-as-caller branch is the same code with no test of its own: no test path
  makes the driver log.
- **The flood bound.** A `:counters` pair holds the events in the relay (at most 32) and the
  dropped count. The relay writes `[N log events dropped]` before the next event it shows. group's
  mailbox stays at most one request from the relay. The test samples group's
  `message_queue_len` while a process logs 2,000 errors; it asserts at most 4, and that the
  dropped line appears.
- **Restoring.** When the driver ends, `uninstall` removes the handler, kills the relay, and
  re-adds `default` with its saved config. Tested: the handler ids after equal those before.
- **Formatting.** The `default` handler's formatter: OTP's `logger_formatter`, with
  `chars_limit` and `max_size` of 4096 unless set, or Elixir's when the default uses it. The output
  is walked piece by piece to UTF-8, a byte that is not UTF-8 becoming `<FF>`, because
  `io:put_chars` refuses invalid UTF-8. beamlet's `unicode:characters_to_binary` also dropped what
  followed the first error in a list, so the walk does not rely on its rest. A formatter that
  raises gives the fixed line.
- **Uncovered, named on shell.md.**
  - Code the person runs that writes to `user` or `standard_error` itself.
  - beamlet's stand-in logger, loaded only when OTP's logger is absent from the code path (the
    host CLI with no system path); Redoubt's volume holds OTP's.

## Tests and gates (head a5a8e066c)

- **./test-shell, full.** Every stage but `on_fake_kernel` passed: beamlet 153/153, BEAM ok,
  formatting, native, entry_point and terminal ok. The `on_fake_kernel` failure is the Rust test
  `an_end_of_input_already_waiting_ends_the_idle_that_takes_it` in userland/otp/redoubt
  (console.rs:226), a host-clock test this branch does not touch. Rerun alone with
  `q run --quiet -- cargo test -p beamlet-redoubt --features fake --test console`: 3/3 pass (13
  tests each). An earlier `./test-shell test/redoubt/shell/driver_test.exs` passed every stage,
  `on_fake_kernel` included, with driver_test at 19/19 on beamlet and on BEAM.
- **The hostile test.** "a crash report of a process a line spawned reaches the terminal as
  visible text":
  - the emulator's report of a raise whose message holds OSC 52, OSC 0, U+202E and CSI 2J;
  - proc_lib's Task report with CSI 6n and a raw 0x9B byte;
  - `:erlang.error` with a non-UTF-8 binary;
  - a logger event whose message is raw controls and invalid bytes, drawn
    `raw ^[]52;c;aGk=^G<U+202E><9B><<FF>>`.

  It is judged by the terminal model, which raises on any sequence the encoder would not write.
  On beamlet the reports' text arrives raw and is drawn visibly; on BEAM, Elixir's formatter
  escapes it first.
- **The other gates.** docs 0, formatting 0, prebuilt 0. rv64 and rv32 userland-boot and
  userland-read-only all exit 0.

## Docs checked

- **Changed:** shell.md.
- **Checked, no change needed:**
  - docs/plan/m2-usable-shell.md's Progress says "hostile text drawn visibly on every path the
    host has", which is now true;
  - SECURITY.md's session residual, "the terminal guard stops hostile text, not the session's own
    code", still holds;
  - README.md and GETTING-STARTED.md hold no claim about the logger.

## Next

Rebase after SHELL4, which touches the driver: the install and uninstall wrap `loop/1` in
`run/1`, and the loop gains one clause.
