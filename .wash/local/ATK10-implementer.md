# ATK10: the scripted hostile agent

Node ATK10 (parent `agent-suite`). Tier A (attack cases; the bench). Size M. Reviewer: a red,
the orchestrator's pick. Ruling: `.wash/local/wave-2026-10-18-rulings.md` "The agent-suite
step". Pages: docs/plan/m3-agents.md "A scripted hostile agent" (the table you fill),
docs/userland/agents.md "Leases", docs/servers/steward.md "Leases", "The powerbox and approvals"
(the free-text rule), docs/testbench.md "Rule F". Patterns: `tests/steward-lease-ends.toml`,
`tests/steward-sub-agent-dies-with-agent.toml` (an agent driven by `~/project/in`),
`tests/steward-screen-inert.toml` (the approve@box screen as the bench reads it),
`userland/shell/lib/redoubt/agents/echo.ex`.

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting` (nightly fmt runs in `userland/otp` too), docs gate `set
  CASES=docs`. No edit while prebuilt builds or cases run. Copy a failing job log aside before a
  rerun.
- Messages at most 2000 bytes; detail in a file. Read pages by section.
- Pages carry no dates or ids; a status line names its tests; a built section has no `**Open:**`.
  A property's verdict comes from a party the attacker cannot impersonate (rule F): the kernel's
  or a server's line, the steward's audit, or the victim's own session; never the scripted
  agent's own output. An agent's report file may be read, never trusted for the verdict.
- A flaky test is a bug; a case orders on events, not the host's clock; a bound a case states is
  measured once on both widths with its arithmetic in the toml's comment.
- You touch no steward, kernel or server code. STEWARD5 and STEWARD7 own the steward's binary
  and the latency manifest; a finding there goes to the Architect on a QA thread, not into your
  branch. The one shared page is m3-agents.md's table: edit only your rows.

## The cases

The scripted agents are Elixir modules beside `Redoubt.Agents.Echo`, each reading its orders from
`~/project/in` as Echo does and launched by `start_agent` from alice's SSH session (the pattern
in `steward-lease-ends.toml`). On the volume they are session-class code; the cases are Tier A.
Each row below is one of m3-agents.md's "not yet" rows; the result it must show is in parentheses.

1. **`steward-agent-escape`** (read outside its scope or reach the network; use its sponsor's
   connection; a lease past `MAX_LEASE`). Alice stages canaries (`~/notes.txt`,
   `~/project/out/canary`) and starts the agent; the agent attempts, and each is refused in
   place: read/write/list of `/home/alice/notes.txt`, `/home/bob`, `/boot/x` (write),
   `/dev/cons`, `/net/tcp/clone`; `bind("/x", h)` of handles `ns()` shows and lookups of paths
   it does not hold; a `start_agent` with a term over `MAX_LEASE`. Verdicts: the canaries' bytes
   and alice's listings unchanged after the lease ends (read by alice's session), the servers'
   refusals in their own lines, `start_agent` answering `failed`/refused, and no grant for the
   agent in the steward's audit beyond its own lease (R25, R41, R39).
2. **`steward-lease-cut`** (outlive a lease; the lender destroyed while walfsd holds its lent
   pages). The agent writes to `/project` in a loop; alice's session ends the lease with
   `end_lease` mid-write, so walfsd holds the agent's lent pages when the budget dies. Verdicts:
   walfsd does not fault (no `init:` fault line), alice reads `/project` back whole, the lease's
   connection is gone (the steward's cleanup line), alice's own home connection still answers
   (R3, R10, R39, R41).
3. **`steward-agent-flood`** (flood the steward and walfsd until the sponsor cannot open a file
   or end the lease). The agent floods `walfsd:data` through `/project` and the steward through
   its badge from many processes; alice opens a file in her home and calls `end_lease`. Verdicts:
   alice's open succeeds and the lease ends, each within the bound the case states (measured once
   both widths, arithmetic in the comment), because ending a lease is answered ahead of
   admission and a badge gets a fair share (R26). The flood is the agent's, the verdict alice's.
4. **`steward-vault-note`** (send free text from a vault to the approval screen). A vault
   session submits a request with free text; an unlabelled session submits one too. Verdicts, read
   from the approve@box screen as `steward-screen-inert` reads it: the vault's free text does not
   appear, the unlabelled's is quoted and marked untrusted (R38). If the host test
   `notes_are_rendered_by_their_requesters_labels...` already covers the rendering, this is its
   boot arm: say so and keep the case to the boot path.

## What each verdict rests on (the reviewer checks)

- No verdict reads the scripted agent's own stdout or a file it wrote as proof of a refusal;
  each is a server line, the steward's audit/usage line, the kernel's blame line, or a canary
  the agent could not have written.
- The agent holds only what `start_agent` gave it: the case shows (`ns()`, the audit) that it
  has no `/net`, no console, no sponsor connection, before it attacks.
- Every "not yet" row of m3-agents.md "A scripted hostile agent" that this node names moves to
  its case id in the table; a row left not yet says why in the report (its work is later).

## Pages

- m3-agents.md "A scripted hostile agent": the rows above gain their case ids (replacing "not
  yet" / "the agent's case not yet"); no row invented.
- agents.md "Leases" and "Delegation only narrows", steward.md "Leases" and "The powerbox and
  approvals": status lines gain the new bench cases where they prove a built rule.
- SECURITY.md rows for R25, R39, R41, R38 gain the cases (a rule's test set only grows).
  Summaries checked (README.md, GETTING-STARTED.md, docs/userland/README.md): say which.

## Owned paths

`tests/steward-agent-escape.toml`, `steward-lease-cut.toml`, `steward-agent-flood.toml`,
`steward-vault-note.toml` and their staged inputs under `tests/data/steward/stage`; the scripted
agent modules under `userland/shell/lib/redoubt/agents/`; the pages above. Not `servers/`, not
`kernel/`, not `libs/`, not `tests/steward-latency.toml` or the steward binary.

## Gates

Each case both widths; `set CASES=formatting`, `set CASES=docs`; the steward set
(`steward-lease-ends`, `steward-sub-agent-dies-with-agent`, `steward-approve-box`,
`steward-screen-inert`) unchanged in verdict; `size-budget` and `unsafe-budget` (the agent
modules add no Rust). Report each with its exit code and, for each case, the one line its verdict
reads.

## Checkpoint

After the first case passes both widths, one progress note: which rows it covers and the verdict
line for each.
