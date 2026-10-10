# SHELL8 checkpoint: the logger's output through the shell's guard

## What happens now

On Redoubt beamlet runs OTP's real logger: in vm.rs `boot`, `beamlet_kernel` adds the kernel's
handlers. The `default` handler is `logger_std_h` at `standard_io`, which writes to `user`.
`user` is `beamlet_io`, which writes straight to the console, past the driver and `Redoubt.Term`.

Every logger event takes that path:
- proc_lib crash reports;
- the emulator's "Error in process" (vm.rs:1309);
- `error_logger` calls, which are forwarded to logger;
- any `logger:log` or Elixir `Logger` call.

## Proposal: a logger handler, not just a formatter

A formatter alone would still write through `user`, out of step with the line being edited.

- **The handler.** `Redoubt.Shell.Log` is a handler module (`log/2`, `adding_handler/1`). It
  formats with OTP's `logger_formatter`, using the default handler's own formatter config. It
  writes the text to the driver's `group` with `io:put_chars`. A report then reaches `Term` as
  group output, made visible grapheme by grapheme like every other group request, and `group`
  redraws the line being edited around it.
- **Where it is installed.** `Driver.run`, right after `:group.start`. It removes the `default`
  handler, then adds this one with the group pid in its config. The driver owns the console, so the
  handler exists exactly when the guard does, and the tests' driver (`input: :messages`) gets it
  too.
- **What it covers.** All logger output in the VM: crash reports, `error_logger` and the
  emulator's reports. A formatter crash or a dead group falls back to one fixed line, never the raw
  report.
- **What it does not cover, which the page will say.**
  - Code the person runs writing to `:standard_error` or `:user` itself. That is the session's own
    authority, as now.
  - VMs without the kernel's logger, where vm.rs loads its stand-in, which prints to
    `standard_error`. Redoubt's volume ships the real logger, so this is the host CLI only; I'd
    leave it and name it.
- **The test.** A driver_test with crash reasons that hold escape sequences, OSC 52, bidi controls
  and invalid UTF-8, raised in a process spawned at the prompt. It is judged by the existing
  terminal model, which accepts only the encoder's own sequences.
- **Docs.** In shell.md, "Hostile text never drives the terminal" and "The loop" lose the
  unguarded-path sentences.

## Base and gates

- Base: main 80400a1d4, in a new worktree .worktrees/SHELL8 on wp-SHELL8. I'll rebase after
  SHELL4.
- Gates as assigned.
