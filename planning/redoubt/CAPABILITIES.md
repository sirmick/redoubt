# Capabilities and principals (users and AI agents)

Status: agreed direction, 2026-09-18. Nothing here is built yet. Packages and code signing:
PACKAGES.md. Namespaces and 9P: NAMESPACES.md.

## Why change what Xous has
Stock Xous uses password capabilities: a server is a 128-bit random `SID`, and knowing it is enough
to connect (`TryConnect(SID)`). Knowledge is authority, so there is no revocation (a number cannot
be un-known), no attenuation, silent leaks (a SID in a log is authority for its reader), and no way
for a server to tell which grant a request came through (confused deputy).

## Layer 1: kernel mechanism
The kernel knows processes, handles and resource containers. Nothing about users, agents or policy.
- **Handles.** A per-process, kernel-held table; a handle is an index into it, meaningless in any
  other process. Unguessable and unforgeable (as seL4, Zircon).
- **Transfer.** Messages carry handles, moved or copied; copy needs the *duplicate* right.
- **Badges.** A server mints a capability with an unforgeable tag; every request through it carries
  the badge, so the server knows which grant is in use (e.g. "9P root `/home/alice/project`,
  read-only"). The kernel guarantees the badge; the server gives it meaning.
- **Attenuation only goes down.** Generic rights (send, transfer, duplicate) are kernel-enforced;
  semantic rights (read-only, a port range) are server-enforced through the badge. Narrowing means
  asking the server to mint a narrower capability. No operation widens authority.
- **Revocation.** Minting yields a revoker; revoking kills every copy wherever it travelled.
  Requests in flight fail cleanly.
- **Death notification.** A dead server's capabilities go dead; callers get errors; holders may ask
  to be told.
- **Leases** are revocation on a timer (a lease service holds the revoker).

## Layer 2: principals (policy, in the steward, a Rust server)
- A **principal** is a named, accountable identity: an authentication method, a root capability set
  (namespace and service grants), and an audit identity. Humans and agents are the same kind of
  principal; they differ in authentication and default policy, not mechanism.
- A **session** is processes started with capabilities derived from a principal's root set, never more.
- **No root, no sudo.** "Admin" is holding specific capabilities over shared things (the NIC, the
  router, the store's GC). The first owner receives the root capability set from init at install and
  delegates from there.
- **Nesting.** Every principal has its own space; its sponsor holds a revoker over it. Principals
  can run their own servers (a littlefs over an image they own) and delegate into them.

## Agents as first-class principals
1. **Own principal, never an impersonation.** Every action is attributable to the agent. Every agent
   has an accountable **sponsor** (a human, or an agent with a human at the top of the chain).
2. **Own durable space** (home, memory store) when long-running; the sponsor can revoke all of it.
3. **Delegation only narrows.** Human -> agent -> sub-agent, each step attenuated, the chain
   recorded. Agents may spawn attenuated sub-agents freely; a new *durable* principal needs approval.
4. **Task-scoped leases:** "read `~/project`, write `~/project/out`, reach `api.example.com:443`,
   2 hours, 1 GB disk, 10 CPU-minutes". Quotas come from resource accounting (PLAN backlog 3).
5. **Assume every agent is compromised** by something it read. The goal is a bounded blast radius:
   a hijacked agent can do what its current capabilities allow, until its lease ends, and nothing more.
6. **Exfiltration is queryable.** Policy can refuse, by default, any principal holding both
   secret-read and unrestricted egress. (Later: drop sensitive capabilities after an agent reads
   untrusted content, a simple information-flow rule.)
7. **No credentials in agent memory.** Agents hold a capability to an **LLM gateway** (which holds
   the API key, meters token and money budgets, logs calls) and sign through the key server.
8. **Runtime:** each agent session is its own beamlet VM (one VM = one trust domain); sub-agents with
   different authority are separate VMs.
9. **Everything is audited:** mint, delegate, revoke, approve, lease expiry, with the principal chain.

Humans: authenticate with an SSH key or passkey, hold durable root sets, approve requests. Agents:
launched by their sponsor with a leased set; no password, their authority is their capabilities.

## Escalation: the powerbox
Needed only when a principal wants authority it does not hold: an agent asks its sponsor, or a
principal asks the holder of a shared resource. Most things need none (installing software for
yourself, running your own network stack on a granted interface).
- **Structured requests.** The powerbox renders what, where and how long from a structured request.
  The requester's free-text reason is shown as clearly marked, quoted, untrusted text.
- **An approval is no stronger than the approver acting directly**, so it may happen in any
  authenticated session of the approver, tiered by stakes:
  - *routine* (inside the sponsor's own space, lease-limited): any sponsor session, even async;
  - *high-stakes* (shared infrastructure, new durable principals, secret-read plus egress): fresh
    hardware-key authentication (FIDO/passkey touch) on the trusted path.
- **Trusted path.** The requester can never influence the approval channel: prompts appear in a
  region session programs cannot write (over SSH, the trusted SSH server reserves a status area or
  channel), and the answering keystrokes go to the powerbox, never to the requester.

## Kernel work this implies (PLAN backlog 1, 3)
Handle tables replacing SID connects; handle transfer in messages; badges; revokers; death
notification; resource containers charged per principal. `xous-names` survives, if at all, as a
boot-time convenience that hands out handles by manifest, not by name.

## Prior art
seL4 and Zircon (handles, badges), KeyKOS/EROS (revocation, confinement), CapDesk, Polaris and
Sandstorm (powerbox), Capsicum, Android permissions (declared, granted), Plan 9 factotum (keys in a
separate agent).
