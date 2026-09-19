# Capabilities and principals

Designed, not built. Owns: handles, minting, revocation, principals, agents, the powerbox and
approvals. Labels: CONTAINMENT.md. Budgets: RESOURCES.md. Packages and signing: PACKAGES.md.

## Why change what Xous has
Stock Xous uses password capabilities: a server is a 128-bit random `SID`, and knowing it is enough
to connect (`TryConnect(SID)`). Knowledge is authority, so:
- no revocation (a number cannot be un-known);
- no attenuation (no read-only version of a number);
- silent leaks (a SID in a log is authority for its reader);
- confused deputies (a server cannot tell which grant a request came through).

## Kernel mechanism
The kernel knows processes, handles, endpoints and budgets. It knows nothing about users, agents or
policy.
- **Handles.** A per-process, kernel-held table; a handle is an index into it, meaningless in any
  other process. Unforgeable (a process can only use indices into its own table).
- **A handle is (object, badge, budget stamp).** No rights bits: every handle can be used, moved and
  copied. A "non-transferable" handle would not stop delegation, since the holder can proxy
  requests; what bounds delegation is the stamp (below) and labels (CONTAINMENT.md).
- **Transfer.** Messages carry handles, moved or copied.
- **Badges.** A server mints a capability with an unforgeable tag; every request through it carries
  the badge, so the server knows which grant is in use (e.g. "9P root `/home/alice/project`,
  read-only"). The kernel guarantees the badge; the server gives it meaning.
- **Attenuation.** The only way to narrow authority is to ask the server to mint a narrower badge.
  Semantic rights (read-only, a subdirectory, a port range) are enforced by the server.
- **Caller identity.** Every message also carries the sender's budget id, unforgeable. Servers use
  it for admission limits (CONTAINMENT.md) and labels.
- **Death notification.** A dead server's capabilities fail; callers get errors; holders may ask to
  be told.

## Revocation: budgets only
There are no per-capability revokers. One mechanism, three rules:
1. **Every handle carries a budget stamp.** A capability minted in answer to a request made through
   capability C takes **C's stamp**, not the recipient's budget. Copies keep the stamp.
2. **Destroying a budget revokes every handle stamped with it or any descendant, wherever the copies
   went.** Requests in flight fail cleanly.
3. **Single-grant revocation = a sub-budget.** To make one grant revocable on its own, mint it into
   a fresh sub-budget and destroy that to revoke. The steward does this for every share and lease.

**Leases** are budgets with a deadline: a kernel field in monotonic time; the kernel destroys the
budget when it passes. A restarted steward can enumerate and destroy its descendant budgets, so no
revocation record lives only in a server's memory.

Why the stamp rule: if Alice shares `shared/` with Bob and Bob asks `fsd` for a narrower capability
to `shared/sub`, that new capability inherits the share's stamp and dies with it. Stamping it with
Bob's budget would let it outlive Alice's revocation.

Model invariant: after budget B is destroyed, no process holds a handle stamped with B or any
descendant of B.

## Principals (policy, in the steward)
- A **principal** is a named, accountable identity: an authentication method, a root capability set
  (namespace and service grants), and an audit identity. Humans and agents are the same kind of
  principal; they differ in authentication and default policy, not mechanism.
- A **session** is processes started with capabilities derived from a principal's root set, never more.
- **No root, no sudo.** "Admin" means holding specific capabilities over shared things (the NIC, the
  store's garbage collector). The first owner receives the root capability set at first boot and
  delegates from there (INIT.md).
- **Nesting.** Every principal has its own space; its sponsor can destroy its budget. Principals can
  run their own servers (a littlefs over an image they own) and delegate into them.

## Agents
1. **Own principal, never an impersonation.** Every action is attributable to the agent. Every agent
   has an accountable **sponsor** (a human, or an agent with a human at the top of the chain).
2. **Own durable space** (home, memory store) when long-running; the sponsor can revoke all of it.
3. **Delegation only narrows.** Human -> agent -> sub-agent, each step attenuated, the chain
   recorded. Agents may spawn attenuated sub-agents freely (child budgets); a new *durable*
   principal needs approval.
4. **Task-scoped leases:** "read `~/project`, write `~/project/out`, connect to `203.0.113.0/24:443`,
   2 hours, 1 GB disk, weight 20".
5. **Assume every agent is compromised** by something it read. A hijacked agent can do what its
   capabilities allow, until its lease ends, and nothing more.
6. **Information containment by labels** (CONTAINMENT.md): capabilities bound what an agent can do;
   labels bound what it can leak.
7. **No credentials in agent memory.** Agents hold a capability to `gatewayd` (which holds API keys,
   meters token and money budgets, logs calls) and sign through `keyd`.
8. **Runtime:** each agent session is its own beamlet VM (one VM = one trust domain); sub-agents with
   different authority are separate VMs.
9. **Everything is audited:** mint, delegate, revoke, approve, lease expiry, with the principal chain.

Humans authenticate with an SSH key or passkey and hold durable root sets. Agents are launched by
their sponsor with a leased set; they have no password, their authority is their capabilities.

## The powerbox and approvals
The powerbox (in the steward) grants authority a principal lacks. It is needed only when an agent
asks its sponsor, or a principal asks the holder of a shared resource. Most things need none
(installing software for yourself, running your own server on a granted resource).

- **Structured requests.** The steward renders what, where, how long, and the label consequences
  ("lets data labelled `alice-secrets` leave via 198.51.100.7") from the structured request. The
  requester's free-text reason is shown only as marked, quoted, untrusted text.
- **Approvals are out of band, like 2FA.** An approval happens only in a separate session where the
  steward alone talks to the terminal (`ssh approve@box`), or on the board console. Sessions, agents
  and the browser GUI can only *notify* that an approval is waiting; they never approve. This is the
  tenet "the requester can never influence the approval channel" (TENETS.md).
- **Tiers.** *Routine* (inside the sponsor's own space, lease-limited): any approval session of the
  sponsor, possibly later (asynchronous). *High-stakes* (shared infrastructure, a new durable
  principal, a new trusted signing key, declassification): fresh authentication with the approver's
  hardware key in the approval session.
- An approval grants no more than the approver holds.
- Later option: a physical approval button or display on the board.

## Prior art
seL4 and Zircon (handles, badges), KeyKOS/EROS (revocation, confinement, the proxy argument against
rights bits), CapDesk, Polaris and Sandstorm (powerbox), Capsicum, Android permissions (declared,
granted), Plan 9 factotum (keys in a separate agent).
