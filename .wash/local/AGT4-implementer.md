# AGT4: the escape room's call-surface fixture (`ecall`)

Tier A (a `/boot` program that makes raw kernel calls from a `user` budget; it is in the image
and on the attack path). Size S. Part of step AGENT0: the escape room's scenarios that aim at the
kernel need the agent to reach the kernel's call surface, and from Elixir it reaches the kernel
only through beamlet's natives, which were written to be safe. This package ships one program that
exposes the raw surface, under the launcher's own authority and nothing more.

The whole of the new behaviour already exists as a scripted attack: `tests/programs/src/bin/
syscall-attack.rs` makes `libs/sys` calls directly and the kernel refuses the bad ones without a
panic. `ecall` is that program with its records read from stdin instead of compiled in, so the
game's referee (or a session, or a launched agent) can decide the records at run time. It grants
nothing a scripted program does not already have: a launch takes a subset of the launcher's
handles and budget (docs/userland/native.md#launching-from-a-session), so a program a session
starts can already make any call the session could, and the kernel checks every one against the
caller's own authority. This is the threat model's "the adversary runs arbitrary code in its own
processes", made a fixture.

Run everything natively on this host through `q run`, under the job pool's rules (docs/testbench.md
"On a shared host"). Scratch under `$REDOUBT_TMP`. Do not edit the worktree while a prebuilt case
runs.

## Context rules (read these first)

- **Don't read whole files.** `libs/sys` by symbol, except `call.rs`'s `Call`/`Number` enum and
  `record.rs`'s record types (read those whole, once). `tests/programs/src/bin/syscall-attack.rs`
  whole (it is short). native.md and agents.md by section.
- **Don't open `.wash/qa/*.md` or other packages' reports.**
- **Pipe bench output;** boot logs through `grep` or `tail`, never whole, never with hex.
- **Read a file right before you Write it,** and prefer Edit.
- **Report under 1900 bytes,** detail in `.wash/local/AGT4-report.md`.
- **A handoff ends with "what consumed my context".**

## Reading list (only these)

- `tests/programs/src/bin/syscall-attack.rs` and `tests/programs/Cargo.toml`: the existing
  program, how it connects to log-server and the checker, and how it is built.
- `tests/syscall-attack.toml`: how the case launches it and takes its verdict (survival, from
  log-server's `DONE` and the checker's power-off under its own PID: rule F).
- `libs/sys/src/call.rs`: `Call`, `Number`, `Handle`, `encode`/`decode`; `libs/sys/src/record.rs`:
  `BodyOf`, `Received`, the budget and usage records; `libs/sys/src/ecall.rs`: `syscall`.
- `docs/userland/native.md`: "Launching from a session", "Standard input and output, and pipes"
  (a stage reaches only `/dev/stdin`, `/dev/stdout`, `/dev/stderr`).
- `docs/userland/agents.md`: "The escape room" (where the fixture is named).
- `docs/plan/m3-agents.md`: "Attack suite" (the scripted-hostile-agent and -user tables, which the
  fixture lets a model drive), "Progress".

## What to build

A `/boot` program, `ecall`, that in a loop:

1. reads one request record from `/dev/stdin` in a small, fixed, self-describing binary framing
   (a length, a call number, the argument words, and for a lend/transfer the page range to take
   from its own mapped memory). Define the framing in the program's own module and document it in
   a comment and on the page; keep it bounded (a max record size, a refusal past it) so the reader
   cannot be driven to allocate without bound.
2. decodes it to a `libs/sys` `Call`. A number `Number::from_raw` does not know, or a body
   `decode` refuses, is answered with an error frame, not a crash.
3. makes the call with `sys::syscall` (or the `rt` wrapper the other programs use), and writes a
   result frame to `/dev/stdout`: the raw return words, or the kernel's error by its name
   (docs/servers/wire.md#error-names, as the client library keeps it).
4. ends cleanly on end-of-input (its stdin writer gone).

Constraints:
- **No authority of its own.** It opens nothing, binds nothing, and holds only what its launcher
  gave it on the launch call. It never names a handle it was not given; a request naming a handle
  number it does not hold gets the kernel's `BAD_HANDLE` (or the decode's refusal), which is the
  point.
- **The reader is hostile-input-hardened** like any parser of untrusted bytes (tenet 5): bounded,
  no panic on any byte sequence. It is the one parser this program adds; treat its robustness as
  the deliverable, not the call-making, which `libs/sys` already does.
- **No new kernel or `libs/sys` code.** If a call cannot be expressed through `libs/sys` as it
  stands, stop and report it rather than widening the ABI: that is a finding about the surface,
  which the escape room wants recorded, not patched here.

## Tests

- **A boot case,** `tests/ecall.toml`, that launches `ecall` from a session (reuse the launch path
  the `pipe`/`beamlet-launch` cases use), feeds it a scripted sequence over its stdin, and takes
  the verdict from survival as `syscall-attack` does (log-server `DONE`, the checker's clean
  power-off under its own PID; rule F), on both widths. The sequence covers: a well-formed benign
  call returning the kernel's result; a malformed frame (a length past the buffer, an unknown call
  number, a truncated body) each answered with an error frame and no panic; an out-of-authority
  call (a handle it was not given, a lend over `MAX_LEND_PAGES`) answered with the kernel's error.
  The claim is exactly I14 (no call panics the kernel) driven through a launched program instead of
  a compiled one.
- Keep `tests/syscall-attack.toml` as it is: it is the compiled twin and a regression anchor. Do
  not fold it into `ecall`.
- The Tier A short gate.

## Pages

- `docs/userland/agents.md` "The escape room": one paragraph naming `ecall` as the fixture that
  gives a scenario the kernel's call surface, with the note that it adds no authority (a launch is
  a subset of the launcher's), and its case.
- `docs/userland/native.md`: if a `/boot` program list or the launch section should name it, one
  line; otherwise none.
- `docs/plan/m3-agents.md` "Progress": one clause, that the call-surface fixture exists so the
  attack-suite scenarios can be driven by a model under the referee, with the kernel's own cases
  (`syscall-attack`, `budget-syscall-attack`, `budget-forge-attack`) as their scripted twins.

## Owned paths

The new program's crate (add it beside `tests/programs` or wherever the `/boot` attack programs
live; follow `syscall-attack`'s crate), `image/manifest.json` and `image/README.md` (to make
`ecall` public in `/boot`), `tests/ecall.toml`, the pages above. Not `libs/sys`, not the kernel:
if either needs to change, stop and ask through the orchestrator.
