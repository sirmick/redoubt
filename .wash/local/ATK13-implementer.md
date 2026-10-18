# ATK13: the last two scripted rows

Node ATK13 (parent `agent-suite`). Tier A (attack cases; the bench). Size S. Reviewer: a red,
the orchestrator's pick. Ruling: `.wash/local/wave-2026-10-18-rulings.md` "agent-suite wave B".
Pages: docs/plan/m3-agents.md (the two rows), docs/servers/ipd.md "R60 (a sink refuses labels)",
docs/servers/keyd.md "R44 (one key, one purpose, keyd's own digest)", docs/userland/agents.md
"Leases" (a lease has no network). Patterns: `tests/steward-approve-box.toml` (a labelled agent
approved and run in its vault), `tests/session-tcp-loopback.toml` (a lease has no `/net`),
`tests/net-attacks.toml` (ipd refuses a labelled caller).

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting`, docs gate `set CASES=docs`. No edit while prebuilt
  builds or cases run.
- Messages at most 2000 bytes; detail in a file. Read pages by section.
- A verdict comes from a party the attacker cannot impersonate (rule F): a server's line, the
  agent's result file in its vault, the bench peer's record, the kernel's; never the agent's own
  stdout. A row that cannot be a boot case today is resolved honestly, not faked.
- You touch no server, kernel or steward code. m3-agents.md's table is shared with ATK10–12:
  edit only your two rows.

## The two rows

1. **R60 — "A labelled agent reaches a sink not cleared for its labels"** (today: `net-attacks`
   for ipd; the agent's case not yet). Build `steward-labelled-agent-no-sink` on
   `steward-approve-box`'s path: alice's agent submits a request for an agent labelled
   `alice-secrets`, alice approves it at `approve@box`, and the labelled agent runs in its vault
   (its `/project` is the `alice-secrets` volume, as that case already shows). The new attack
   content: the labelled agent attempts `:gen_tcp.connect` to the bench peer and to a loopback
   address, and gets `{error, :enetunreach}` — its lease namespace holds no `/net`
   (`session-tcp-loopback` shows this for an unlabelled lease; here the agent is labelled too, so
   the namespace wall and R60 both apply). Verdicts (rule F): the agent writes its result to its
   vault (`alice-secrets` volume), which alice's vault session reads; the bench peer records no
   frame from it; and the R60 backstop — a labelled caller at ipd refused before admission — is
   cited to `net-attacks`, not re-tested here. If reaching the peer needs a `/net` the agent does
   not have, that *is* the result: state that the agent cannot even attempt the sink, the
   namespace being the first wall and R60 the second.
2. **R44 — "Have keyd sign arbitrary bytes (an SSH user-auth blob relayed from its peer)"**
   (today: not yet in a boot). There is no boot arm reachable in the image: no session or lease
   holds a keyd badge (keyd's purposes are sshd's `ssh_host` and the steward's `audit`; keys in
   leases are M6), so no attacker in the image can ask keyd to sign anything. Resolve the row on
   m3-agents.md: cite keyd's host tests (`a_relayed_ssh_user_auth_blob_is_never_what_gets_signed`,
   `a_badge_signs_only_its_own_key_and_only_its_own_purpose`, `parts_cannot_be_slid_into_each_other`,
   `the_hash_matches_an_independent_implementation`) and state in the row why no boot arm exists
   (no keyd badge reaches an attacker until M6's keys-in-leases). The boot *evidence* that keyd
   signs only its own digest is a normal SSH login's end-to-end verification: the client checks
   the host signature over the exchange hash it computed itself, which every login case already
   exercises (`launch-system`, `steward-ssh-two-principals`) — cite one, do not add a case.

## What the reviewer checks

- (1)'s verdict is the agent's vault result and the peer's silence, never the agent's stdout; the
  labelled agent genuinely runs (the approval path from `steward-approve-box`), and its lack of a
  sink is shown, not assumed.
- (2) adds no manufactured boot case: the row cites the host tests and a login's end-to-end
  check, and states the reason no attacker-driven boot arm exists. A reviewer who wants a boot
  arm is answered: there is no keyd badge to give an attacker in the image.

## Pages

- m3-agents.md: row 1 gains `steward-labelled-agent-no-sink`; row 2 gains the host-test citation
  and the one-clause reason (replacing both "not yet"s).
- ipd.md R60 and keyd.md R44 status lines gain the bench case / the citation where they prove the
  built rule; SECURITY.md R60 row gains the case. agents.md "Leases" if the no-network clause
  wants the case. Summaries checked: say which.

## Owned paths

`tests/steward-labelled-agent-no-sink.toml` and its staged data under `tests/data/steward/stage`;
a scripted labelled-agent module under `userland/shell/lib/redoubt/agents/` if
`steward-approve-box`'s agent does not already attempt a connect; the pages above. Not `servers/`,
not `kernel/`, not `libs/`, not `tests/steward-latency.toml`.

## Gates

The new case both widths; `set CASES=formatting`, `set CASES=docs`; `steward-approve-box`,
`session-tcp-loopback`, `net-attacks` unchanged in verdict; `size-budget`, `unsafe-budget`.
Report each with its exit code and the verdict line for (1).

## Checkpoint

None required (size S); if (1)'s labelled agent cannot attempt a connect at all (no `/net` to
call `gen_tcp` against), say so in the report as the row's resolution rather than forcing a
connect, and the row still closes.
