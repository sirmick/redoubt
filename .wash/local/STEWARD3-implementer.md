# STEWARD3: leases and agents, the powerbox at `approve@box`, and crash blame

Tier A (a trusted system server). Size L. Needs STEWARD2 (the server, sessions), BEAM4 (an
agent is a VM whose code calls the steward through the natives), IPC3 (per-endpoint delivery:
the lease-end path's latency is assumed) and SCHED1 (the wake targets). Don't start until the
node's needs are met.

**What it adds to STEWARD2's server:** the batches and transport for the rest of the core's
events: `StartAgent`, `Submit`, `Pending`, `Approve`, `Deny`, `EndLease`, `ApprovalOpened` /
`ApprovalClosed` (the `approve@box` channel, which `sshd` opens), `Blame` from `init`, and the
lease supervision notices. The policy is the core's (STEWARD0); the approval screens are rendered
by the core (`render.rs`); you bind effects and carry bytes.

Run everything natively on this host, under the job pool's rules (docs/testbench.md "On a shared
host"). No `in-dev`.

## Context rules (read these first)

- **Don't read whole files.** steward.md by section; `libs/steward/src` by symbol, except
  `effect.rs`, `event.rs` and `render.rs` (read whole, once).
- **Don't open `.wash/qa/*.md` or other packages' reports.** STEWARD2's report is the one
  exception: its "what the server binds" section.
- **Pipe bench output;** read boot logs through `grep` or `tail`.
- **Host-clock cases run alone:** your `ssh-loopback` and `[net]` cases.
- **Read a file right before you Write it,** and prefer Edit.
- **Reports under 1900 bytes,** detail in `.wash/local/STEWARD3-report.md`.
- **A handoff ends with "what consumed my context".**

## Reading list (only these)

- `docs/servers/steward.md`: "Leases", "The powerbox and approvals", "Crash blame", R38, R39,
  R40, R41, R42's first paragraph, "Guards and effects" (the constants), "Machines" (the lease
  and request machines; their generated diagrams).
- `docs/userland/sessions.md`: "`approve@`"; `docs/userland/agents.md` whole (short).
- `docs/servers/sshd.md`: "Sessions over SSH" (the approval channel), R67.
- `docs/servers/init.md`: "Restarts and reboots" (the `blame` call, "Only `init` can blame", "A
  wedged steward cannot stall restarts").
- `docs/servers/serving.md`: "`admit`" (a sponsor's `end_lease` answered from the receive loop,
  ahead of admission), R26, R28.
- `docs/kernel/budgets.md`: "Deadlines" and R10 (a lease's end is a destruction).
- `libs/steward/tables/{lease,request,approval_channel,blame}.md`; `libs/steward/src/render.rs`.
- `.wash/local/STEWARD2-report.md`, the section on the batch runner and badge classes.
- `tests/programs/src/bin/sched-latency.rs`: the lease-deadline and `killed`-notice roles, for
  what the real steward must do in STEWARD4.

## The design

1. **Agents.** `start_agent` from an unlabelled session's badge: the core answers `Lease { id,
   name }` with a batch: `CreateBudget` under the sponsor's sub-budget for the lease's label set
   (never the session's budget: R39's lifetime), with the lease's limits, labels and **deadline**
   (at most `MAX_LEASE`; the core refuses longer, the server never shortens), `Connect` to what
   the lease names (its `~/project` read root, its `out` write root: `fsd` connections rooted
   there; no `ipd`; the steward's endpoint on a minted badge), `Launch` of beamlet with the
   agent's module as its argument, the exit notice to the steward. A labelled agent (from a
   labelled session, in exactly its label set; or from an unlabelled session in a set its
   principal owns) goes through `submit` and an approval, as the core's rows say. Sub-agents are
   the agent's own `budget_create` inside its budget, nothing of the steward's.
   - **The lease ends** by its deadline (the kernel destroys the budget; the steward receives
     the `killed` notice as the sponsor of the exit-notice endpoint and runs `Exited`), by
     `end_lease` from any of the sponsor's unlabelled sessions (answered **from the receive
     loop, ahead of admission**: serving.md "`admit`"; the batch is one `DestroyBudget`), or by
     blame. Each path leaves `Running` with `notify_sponsor`: a notice to the sponsor's
     unlabelled sessions naming the lease, never why (the lease-supervision edge). The notice
     is a `send` on the session's steward connection, which beamlet surfaces as a message
     (BEAM4's `call/send` natives); say on the page what a session that never reads it costs:
     nothing, since the core forgets a notice a dead session cannot take.
   - **Narrowing is a revocation scope** (R41): where a server is given a way to narrow a
     session's or lease's connections, the steward passes a `CreateScope` budget (zero limits
     inside the lease's budget), never the budget itself. In this package the only user is the
     lease's own connections, created inside the scope so that ending the lease ends them.
2. **The powerbox.** `submit` from a session's or agent's badge carries the structured
   `Content` (an agent in a label set, a note; declassify and push are STEWARD4's) and the
   requester's reason. The core freezes it, draws its id and binding hash, checks the pending
   cap and the fair share, and answers `Request { id }`; its `notify` effect reaches only
   channels whose labels include all of the request's, as sends on their steward connections.
   - **`approve@box`.** `sshd` recognises the user name `approve` and authenticates with the
     principal's **approval** key (never a login key: the core's `approval_key` guard, with
     `sshd` asking `holds` first as for any key); it then opens an approval channel at the
     steward (`ApprovalOpened { principal, channel }` on `sshd`'s root badge) and serves the
     channel as a consol-protocol console whose other end is the steward, not a VM: the steward
     writes the rendered screens (`Pending`, the core's `screen` effect: printable ASCII only,
     already escaped by `render.rs`) and reads the person's lines `approve <id> <hash>` and
     `deny <id>`, which it turns into `Approve`/`Deny` events on that channel. The channel's
     close is `ApprovalClosed`. The screen's bytes cross `sshd` as any console's do, on a
     channel labelled with nothing: R67 keeps a labelled request's screen off any channel but
     its owner's, which the core's `notify` and `Pending` rows already enforce by labels.
   - **Binding:** an `Approve` whose hash is not the one the last screen showed is refused
     (`HashMismatch`), answered to the channel only, never to the requester (C9).
   - **A granted labelled agent** runs point 1's batch from the request's approval row.
3. **Crash blame.** `blame(account, labels, server)` on `init`'s root badge class only: the core
   counts per (account, label set), and at `BLAME_COUNT` within `BLAME_WINDOW` answers with a
   lockout: `DestroyBudget` for every session and lease of that domain, a refusal of new
   sessions for it until the window passes, both audited. Answer `init` within its blame
   timeout; a blame the steward cannot take in time is `init`'s console line, not a restart
   loop (init.md). The steward's own restart (it is trusted base) is `init`'s: it comes back
   with empty tables, and in M1 every session dies with it (steward.md "Failure and restart";
   say so in the residuals).
4. **Audit records** stay console lines (STEWARD2's residual) until M3.

### The rules it keeps

R38 (approvals only where the steward alone talks to the terminal; the approval key is the
person's own; rendering is the core's), R39 (every lease has a deadline; a sub-agent dies with
its agent; `end_lease` is always accepted from the sponsor), R40 (blame by (account, label set),
three in ten minutes, keyed by the label set), R41 (narrowing handles are revocation scopes),
R26 (an agent's requests count against the sponsor's admission, with a fair share per badge),
R21 (blame comes from the exit notice through `init`), R10 (a lease's end is a destruction).

## The cases (both widths; system verdicts)

The image of STEWARD2, plus an agent module in the userland disk (`Redoubt.Agents.Echo`: reads
`~/project/in`, writes `~/project/out/result`, then submits a note). `ssh-loopback` cases run
alone.

1. **`steward-lease-ends`**: alice starts an agent with a 2 s deadline; it writes its result;
   the deadline destroys its budget (the audit shows the subtree gone); alice's session receives
   the lease-ended notice (the shell prints it) and bob's does not. Forbid any `exited` line for
   the steward.
2. **`steward-end-lease`**: alice starts an agent spinning for ever; `end_lease` from her
   session ends it within the lease-end target (a lease's end p99 125 ms from the decision,
   measured by the case from the call to the notice, net of audits as `sched-latency` nets
   them); bob cannot end it (`NotSponsor`); a vault session of alice's cannot (`Labelled`).
3. **`steward-sub-agent-dies-with-agent`**: the agent creates a sub-budget with a longer
   deadline and a thread spinning in it; the lease's end takes both (the audit).
4. **`steward-approve-box`** (`[[session]]`): the agent submits a request for a labelled agent in
   `{alice-secrets}`; alice's unlabelled session prints "an approval is waiting" and bob's prints
   nothing; `ssh approve@box` with alice's approval key shows the screen; `approve <id> <hash>`
   grants it and the labelled agent runs (it writes to the labelled volume); `approve` with a
   wrong hash is refused on the channel and nothing is granted; `approve@box` with alice's
   **login** key is refused by `sshd`.
5. **`steward-screen-inert`**: the request's reason is 64 characters of ANSI escapes, bidi and
   format characters; the screen shows them escaped (the case greps the channel's bytes for no
   0x1B and no byte outside 0x20..0x7E besides the line ends). The verdict is the bytes `sshd`
   relayed.
6. **`steward-pending-cap`**: an agent submits `PENDING_CAP` + 1 requests; the last is refused
   (`Cap`); a second agent of alice's in the same label set still gets its fair share (one
   accepted); a dead session's requests are gone (the next `approve@box` screen lists none of
   them).
7. **`steward-blame-lockout`**: bob's session crashes `fsd:data` three times within the window
   (a hostile request the case plants; the case must name the real fault it uses, or use the
   bench's `fsd` fault injection if one exists, and say which); every budget of bob's `{}` is
   destroyed, his next login is refused until the window passes (the case advances virtual
   time), alice's session is untouched and `fsd` is restarted by `init`. A vault session of
   bob's is not ended by his unlabelled crashes (a second run with the crash from the vault,
   the unlabelled session untouched).
8. **Host:** the approval-channel line parser (fuzz target); the badge classes for the new
   operations; `end_lease` answered before admission (a server busy with a flood still answers
   it: a unit test on the receive loop); the blame timeout.

## Page lines (exact text in the report)

- **steward.md:** "Leases", "The powerbox and approvals", "Crash blame" statuses to `built ·
  tested` with the cases; "Authority" gains the `approve@box` channel's badge class; R38, R39,
  R40, R41 statuses. Residual: "The steward's restart ends every session (M1)".
- **sshd.md:** `approve@box` paragraph and R67 status to built with cases 4 and 5.
- **sessions.md:** "`approve@`" status to built; **agents.md:** its M1 status lines.
- **init.md:** "Restarts and reboots": the blame half's status to built with case 7.
- **SECURITY.md:** R38, R39, R40, R41 rows.
- **scheduling.md** "Responsiveness": a sentence that the lease-end target is also measured
  with the real steward in `steward-end-lease` (the full re-measurement is STEWARD4's).

## Owned paths

- `servers/steward/**`; `servers/sshd`: the `approve` user, the approval channel's consol end,
  the approval-key path; `libs/wire/tables/steward.md` (the new operations' fields).
- `userland/otp/redoubt` or the shell: the agent module and the notice's printing, only as a
  consumer of BEAM4's natives (ask before touching a native).
- `image/**`: the agent module, the approval keys in `tests/keys/`.
- The cases and pages above.

**Not yours:** the core's guards, rows and constants (a case that needs one changed is a design
question); `init`'s restart machine; the SSH core; `fsd`.

## Gates

The whole bench on both widths under the job pool's rules; `elixir-oracles` green; the host
tests; fmt; the unsafe ratchet; the size budget; doccheck. Report each command with its exit
code.

## Not here

Declassification and push, the latency bench with the real steward (STEWARD4); the audit file
(M3, M4); `gatewayd` and keys in leases (M4, M5).

## Checkpoint

After point 1's first case (`steward-lease-ends` green on one width), one progress line with
the branch and the measured lease-end latency.

## 2026-10-06: a design question handed from STEWARD2 (architect-15)

**R41 has no mechanism yet.** STEWARD2 found that `new_connection(root, quota)` carries no
scope and only the process that makes a handle stamps it, so a session's connections are not
bound to the revocation scope the core creates for them; they end by the launcher's disconnect.
Before any lease relies on R41 (a server narrowing a lease's connections by a scope it is
handed), design the means and bring it to the thread as a checkpoint, with the page line for
steward.md R41 and the kernel or protocol page it touches: candidates are a scope handle a
server accepts at `new_connection` and kills connections by (a `ninep_common` change, every
file server's), or a disconnect-by-scope the steward drives. The brief's point 1 ("Narrowing is
a revocation scope") is built only after that ruling.
