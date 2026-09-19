# Containment: information flow, covert channels, and a testable model

Status: agreed direction, 2026-09-18. Nothing here is built yet. Builds on CAPABILITIES.md.

Capabilities contain *authority*: what a process can do. Agents also need *information*
containment: what a process can leak, including through authority it legitimately holds (the LLM
gateway, an allowed `git push`). This is the confinement problem (Lampson, 1973). This note is the
design answer.

## Information-flow labels
Decentralized information flow control, as in HiStar, Flume and Asbestos.
- **Labels on data.** A secrecy label (e.g. `alice-secrets`) is attached to data by its owner: files
  and directories in fs servers, key material in keyd, message payloads.
- **Taint on read.** A process that reads labelled data carries that label from then on. Labels live
  on processes and budgets; the kernel propagates them on IPC and handle transfer (a message from a
  tainted process taints its receiver unless the receiver is cleared for the label).
- **Sinks check clearance.** A tainted process may write only to sinks cleared for all its labels.
  Servers that talk to the outside world (ip stacks, the LLM gateway, sshd/webd output to other
  principals) are sinks with explicit clearances; by default an external sink is cleared for nothing.
- **Only the owner declassifies**, as an explicit, audited act (a powerbox request with the tier
  rules of CAPABILITIES.md).
- **Effect for agents:** a confined agent that reads `~/secrets` loses the gateway and `/net` for
  that data automatically; the powerbox shows the consequence of an approval ("lets data labelled
  alice-secrets leave via github.com"), not only the request.
- **Integrity labels** (the dual, later): data from untrusted sources (web content, model output)
  carries a low-integrity label; high-integrity sinks (system config, signing) refuse it without an
  endorsement. This is the structural answer to prompt injection reaching privileged actions.
- Replaces the static "exfiltration query" in CAPABILITIES.md, which fails once a principal reads a
  secret and is then granted egress.

Kernel cost: a small label set per process and budget, a subset check on IPC and handle transfer.
Label meaning lives in userspace; the kernel only compares sets. HiStar showed this fits a small kernel.

## Covert channels
They never reach zero. The goal: low bandwidth, stated, audited; the rest handled in RTL on the
FPGA target. Channels our own design created, and their fixes:

| Channel | Fix |
| --- | --- |
| Shared content-addressed store: add a blob, probe for it | A principal sees only its own profile's closure; existence of others' blobs is not observable |
| Overcommitted budgets: allocation failure reveals a sibling's usage | Confined principals get hard reservations, not overcommit |
| Shared fs metadata (free space, `statfs`, 9P versions) | Confined agents get their own volume |
| High-resolution time (`rdtime` readable from U-mode) | The kernel can disable it per process (`scounteren`), trap and return a coarse clock, as browsers do |
| CPU contention between budgets | Stride shares are fixed per budget; the remainder is an RTL/partitioning concern |

## Availability in shared servers
Donation charges server CPU to the caller, but server threads and queue slots are shared: one
client flooding `fs:data` can make others wait. The shared server library gives each badge an
admission limit (queue slots, in-flight requests); the bench gets a flooding test.

## System signing key
One key owning every machine is a single point of failure. System packages need **M-of-N
signatures**, builds are **reproducible** and confirmed by independent builders, and updates have
**rollback protection** (VERIFIED-BOOT.md's open items).

## Control files
Plan 9 style `ctl` files would make every server write a text parser. Instead: one specified
grammar and one shared, fuzzed parser (or typed binary control messages). No server parses control
text on its own.

## The trusted path (accepted limitation)
The browser GUI puts a browser in the human's approval path; a compromised page can show one
request and have the user approve another. We accept this for routine approvals. In exchange, the
OS contains no graphics code at all. High-stakes approvals can use a device with its own trusted
display (the board console, or a Precursor) when one is available; not required.

## An executable security model, first
The design is what must hold, so it gets its own attack surface before the kernel does: a small
executable model (~1-2k lines, Rust; or TLA+/Alloy) of capabilities, minting and derivation,
revocation, budgets, labels and the powerbox. Invariants, checked by model checking and property
tests and attacked by red-team agents from several vendors writing counterexample traces:
- a principal never holds a capability not derived from its grants;
- revocation removes everything it is meant to (the derivation rule is still open: see below);
- data labelled L never reaches a sink not cleared for L without the owner declassifying;
- budgets never go negative; destroying one returns everything.
The kernel is then built to the model, and the bench checks conformance against it.

## Revocation of derived capabilities: by budget, not by derivation tree (decided)
A server may mint a new, narrower capability D in answer to a request made through C. D is not a
copy of C, so revoking C does not reach D. Options were a kernel derivation tree (seL4-style),
servers checking C's liveness on every use, or revocation at the budget level. **Decided: budget
level**, as the simplest mechanism that fits the tenets.
- **Every capability records the budget it was minted into** (the recipient's budget at mint time),
  and keeps it when copied or transferred.
- **Destroying a budget revokes every capability minted into it or its descendants, wherever the
  copies went.** A lease ending, an agent being revoked or a session ending therefore removes
  everything granted to or obtained by that principal, including anything it passed to others.
- **Revoking one capability** kills it and its copies only. Things obtained through it survive until
  the budget ends. Taking back *one* grant mid-task is coarse by design: end the lease (or give
  agents short leases and renew them).
- Kernel cost: one field per capability and a sweep on budget destruction. No derivation tree.
- Model invariant: after budget B is destroyed, no process holds a capability minted into B or any
  descendant of B.
