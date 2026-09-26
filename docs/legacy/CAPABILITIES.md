# Capabilities, IPC and principals

Designed, not built. Owns: why handles, how IPC and minting are used, revocation policy, principals,
agents, projects, the powerbox and approvals. The precise kernel rules: KERNEL-SPEC.md. Labels:
CONTAINMENT.md. Budgets: RESOURCES.md. Launching and signing: PACKAGES.md.

## Why change what Redoubt has
Stock Redoubt uses password capabilities: a server is a 128-bit random `SID`, and knowing it is enough
to connect (`TryConnect(SID)`). Knowledge is authority, so:
- no revocation (a number cannot be un-known);
- no attenuation (no read-only version of a number);
- silent leaks (a SID in a log is authority for its reader);
- confused deputies (a server cannot tell which grant a request came through).

## Handles
A handle is (object, badge, stamp), an index into a kernel-held, per-process table: unforgeable,
because a process can only name indices into its own table. The kernel knows its four object kinds (a
device object takes one of three forms; KERNEL-SPEC.md) and nothing about users, agents or policy.
- **No rights bits.** Every handle can be used and copied. Non-transferable handles would not help:
  a holder can proxy. Delegation is bounded by stamps (below) and labels (CONTAINMENT.md).
- **Badges.** A server mints a handle with a badge; every request through it carries the badge, so
  the server knows which grant is in use (e.g. "9P root `/home/alice/project`, read-only"). The
  kernel guarantees the badge; the server gives it meaning.
- **Attenuation** is asking the server to mint a narrower badge. Semantic rights (read-only, a
  subdirectory, a port range) are enforced by the server.
- **One badge, one client.** Every copy of a handle carries the same badge, and the kernel gives a
  server no per-process identity, so all holders of a copy share one connection (one 9P fid table).
  Rule: **a launcher never passes its own connection to a child.** It asks the server for a fresh
  connection for each child (`new_connection`, NAMESPACES.md, which mints one and returns it with a
  random connection id) and passes that. Otherwise a hostile agent started by Alice's shell could
  read, close or wipe her open files. Servers also key per-client state by (badge, account, label
  set) as a second line of defence (CONTAINMENT.md).
- **Disconnect, not a kernel notice.** The kernel does not tell a server when a client's handles
  are gone. Only the holder of a connection id can `disconnect(id)`, which frees that connection
  and everything minted under it. A launcher disconnects a child's connections when it receives the
  child's exit notice, and on the same notice **releases the child's grants from typed servers**,
  which work the same way (WIRE.md states the pattern: `grant` and `release` are the typed
  counterparts of `new_connection` and `disconnect`); the steward does so at logout and at lease
  expiry. Stated residual: a
  launcher that dies without disconnecting leaks its children's connections until its own
  connection is freed, and the leak counts against its own (account, label set).

## IPC
Two primitives, each with a timeout (KERNEL-SPEC.md, Messages):
- **`call`** waits for the reply and may **lend** one writable buffer, zero-copy: the pages are
  mapped into the server and unmapped from the caller until the call ends. Read-only data is lent
  writable; a client already trusts the server with it.
- **`send`** is one-way and may **transfer** pages for good, zero-copy. The receiver must opt in with
  a maximum; the pages' owner and payer change together. Handing pages over and waiting for an answer
  is a `send` then a `call`.
- **No kernel queue.** A sender waits until the receiver takes its message; waiting senders are
  served round-robin by account and label set, and each may have only a few waiting per endpoint,
  so thousands of threads from one account cannot starve another (CONTAINMENT.md says why the label
  set counts too).
- **Every message carries the caller's badge, account and labels.** Servers use them for admission
  and label checks (CONTAINMENT.md). The raw budget id does not travel.
- **The other side going away** (death, timeout or revocation) never corrupts a server: a lent
  buffer stays with the server, charged to it, until it replies, and the server is told with an
  abandoned-call notice, so it replies and frees the call (KERNEL-SPEC.md, R3). A server's budget
  pays for its open lends while it holds them.
- **Exit notices.** Whoever creates a process names an endpoint (a badge-0 handle) and receives one
  exit notice there; the process object, charged to its creator, holds the notice. There are no
  death subscriptions.
  There is no per-process kill: a process that must be killable on its own gets its own budget, and
  killing it means destroying that budget.
- **Interrupts** are received like messages: a driver thread waits on its IRQ handle.

**The runtime owns a lend across the call** (answer 167). The consuming API and matching
ABI/kernel completion paths are present in **this checkout**; this is
not WP-IPC1 acceptance. Model, K5 timer and simultaneous multi-hart completion-race gates remain
outstanding (STATUS.md). The safe API consumes its
optional `Buffer`, rather than borrowing a buffer that might disappear. Its outcome carries the
kernel status, the buffer only when returned, and a reply only when the kernel reports a committed
record (KERNEL-SPEC.md, IPC completion). A consumed buffer is disarmed without accessing or
unmapping its old address; a returned buffer remains usable and has exactly one owner. A partial
reply on `OutOfMemory` still owns its delivered words and handle slots. A higher-level client may
expose them or discard them while closing every surviving handle, but cannot erase the outcome
with an early error return. Its own reply record remains backed and exclusively managed while
the syscall runs. This contract is required of any facade above the runtime as well; an error
translation never silently loses a buffer or delivered handle. WP-IPC1 implements the contract.

## Minting and revocation
There are no per-capability revokers. **Budgets are the only revocation**: destroying a budget
revokes every handle stamped with it or a descendant, wherever the copies went.
- **Minting keeps the stamp by default.** A handle a server mints in answer to a request is stamped
  like the handle the request came through. Bob's narrower capability to `shared/sub` is therefore
  stamped with Alice's share, and dies with it, without Bob ever holding Alice's budget.
- **A budget handle can only narrow.** Minting "into" a budget is allowed only for the default stamp
  or a descendant of it. This is how the steward places a principal's capabilities in sub-budgets
  under that principal: it mints through the principal's capabilities, narrowing to a sub-budget.
  A budget handle is also a destroy right, so **a narrowing handle a server holds is always a
  revocation scope** created for that purpose, never a budget that holds processes: a compromised
  `fsd` could otherwise end every session.
- **Revocation scopes.** To make one grant revocable on its own, mint it into a **revocation scope**:
  a budget with zero limits (no pages, processes or weight), used only to be destroyed. Nothing runs
  in it, and its handle is never given to another principal.
- **Creating a process in a budget** counts it against that budget's process limit, charges its
  threads and memory there, and attributes it to that budget's account; the process object itself,
  which holds the exit notice, is charged to its creator (KERNEL-SPEC.md).
- **Leases** are budgets with a kernel deadline; the kernel destroys them when it passes. A lease
  is at most **`MAX_LEASE` = 24 h**, a steward constant (the kernel knows deadlines, not leases); the
  steward refuses a longer request rather than clamping it silently.
- **Budget ids are never reused**, so a stale stamp never matches a new budget. Ids identify; only
  handles grant.

Example (milestone 2): Alice shares `shared/`; the steward mints the share into a revocation scope
under Alice's budget and passes it to Bob. Bob asks `fsd` for `shared/sub`; the new handle keeps the
share's stamp. Alice un-shares: the scope is destroyed, and `sub` dies with it.

## Principals (policy, in the steward)
- A **principal** is a named, accountable identity: an authentication method, its capability set
  (namespace and service grants), and an audit identity. Humans, agents and projects are the same
  kind of principal; they differ in authentication and default policy, not mechanism.
- Each principal's top budget carries its **account** (set by the steward). Everything under it,
  including its agents, shares that account.
- A **session** is processes started with capabilities derived from a principal's set, never more.
- **No root, no sudo.** "Admin" means holding specific capabilities over shared things. In
  milestone 1 the principals come from the boot manifest (INIT.md); from milestone 2 the first owner
  is enrolled at first boot and delegates from there.
- **Nesting.** Every principal has its own space; its sponsor can destroy its budget. Principals can
  run their own servers and delegate into them.

## Agents
1. **Own principal, never an impersonation.** Every action is attributable to the agent. Every agent
   has an accountable **sponsor** (a human, or an agent with a human at the top of the chain).
2. **An agent's budget sits under its sponsor's, so it shares the sponsor's account.** Its requests
   count against the sponsor's admission limits for its label set, with a fair share per badge
   inside them, so an agent cannot lock its sponsor out; ending a lease is always accepted from the
   sponsor, ahead of admission. Three crashes blamed on the agent end every session and lease of the
   sponsor's with that label set (CONTAINMENT.md). The sponsor answers for its agents.
3. **Delegation only narrows.** Human -> agent -> sub-agent, each step attenuated, the chain
   recorded. Agents may spawn sub-agents freely, as budgets **inside their own budget**: an agent
   holds only its own budget handle, so it cannot create siblings, and destroying the agent's budget
   (lease expiry) ends its sub-agents with it (R10), whatever their own deadlines. A new durable
   principal, or a budget with more labels than its parent, needs the steward and an approval.
4. **Task-scoped leases:** "read `~/project`, write `~/project/out`, connect to `203.0.113.0/24:443`,
   2 hours, 256 MB, 4 processes, weight 20", at most `MAX_LEASE`.
5. **Assume every agent is compromised** by something it read. A hijacked agent can do what its
   capabilities allow, until its lease ends, and nothing more. It can run code it wrote, but never
   with more authority than it holds (PACKAGES.md).
6. **Labels** bound what an agent can leak; capabilities bound what it can do. The isolation unit is
   the label set, not the capability set (CONTAINMENT.md, Labels; TENETS.md, Purpose and threat model).
7. **No credentials in agent memory.** Agents use keys through `keyd` and, later, models through
   `gatewayd`, which holds API keys. **A lease carries `keys` only from milestone 2**, and then only
   if its approval named the key: a principal's key comes with **the one message shape it may sign**
   (a `keyd` badge names one key and one purpose — for SSH, a signature over the session identifier
   `keyd` computed itself), never arbitrary bytes, or a hijacked agent is a signature oracle that
   lets its peer log in as its sponsor elsewhere. In milestone 1 no session and no lease holds
   `keys` at all: `keyd`'s only purposes are the host key and audit signing, and `grant` mints
   nothing but the granter's own key and purpose, so there is nothing to hand out (INIT.md's worked
   example).
8. **Runtime:** each agent is its own beamlet VM (one VM = one trust domain); sub-agents with
   different authority are separate VMs.
9. **Everything is audited:** mint, delegate, revoke, approve, lease expiry, with the principal chain.

## Projects (group principals; milestone 2)
A **project** is a principal sponsored by several members, with its own budget, volume
(`fsd:project-x`), package directory and profile, and optionally a label. Membership is capabilities
minted into a revocation scope per member; removing a member destroys it. A labelled project is
worked on in project vault sessions (`ssh alice+project-x@box`); declassifying one of its items needs
a project owner's approval (which members count is project policy). No kernel mechanism is involved.

## The powerbox and approvals
The powerbox (in the steward) grants authority a principal lacks, and confirms a principal's own
high-stakes steps. It is needed when an agent asks its sponsor, when a principal asks the holder of a
shared resource, or for a high-stakes step on one's own behalf (a new trusted key, a
declassification). Most things need none.

- **Out of band, like 2FA.** An approval happens only where the steward alone talks to the terminal:
  `ssh approve@box` (and, from milestone 2, the physical console for the first owner). Sessions and
  agents only *notify* that an approval is waiting. This is the tenet "the requester can never
  influence the approval channel" (TENETS.md).
- **The approval key is the person's own.** Keys that authenticate a person to the box (login and
  approval) stay on the person's machine or security key and **never live in `keyd`**. The steward
  refuses to enrol a key in both roles, and `init` refuses a manifest listing one key both as a
  principal's login or approval key and as a `keyd` key; `sshd` rejects authentication with any
  public key `keyd` holds; session network capabilities never include the box's own addresses, which
  include any address that routes back to the box (for example, QEMU's gateway with a forwarded
  port). Otherwise a hijacked session could log in to `approve@box` over loopback, signing with
  `keyd`, and approve itself.
- **Rendering.** The steward renders from the structured request: who is asking (the requester's
  kind, such as agent or session, and its steward-assigned name, `agent-7`, besides its principal),
  what, where, how long, and the label consequences. Every rendered field is a whitelist of
  printable ASCII (0x20-0x7E; anything else is escaped), length-capped: stripping control
  characters alone would miss bidi and format characters (U+202E, U+2066, U+200B). An unlabelled
  requester's free-text reason is quoted, escaped and marked untrusted. A **labelled** requester's
  request shows only text the steward generates (kind, target, size); its free text reaches the
  screen only through declassification (CONTAINMENT.md).
- **Binding.** Each request has a random 64-bit id and a hash of its exact content; approving
  confirms both. The request is frozen until answered; any change makes it a new request.
- **Limits and labels.** Each (account, label set) has a cap on pending requests. A request from a
  labelled budget is shown only to principals owning every label it carries; otherwise it is refused
  at submission. Its "approval waiting" notification reaches only channels whose labels ⊇ the
  request's, and `approve@box`.
- **Milestone 1** has one approval path: `ssh approve@box` with the person's own SSH key. An approval
  grants no more than the approver holds. Stated milestone 1 residual: `approve@` shares `sshd` with
  the most hostile input, so a `sunset` bug reached from any channel controls the screen and a
  network flood delays approvals. Milestone 2 gives `approve@` its own `sshd` instance or the
  console.
- **Later:** high-stakes approvals in a fresh `ssh approve-hs@box` connection that accepts only the
  approver credential, a FIDO `sk-` key; `sshd` must check the signature's user-verification flag,
  not just the key type, and `sunset`'s `sk-` support is unverified. A physical approval button or
  display on the board is an option (PLATFORM-FPGA.md).

## Prior art
seL4, Zircon, KeyKOS/EROS, CapDesk, Polaris, Sandstorm, Capsicum, Android permissions, Plan 9 factotum.
