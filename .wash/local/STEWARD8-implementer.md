# STEWARD8: a lease's launch under load is a measured quantity; start_agent's bound follows it

Node STEWARD8 (parent AGENTS). Tier A (STEWARD8 adds the steward's `served start_agent`
line as its first commit: `10 => Some("start_agent")` in `served_op`, one console write per launch
through 3U's queue, stated on steward.md; the rest is the Elixir client, the shell's call, a toml,
pages). Reviewer steward-red-4. Size S. Starts when STEWARD5 and STEWARD6 are on main (both are). Ruling: `.wash/local/wave-2026-10-18-rulings.md` "F4"; evidence
`.wash/local/B88-report.md` ("The launch time"), scheduling.md "With the real steward".

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting` (it runs nightly fmt in `userland/otp` too), docs gate
  `set CASES=docs`. No edit while prebuilt builds or cases run. A `steward-latency` run at two
  harts on rv32 may pass 1.5 h: launch it detached (`setsid nohup`); copy job logs aside before
  a rerun (`target/jobs/<w>-<case>.log` is overwritten).
- Messages at most 2000 bytes; detail in a file. Read reports by section.
- Pages carry no dates or ids; a number on a page shows its arithmetic; a bound is measured once
  on both widths, never guessed. A miss is a finding, not a tuning; no target moves.
- Do not touch the steward, the kernel or the scheduler: a weight is the owner's. If the
  measurement wants a steward change, stop and ask on a QA thread.

## The symptom

On rv32 at two harts a lease VM's launch beside two spinner VMs takes 5.3–5.7 s (3.9 s alone),
and the shell's `start_agent` answers `{:error, :timeout}`: `@timeout 5000` in
`libs/wire/elixir/lib/client.ex`, the Elixir client's bound for every steward call, a platform
constant no page states as a target; no page bounds a launch at all. The round's loop dies on
the match and the rv32 two-hart rows at N = 3 and 4 are "not reached" with the cause named
(compute: the launching VM's share beside two equal-weight spinners).

Also (STEWARD6's measurement run, thread STEWARD6-bound): on rv32 at one hart the N = 4 round
beside three spinners died on the same 5 s bound (B88 had it borderline at 5.1 s), so the
one-hart rows are in scope too: measure the launch at N = 4 on both widths at one hart and
derive the bound over every round the case runs. STEWARD6's report names the log.

## The rule

1. **The launch is stated and measured**, as `end_lease`'s 589 ms is on steward.md "Residual
   risks" ("Steward work is paid by the steward"): `start_agent`'s `served` time (the steward's
   `steward: served start_agent in N us` line) alone and beside N − 1 spinners, from the latency
   case's rounds, both widths, one and two harts, with the launch as B88 measured it (the gap
   between consecutive `AgentStarted` lines less the session's sleep) beside it if the two
   differ by more than the sleep's rounding.
2. **The shell's `start_agent` passes the client a bound of its own**: twice the worst launch
   measured beside the spinners the case runs, rounded up to a second (`Redoubt.Wire.Client`'s
   `call/5` takes a timeout; `Redoubt.Wire.Client.Steward.start_agent` gains the argument or a
   module constant with the arithmetic in its comment). The 5 s default stays for every short
   call. The constant lives in the shell's client, not the steward.
3. **Then the rv32 two-hart rounds run.** With the bound derived, `steward-latency` at two harts
   on rv32 reaches N = 3 and 4; measure the rows, fill the table's column. A target missed there
   is a scheduler finding, written beside the table with its cause and the owner's weight
   question named without an id; nothing moves.

Drafted under the image's weights (session 100, agent 50, steward 100; the case's steward
1,000): the owner's answers change the number, not the design. If the owner moves a weight
before you measure, measure under the new one and say so.

## Cases

- Bench: `steward-latency` (seed 3, by name, `--keep-failed`) both widths, one and two harts:
  every round reached, the rv32 two-hart cells filled; `steward-lease-ends`, `steward-end-lease`,
  `agent-login`, `shell-compile`, both widths, unchanged in verdict.
- Host (`userland/shell` or `libs/wire/elixir` tests, whichever holds the client's tests): the
  call passes its own bound; a reply after the bound is `{:error, :timeout}` as today.

## Pages

- steward.md "Residual risks": the launch's measured cost beside `end_lease`'s, the bound's
  arithmetic.
- scheduling.md "With the real steward": the rv32 two-hart column measured; the paragraph on
  the bound (the client's 5 s) rewritten to the derived bound; "not reached" removed where a
  round now runs; the N = 4 sentence.
- agents.md "Leases" one clause (how long a `start_agent` may take under load, by the page).
- `tests/steward-latency.toml`'s header comment. Summaries checked: say which.

## Owned paths

`libs/wire/elixir/lib/client.ex` and the generated steward client's `start_agent` (or the shell
module that calls it: `userland/shell/lib/redoubt/...`; read where the call is made and change
the one place), their tests; `tests/steward-latency.toml` (comment only unless a round's shape
must change: then say why); the pages above. Not `servers/steward`, not `kernel`, not
`image/manifest.json`.

## Gates

`set CASES=formatting` (nightly fmt in `userland/otp` included); `set CASES=docs`; the bench
cases above both widths; the shell's host tests (`beamlet-shell-host-tests` or the case the
bench names for them). Report each with its exit code, the measured launches with log paths,
and the arithmetic that set the bound.

## Checkpoint

After the one-hart measurements on both widths: one progress note with the worst launch and
the bound you derive, before the two-hart runs.
