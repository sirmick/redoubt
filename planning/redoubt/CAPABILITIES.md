# Capabilities, IPC and principals

Designed, not built. Owns: handles, IPC, minting, revocation, exit messages, principals, agents,
projects, the powerbox and approvals. Labels: CONTAINMENT.md. Budgets: RESOURCES.md. Launching and
signing: PACKAGES.md.

## Why change what Xous has
Stock Xous uses password capabilities: a server is a 128-bit random `SID`, and knowing it is enough
to connect (`TryConnect(SID)`). Knowledge is authority, so:
- no revocation (a number cannot be un-known);
- no attenuation (no read-only version of a number);
- silent leaks (a SID in a log is authority for its reader);
- confused deputies (a server cannot tell which grant a request came through).

## Handles
The kernel knows processes, handles, endpoints, budgets and device objects. It knows nothing about
users, agents or policy.
- **A handle is (object, badge, stamp)**, an index into a per-process, kernel-held table.
  Unforgeable: a process can only use indices into its own table.
- **No rights bits.** Every handle can be used, moved and copied. Non-transferable handles would not
  help: a holder can proxy. Delegation is bounded by stamps (below) and labels (CONTAINMENT.md).
- **Badges.** A server mints a capability with an unforgeable tag; every request through it carries
  the badge, so the server knows which grant is in use (e.g. "9P root `/home/alice/project`,
  read-only"). The kernel guarantees the badge; the server gives it meaning.
- **Attenuation** is asking the server to mint a narrower badge. Semantic rights (read-only, a
  subdirectory, a port range) are enforced by the server.

## IPC
Two primitives. Each may carry handles and at most one buffer.

| | no buffer | lend read-only | lend writable | transfer |
| --- | --- | --- | --- | --- |
| **`call`** (waits for the reply) | yes | yes | yes | yes |
| **`send`** (one-way) | yes | no | no | yes |

- **All buffer modes are zero-copy:** the kernel remaps pages. A lent buffer is unmapped from the
  lender until the reply (no double fetch).
- **Transfer** moves pages to the receiver for good: owner and payer change together, so a page's
  owner is always its payer. A receiver gets transferred pages only if its `receive` declared that it
  accepts transfers and how many pages at most.
- **No kernel queue.** `send` waits until the receiver takes the message (rendezvous), so a message
  always occupies its sender's thread. `receive` takes a timeout.
- **The kernel attaches the caller's budget id, principal id and label set** to every message,
  unforgeably. Servers use them for admission limits and label checks (CONTAINMENT.md).
- **Server death:** the endpoint and handles to it survive (INIT.md); senders blocked on it and
  calls in flight get an error.
- **Exit messages.** Whoever creates a process names an endpoint at creation and receives one exit
  message there. There are no death subscriptions.

## Minting and revocation
There are no per-capability revokers. Budgets are the only revocation.
1. **Mint takes a budget handle.** The kernel accepts it only if that budget is the stamp of the
   capability the request came through, or a descendant of it. You can place a capability only in a
   budget below the one your own authority came from.
2. **Destroying a budget revokes every handle stamped with it or any descendant, wherever the copies
   went.** Calls in flight fail cleanly.
3. **Single-grant revocation is a sub-budget.** To make one grant revocable on its own, mint it into
   a fresh sub-budget and destroy that to revoke.
4. **The steward mints a principal's capabilities through that principal's root capability**, into
   sub-budgets under the principal. Destroying the principal's budget, or a sub-budget, revokes
   exactly what it should.
5. **Budget ids are 64-bit and never reused**, on both widths, so a stale stamp can never match a new
   budget. Ids identify; only handles grant.

**Leases** are budgets with a kernel deadline in monotonic time; the kernel destroys the budget when
it passes. **Killing** a process means destroying its budget. A restarted steward can enumerate and
destroy its descendant budgets, so no revocation record lives only in server memory.

Example: Alice shares `shared/`; the steward mints the share into a sub-budget of Alice's. Bob asks
`fsd` for a narrower capability to `shared/sub`; it can only land in that sub-budget or below, so
when Alice destroys the share, `sub` dies with it.

Model invariant: after budget B is destroyed, no process holds a handle stamped with B or any
descendant of B.

## Principals (policy, in the steward)
- A **principal** is a named, accountable identity: an authentication method, a root capability set
  (namespace and service grants), and an audit identity. Humans, agents and projects are the same
  kind of principal; they differ in authentication and default policy, not mechanism.
- A **session** is processes started with capabilities derived from a principal's root set, never more.
- **No root, no sudo.** "Admin" means holding specific capabilities over shared things. The first
  owner receives the root capability set at first boot and delegates from there (INIT.md).
- **Nesting.** Every principal has its own space; its sponsor can destroy its budget. Principals can
  run their own servers and delegate into them.

## Agents
1. **Own principal, never an impersonation.** Every action is attributable to the agent. Every agent
   has an accountable **sponsor** (a human, or an agent with a human at the top of the chain).
2. **Own durable space** when long-running; the sponsor can revoke all of it.
3. **Delegation only narrows.** Human -> agent -> sub-agent, each step attenuated, the chain
   recorded. Agents may spawn sub-agents in child budgets freely; a new *durable* principal, or a
   budget with more labels than its parent, needs the steward and an approval.
4. **Task-scoped leases:** "read `~/project`, write `~/project/out`, connect to `203.0.113.0/24:443`,
   2 hours, 256 MB, 4 processes, weight 20".
5. **Assume every agent is compromised** by something it read. A hijacked agent can do what its
   capabilities allow, until its lease ends, and nothing more.
6. **Labels** bound what an agent can leak; capabilities bound what it can do (CONTAINMENT.md).
7. **No credentials in agent memory.** Agents use keys through `keyd` and, later, models through
   `gatewayd`, which holds API keys; they never hold keys themselves.
8. **Runtime:** each agent session is its own beamlet VM (one VM = one trust domain); sub-agents with
   different authority are separate VMs.
9. **Everything is audited:** mint, delegate, revoke, approve, lease expiry, with the principal chain.

Humans authenticate with an SSH key (and the approver credential for high-stakes approvals) and hold
durable root sets. Agents are launched by their sponsor with a leased set; they have no password.

## Projects (group principals)
A **project** is a principal sponsored by several members, with its own budget (carved from the
sponsors), its own volume (`fsd:project-x`), its own package directory and profile, and optionally
its own label. Membership is capabilities minted into a **sub-budget per member**: removing a member
destroys their sub-budget. Members bind the project into their namespace (`/proj/x`) and run its
tools if they trust the signer. A labelled project is worked on in project vault sessions
(`ssh alice+project-x@box`); declassifying one of its items needs a project owner's approval
(which members count is project policy). No kernel mechanism is involved.

## The powerbox and approvals
The powerbox (in the steward) grants authority a principal lacks, and confirms a principal's own
high-stakes steps. It is needed when an agent asks its sponsor, when a principal asks the holder of a
shared resource, or for a high-stakes step on one's own behalf (a new trusted key, a
declassification). Most things need none (installing a trusted package, running your own server on a
granted resource).

- **Out of band, like 2FA.** An approval happens only in an approval session where the steward alone
  talks to the terminal (`ssh approve@box`), or on the board console. Sessions, agents and the
  browser GUI only *notify* that an approval is waiting. This is the tenet "the requester can never
  influence the approval channel" (TENETS.md).
- **Rendering.** The steward renders from the structured request: what, where, how long, and the
  label consequences. Printable text only, control characters stripped, every field length-capped.
  Names are steward-assigned (`agent-7`, `request 41`), never requester-chosen. The requester's
  free-text reason is quoted, escaped and marked untrusted. A declassification shows the item's
  content (bounded), not just its name.
- **Binding.** Each request has a steward-issued id and a hash of its exact content; approving
  confirms both. The request is frozen until answered; any change makes it a new request.
- **Labels.** A request from a labelled budget is shown only to principals owning every label it
  carries; otherwise it is refused at submission.
- **Tiers.** *Routine* (inside the approver's own space, lease-limited): `ssh approve@box`, possibly
  later (asynchronous). *High-stakes* (shared infrastructure, a new durable principal, a new trusted
  signing key, a budget with added labels, declassification): `ssh approve-hs@box`, a fresh
  connection that accepts only the approver credential (a FIDO `sk-` key with user verification).
- An approval grants no more than the approver holds.
- Later option: a physical approval button or display on the board (PLATFORM-FPGA.md).

## Prior art
seL4, Zircon, KeyKOS/EROS, CapDesk, Polaris, Sandstorm, Capsicum, Android permissions, Plan 9 factotum.
