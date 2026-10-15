# AGT3: compiling on the box, proven in a boot

Tier B (the session's own VM, files in its own home; no capability changes hands). Size S.
Needs AGT1 (the case logs in as `agent`). Step AGENT0: this is the package that lets the agent
build itself a home: write Elixir, compile it, keep the `.beam` in its home, load it again later.

Run everything natively on this host through `q run`, under the job pool's rules
(docs/testbench.md "On a shared host"). Scratch under `$REDOUBT_TMP`. Do not edit the worktree
while a prebuilt case runs.

## Context rules (read these first)

- **Don't read whole files.** development.md and beamlet.md by section; the compiler test whole
  (it is 30 lines).
- **Don't open `.wash/qa/*.md` or other packages' reports.**
- **Pipe bench output;** boot logs through `grep` or `tail`.
- **Read a file right before you Write it,** and prefer Edit.
- **Report under 1900 bytes,** detail in `.wash/local/AGT3-report.md`.
- **A handoff ends with "what consumed my context".**

## Reading list (only these)

- `docs/userland/development.md`: "The compilers on beamlet" (its status line is what you
  change) and "Compiling on the box" (planned, M5: you do not build it; read it so the new case
  claims less than it).
- `docs/userland/beamlet.md`: "beamlet on Redoubt", the `load_module` row (what the code path
  looks at: the boot pack and the verified system volume, **never the session's writable
  namespace**, by BL1's rule on docs/userland/packages.md#profiles-and-upgrades). This decides
  the shape of the case: a module is reloaded from home by reading the bytes and
  `:code.load_binary/3`, not by `Code.append_path/1`, which must stay without effect.
- `userland/otp/tests/elixir/compiler_test.ex` whole.
- `tests/shell-commands.toml`, `tests/agent-login.toml` (AGT1): how a case drives a session.

## What to build

No code on the box is expected to change. The package is a case and the pages it moves, unless
the case finds the compiler does not run in a session on the machine, in which case the fix is
the package and you say so in your first progress note.

The case, `tests/shell-compile.toml`, on both widths, with the image's recipe, a session as
`agent` over SSH (AGT2's machine mode if it has landed, else tagged lines as `shell-commands`):

1. `Code.compile_string/1` of a small module (take `compiler_test.ex`'s), bound to `{mod, bin}`;
   a call on it is the expected value.
2. `File.mkdir_p("/home/agent/lib")` and `File.write("/home/agent/lib/Elixir.<Mod>.beam", bin)`;
   the file server's `:ok`.
3. `:code.purge(mod); :code.delete(mod)`; a call now raises `UndefinedFunctionError` (tag the
   rescue's result).
4. `Code.append_path("/home/agent/lib")` followed by the same call still raises: the code path
   does not reach home (the point of BL1; the case states it).
5. `File.read!` the `.beam` back and `:code.load_binary(mod, ~c"/home/agent/lib/...", bytes)`;
   the call gives the expected value again.
6. `Code.eval_string("a + b", a: 1, b: 2)` is `3`, so the evaluator's path is covered too.
7. Remove what was made.

Verdicts: the shell's printed values and the file server's answers (rule F), as `shell-commands`.
Keep the module small: the session's heap limit is the evaluator's 128 MiB on a 64-bit VM and
the compiler is the heaviest thing the shell has run in a boot; if rv32 fails on memory, report
the figure before changing any limit.

## Pages

- `docs/userland/development.md` "The compilers on beamlet": the status line loses "runs on the
  host only" and names `bench:shell-compile`; the differential suites' clause stays as it is.
  One sentence in the body: on the machine, a compiled module is kept as a `.beam` in the
  session's home and loaded from its bytes; the code path does not search home
  ([beamlet](beamlet.md#beamlet-on-redoubt)).
- `docs/plan/m3-agents.md` "Progress": one clause, beside AGT1's.
- `docs/plan/m5-self-hosted.md`: only if it says the compiler is host-only; then the one word.

## Owned paths

`tests/shell-compile.toml`, the pages above, and `userland/otp` only if step "no code is expected
to change" turns out false.
