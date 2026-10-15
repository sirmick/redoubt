# AGT1: the `agent` principal in the image

Tier A (the manifest and the steward's principals: who may log in and what budget they get).
Size S. Needs nothing. Step AGENT0 (owner, 2026-10-15): a sponsor-run agent whose model loop
runs on the owner's host and whose body is an ordinary principal's session in the box. This
package is the principal; AGT2 is its eval channel, AGT3 proves it can compile its own code.

Run everything natively on this host through `q run`, under the job pool's rules
(docs/testbench.md "On a shared host"). Scratch goes under `$REDOUBT_TMP`, never `/tmp`. Do not
edit the worktree while a prebuilt case runs.

## Context rules (read these first)

- **Don't read whole files.** Pages by section; `image/manifest.json` whole (it is short);
  `servers/steward` and `servers/init` by symbol.
- **Don't open `.wash/qa/*.md` or other packages' reports.**
- **Pipe bench output;** read boot logs through `grep` or `tail`, never whole, never with hex.
- **Read a file right before you Write it,** and prefer Edit.
- **Report under 1900 bytes,** detail in `.wash/local/AGT1-report.md`.
- **A handoff ends with "what consumed my context".**

## Reading list (only these)

- `image/README.md` whole: the principals, how the budgets add up under `users`, the staged homes.
- `docs/servers/init.md`: "The manifest" (the `principals` and `steward` entries and the checks
  `init` makes: every size within every principal's smallest share).
- `docs/servers/steward.md`: "Principals", "Fixed sub-budgets per label set".
- `docs/userland/sessions.md`: the opening example and "What a session is told".
- `GETTING-STARTED.md`: "Try the system" (`launch --system`, `--key`).
- `tests/launch-system.toml`, `tests/steward-ssh-two-principals.toml`, `tests/shell-commands.toml`:
  how a case logs a principal in and what it expects; `tests/image-disk.toml` for the staged homes.

## What to build

1. **A third principal, `agent`,** in `image/manifest.json`: account `1003`, no labels, no
   `label_sets`, **no `net`** (the agent never gets a network scope: docs/userland/agents.md
   "Delegation only narrows"), home `data:/home/agent` with the same quota as bob's, and a budget
   that holds one session with room for a second context: start from bob's shape and re-balance
   alice's and bob's so the three still fit under `users` as image/README.md describes. Record
   the arithmetic in image/README.md where the budgets are explained.
2. **Its key.** The image's development key is alice's. Give `agent` its own development key the
   same way alice gets hers (made once under `.tmp/launch` by `launch`, and the checkout's test
   key for the bench), and add `--agent-key PUB` to `launch --system` beside `--key`. `launch`
   prints a second ssh line, `agent@`, after alice's.
3. **The staged home.** `./mkimage` makes `home/agent` as it makes `home/alice` and `home/bob`.
4. **No sponsor mechanism.** M1 has no sponsors or leases; `agent` is "alice's" only in the
   image's README and in name. Do not add a field, a steward rule or a page section for it:
   STEWARD3 brings the real thing, and this principal is what it will convert.

## Tests

- **A new boot case,** `tests/agent-login.toml`, on both widths, with the image's recipe (as
  `shell-commands` uses): a session as `agent` over SSH reaches its prompt in `/home/agent`;
  `{:pwd, pwd()}` says so; `{:other, File.read("/home/alice/x")}` is `{:error, :enoent}` (no
  binding holds alice's home); `{:net, ns()}` lists no `/net`; `{:who, whoami()}` is `"agent"`;
  `{:lab, labels()}` is `{}`. Tag every answer as `shell-commands` does, so a pattern never
  matches the echo. Verdicts: the shell's printed values and the steward's `login agent:
  labels {}` line in the server log (rule F).
- **Existing cases** that count or name principals (`launch-system`, `steward-ssh-two-principals`,
  `image-disk`, any `forbid` on `login .*: refused`) are run and, where their expectations
  enumerate principals or budgets, extended, never loosened.
- The Tier A short gate.

## Pages

- `image/README.md`: the principal and the budget sum.
- `docs/userland/sessions.md`: where the image's principals are named, `agent` joins them in one
  sentence, with its purpose (a principal for a sponsor-run agent; no network; no labels).
- `GETTING-STARTED.md`: the `agent@` ssh line and `--agent-key`.
- `docs/plan/m3-agents.md` "Progress": one sentence, that the agent principal exists in the
  image ahead of leases.
  Nothing on `docs/userland/agents.md`: its harness and loop sections stay planned for M5.

## Owned paths

`image/manifest.json`, `image/README.md`, `image/disk.toml` (if the stage needs it), `mkimage`,
`launch`, `tests/agent-login.toml`, the pages above. Not the steward's or `init`'s code: if the
manifest cannot say what is needed without a code change, stop and ask through the orchestrator.
