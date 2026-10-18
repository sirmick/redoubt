# ATK11: the scripted hostile user at the shared servers

Node ATK11 (parent `agent-suite`). Tier A (attack cases; the bench). Size M. Reviewer: a red,
the orchestrator's pick. Ruling: `.wash/local/wave-2026-10-18-rulings.md` "The agent-suite
step". Pages: docs/plan/m3-agents.md "A scripted hostile user" (the fairness rows you fill),
docs/kernel/ipc.md "R2 (fair waiting)", docs/servers/serving.md "R26 (admission fairness)",
docs/servers/steward.md "R37 (vault non-interference)", docs/servers/ipd.md "Pinned and
abandoned calls", docs/testbench.md "Rule F". Patterns: `tests/steward-sub-budget-flood.toml`,
`tests/net-pinned.toml`, `tests/steward-home-quota.toml`, `tests/steward-ssh-two-principals.toml`.

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting`, docs gate `set CASES=docs`. No edit while prebuilt
  builds or cases run. Copy a failing job log aside before a rerun.
- Messages at most 2000 bytes; detail in a file. Read pages by section.
- A verdict comes from the victim (rule F): the operation alice's own session completes while
  the flood runs, in her shell's printed value or a server's answer to her; never the attacker's
  output. The attacker shows only that it flooded (its process count, its request rate).
- A bound a case asserts (how long alice's operation may take under the flood) is measured once
  on both widths, stated with its arithmetic in the toml's comment, and is the fairness claim,
  not a tuning: a miss is a finding. A case orders on events, not the host's clock; under
  `icount` a deadline passes deterministically.
- You touch no server or kernel code. A finding in a server goes to the Architect on a QA
  thread. The shared page is m3-agents.md's table: edit only your rows.

## The cases

Each is a boot with two or more sessions; the attacker floods, the victim's operations are timed
or counted by the bench against the stated bound.

1. **`walfsd-flood-turn`** (endpoint flooding: Alice is still served in her turn). Bob's session
   floods `walfsd:data` from as many processes as his VM's hub admits (the hub's `MAX_WAITERS`;
   `steward-sub-budget-flood` shows how to spawn them), each parking an operation; alice's
   session reads and writes her home throughout. Verdict: each of alice's operations returns
   within the bound, her shell printing the values; R2 serves the least-recently-served group and
   R26 gives her badge its share.
2. **`vault-flood-turn`** (R37: a vault filling its `WAIT_CAP` leaves the owner's unlabelled turn
   and cap unaffected). Alice's vault session floods `walfsd:data` (a vault may read the unlabelled
   volume) while alice's unlabelled session does the operations of (1). Verdict as (1), and the
   unlabelled session's cap and turn are what they were without the vault (compare the bound to
   (1)'s): the vault's queued calls change neither.
3. **`blkd-fairness`** (system fairness: a busy `walfsd:data` does not fill `blkd`'s `WAIT_CAP`
   for `walfsd:alice-secrets`). Bob writes large files through `walfsd:data` without end while
   alice's vault session writes and reads back in `walfsd:alice-secrets`; both file servers share
   one `blkd`. Verdict: alice's vault writes and reads complete within the bound (her vault
   session's printed values), so `walfsd:data`'s load at `blkd` left `walfsd:alice-secrets` its
   turn (R2 at `blkd`).
4. **`net-pinned-ssh`** (pinned open calls with SSH surviving). Alice's shell parks 64 `gen_tcp`
   reads at `ipd` that her own short timeouts abandon (the `net-pinned` shape, now from a session's
   `gen_tcp` over `/net`, which BEAM5 built), each of which `ipd` must answer and free; bob logs
   in over SSH during it; then alice's echo to the bench peer still works. Verdicts: bob's login
   reaches its prompt (its session line), alice's echo returns the bytes, and no read is refused
   at once for a full share (R28, R4a).

## What each verdict rests on (the reviewer checks)

- The timed operation is the victim's and its result is read from the victim's session or a
  server's answer to it, never the attacker's.
- The flood is real: the case shows the attacker's process count or parked-call count reaching
  the server's cap, so the fairness claim is tested, not merely asserted.
- The bound each case asserts is measured on both widths and stated with its arithmetic; the
  case fails if the victim's operation exceeds it (the finding), never retried into passing.

## Pages

- m3-agents.md "A scripted hostile user": the four rows gain their case ids (replacing "not
  yet" / "with walfsd not yet" / "with SSH not yet").
- ipc.md R2, serving.md R26/R28, steward.md R37, ipd.md status lines gain the cases where they
  prove a built rule. SECURITY.md rows for R2, R26, R28, R37, R4a grow. Summaries: say which.

## Owned paths

`tests/walfsd-flood-turn.toml`, `vault-flood-turn.toml`, `blkd-fairness.toml`,
`net-pinned-ssh.toml` and their manifests/staged data; the pages above. Not `servers/`, not
`kernel/`, not `libs/`, not `tests/steward-latency.toml`.

## Gates

Each case both widths; `set CASES=formatting`, `set CASES=docs`; `net-pinned`,
`steward-sub-budget-flood`, `steward-ssh-two-principals` unchanged in verdict; `size-budget`,
`unsafe-budget`. Report each with its exit code, the measured bound per case with the log path,
and the verdict line.

## Checkpoint

After the first case passes both widths, one progress note: the bound you measured and the
verdict line.
