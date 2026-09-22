# Containment: labels, shared servers, channels, the model

Designed, not built. Owns: information-flow labels and their policy, sessions and vaults,
declassification, the shared server library, crash blame, covert and timing channels, the executable
security model. The kernel's rules: KERNEL-SPEC.md (R1, R6, `budget_usage`). Capabilities and IPC: CAPABILITIES.md.

Capabilities contain *authority*: what a process can do. Agents also need *information* containment:
what a process can leak, including through authority it legitimately holds (an allowed `git push`).
This is the confinement problem (Lampson, 1973).

## Labels
Decentralized information flow control in the Flume/HiStar style, with labels fixed per budget.
- **The isolation unit is the label set, not the capability set** (TENETS.md, The use case; tenet 2).
  Capabilities bound what a budget can *do*; labels bound what it can *leak*, and data moves only
  along labels. Two budgets with different handle sets but equal label sets are **one trust domain**:
  the OS sees no boundary between them, and a handle passed from one to the other is not a crossing.
  Two budgets with differing label sets have no path the OS carries a message over (R1).
- **A label is a name** such as `alice-secrets`, owned by a principal; the kernel sees a 64-bit id.
- **Labels live on budgets and volumes.** A budget's label set is fixed when it is created: it may
  read data with those labels, and the kernel assumes it has read all of it. Children inherit their
  parent's labels. **Only the steward creates a budget with more labels than its parent**, after an
  approval. A volume has one fixed label set, **set by the boot manifest or the steward, never read
  from the medium**.
- **Reading above your labels fails.** There is no taint at run time: an unlabelled agent that
  stumbles on labelled data gets an error and keeps its network. Nobody can taint someone else.
- **Between user budgets, calls and sends need equal label sets** (the kernel checks). Data enters a
  label by being read down: a vault session reads it from an unlabelled volume. Nothing writes into
  a label from outside it: every write needs equal labels (`check`, below). **Read-down is itself a
  channel** when the lower side is adversarial: an unlabelled agent that can write the volume a
  labelled agent reads has a low-to-high path. A confined domain (TENETS.md, The high/low pair) reads
  no shared unlabelled data; input arrives by an audited push from the steward (Push, below).
- **System servers are exempt from the kernel check and enforce labels themselves**, using the
  caller's label set the kernel attaches to every message (the shared server library, below).
- **A receive right is never handed across label sets.** R1 compares a sender with the endpoint's
  owner, not with whichever process receives (KERNEL-SPEC.md, I7), so handing a badge-0 handle to a
  budget with other labels would deliver messages nobody compared with it. Handing one out is
  delegation; a system server that hands one across label sets is buggy, not the kernel.
- **Budget observation obeys labels.** Reading a budget's usage, or receiving its exit notices,
  needs the reader's (for a notice, the exit endpoint's owner's) labels ⊇ the target's, unless the
  reader is class `system` (like the message check: `init` and the steward must see labelled
  processes exit). A parent's usage counts its children's limits, never their live usage, so a
  labelled child cannot signal through its parent's counters.
- **Sinks** (servers whose output leaves a principal or the machine: `ipd`, later `gatewayd`) are
  cleared for nothing by default, and refuse labelled callers. The one exception is `sshd`, cleared
  for a label only on the channel its owner authenticated (Sessions and vaults). A local model on
  the FPGA's GPU card can be cleared for a label, because the data stays on the machine.
- **User-level servers** get no exemption: a vault session cannot call an unlabelled user server.
  Something like a local model server runs one instance per label, or in the system class.
- **Defaults.** The steward mounts known-sensitive places (`~/.ssh`, credential directories) on the
  principal's labelled volume. Keys and tokens belong in `keyd`, which never releases them, so
  sessions use them without reading them.
- **Deferred:** dynamic taint-on-read (it lets accidental reads cut agents off, lets labelled data be
  planted on others, and leaks through siblings started before the read); integrity labels
  (low-integrity data such as model output cannot reach high-integrity sinks without endorsement).

## Declassification
Only a label's owner declassifies, one item at a time, after a high-stakes approval:
1. The steward **snapshots** the item when the request is submitted, and hashes the snapshot. The
   steward is unlabelled and cannot read the item, so it creates a short-lived **reader budget**
   carrying exactly the item's labels (a deadline; creating it needs the system class, which the
   steward has). The steward `call`s the reader, which reads the item and fills the steward's lend
   with the snapshot, so a labelled budget only ever answers a request, never starts one. The
   steward is class `system`, so R1 does not stop that call. There is no standing universal reader,
   and the steward itself stays unlabelled. Any other labelled read the steward needs (a labelled
   volume's `stat`) goes the same way.
2. The approval shows **all** of it. Items over a size cap, or not printable text, are refused.
3. On approval, the steward copies exactly that snapshot to an unlabelled volume.

Stated residual: text an agent wrote and a human approved can still carry a hidden message. No
system can prevent that.

## Push: input into a labelled domain
Declassification is high to low; a **push** is its mirror, low to high, and is how input enters a
labelled domain that is confined. A confined domain does not read a shared unlabelled volume: with a
colluding lower domain, that read-down is a B-to-A channel (TENETS.md, The high/low pair; the channel
table's read-down row). The push is the replacement, and it is deliberately shaped exactly like
declassification:
- **One item per push.** A push moves one item (a file, or one byte string) from an unlabelled source
  volume into a labelled domain's volume; the steward is unlabelled, so `check` lets it read the
  source. There is no standing path, no queue and no batch: one audited approval moves one item.
- **The target label's owner triggers it**, through the powerbox, with an out-of-band approval
  (CAPABILITIES.md, approvals), exactly as a declassification is approved. The confined domain cannot
  trigger a push, name the item for one, or pull one: it has no read path to the source and no
  unlabelled authority. That is what closes the channel — the lower side cannot make a push happen or
  choose its timing.
- **The steward carries it out with a short-lived writer budget carrying exactly the target label
  set**, the mirror of the reader budget. A write needs equal labels (`check`), so the steward, which
  is unlabelled, cannot write the labelled volume itself and an unlabelled writer cannot either; the
  writer budget makes the labels equal. The confined domain then reads the item in its own volume.
- **It is audited** with the request's labels, on the same path as every other steward action.
- **`check` is unchanged.** The steward declines a labelled session's mount of a shared unlabelled
  volume and offers the push instead (WP-S2); in a confined deployment no session reads down.

## Sessions and vaults
- **Normal sessions are unlabelled** (`ssh alice@box`): full network, all tools; they cannot read
  labelled volumes.
- **A vault session carries exactly one label.** `ssh alice+X@box` opens a session labelled
  `{alice-X}`, only if the authenticated person owns that label. It reads and writes `fsd:alice-X`.
  In ordinary multi-tenancy it may read, never write, unlabelled volumes, which is how data enters
  the vault; in a **confined** deployment it does not — that read-down is a B-to-A channel
  (TENETS.md, The high/low pair; question 153) — and input enters only by an audited steward push.
  It runs local tools and local models, and reaches no external sink.
- **Each SSH channel is labelled with its session's labels** (`alice@` -> none, `alice+X@` ->
  `{alice-X}`), and `sshd` applies `check` (below) to them. A vault session's output reaches only its
  own channel, which the steward opened for the label's owner. **`sshd` is the one sink cleared for
  a label**, and only for that channel: a pty session its owner authenticated, with no forwarding,
  no subsystems and no `exec`. No other sink has an owner exemption. Stated milestone 1 residual:
  that channel, every other channel and `approve@box` share one `sshd`, so a `sunset` bug reached
  from any channel reaches them all (CAPABILITIES.md, approvals).
- Vault data can appear in one other place: the approval screen, for its owner (declassification).
  A labelled session's other requests show only text the steward generates (kind, target, size);
  its free text (a reason, a note) reaches the screen only through declassification
  (CAPABILITIES.md, approvals).
- **An agent that must read secrets is started with that label** (the powerbox decides up front) and
  so reaches no external sink. "Read first, decide later" needs a new budget.

## The shared server library
Every system server that serves more than one account links one small library:
- **`admit(badge, account, labels)`**: limits on in-flight requests, open files and per-client
  state, per (account, label set). Accounts, not badges or budgets, because both of those are cheap
  to create; with the label set, because caps are counted that way (below). Within a bucket each
  badge gets a fair share, with the bucket as the ceiling, so an agent cannot lock out its sponsor,
  who shares its bucket. A connection minted through another counts in that connection's share
  while the same (account, label set) uses it, so minting badges gains nothing; one minted for a
  client by someone else (the steward, for a lease's agent) is a share of its own (question 117).
  A cap below two per bucket cannot seat a share and its sponsor, so the library refuses one. Account 0 (every system-class caller) is admitted per badge, so one daemon
  cannot fill a bucket the steward needs. The caps are sized so that every bucket at its cap fits
  the server's budget, and so that the open calls they allow sum to less than `MAX_OPEN_CALLS` with
  headroom; a parked call gets a server-side deadline. A `disconnect` (CAPABILITIES.md) frees a
  connection's state (fids, admission slots), so a dead client's quota comes back.
- **`check(caller_labels, object_labels, read | write)`**: a read needs the object's labels ⊆ the
  caller's (*no read up*); a write needs them **equal** (no write down, and no blind write up: a
  write up could truncate or remove what the writer cannot read, and `Tcreate`'s "exists" error
  would reveal names in a directory it cannot list).
- **Metadata is a read.** A 9P qid (with its version) and a `stat` are reads of their node: a walk
  into a node the caller cannot read is refused, and a directory read lists only the entries the
  caller can read. Otherwise every vault write would change what an unlabelled caller sees.
- **One connection per client.** A 9P connection is a badge; its fid table is keyed by (badge,
  account, label set), and launchers never pass their own connection on (CAPABILITIES.md).
- **Replies come from the taking thread.** A `reply` names an open call of the replying thread
  (KERNEL-SPEC.md), so an event-driven server replies from the thread that took the call. Before
  resuming work on a parked call, the library calls `serve(msg_id)`, so a crash blames that call;
  an abandoned-call notice makes it reply at once, freeing the call. A notice reaches only the
  thread that holds the call, on the endpoint the call came in on, so **every serving thread keeps
  receiving there**: a thread that parks calls and stops receiving would never be told they were
  abandoned (question 104).
- **A server that must wait parks the call, in its own buckets.** A 9P server that cannot answer yet
  (a console read with no input, a connect waiting for the network) holds the call and serves it
  later, rather than blocking or answering a default (NAMESPACES.md, Holding a call). Parking is
  charged to the **same** `Admission` the server's fids are, so a client cannot fill a server's fid
  table and its parked calls independently; a parked call has a server-side deadline and is answered
  when it expires, and an abandoned one is replied to at once. The caps leave the open-call headroom
  ([`OPEN_CALL_HEADROOM`]) under `MAX_OPEN_CALLS` so parked calls never stop the server taking new
  ones (answers 81, 82).
- **Byte quotas belong to the server.** The library carries `new_connection`'s `quota` and calls
  two hooks, one when a connection is granted and one when it is disconnected; it counts no bytes
  itself. `fsd` implements them (NAMESPACES.md); every other server leaves them empty (question
  118).
- **Handles and badges.** The library closes every handle a request carries that the protocol did
  not ask for, so a client cannot grow a server's handle table. A server never reuses a badge
  number, so a handle revoked in flight never reaches a later connection. **A server draws its
  first minted badge at random above 2^63** (from `random`) and counts up from there, refusing to
  mint rather than wrapping. Endpoints outlive servers and a server keeps no state across a restart
  (INIT.md), so a restarted server that began again at a fixed number would reissue badges its
  clients still hold, and their old handles would match its new grants; drawing the start at random
  makes that collision negligible instead of certain (question 126). Badge 0 stays the receive
  right (KERNEL-SPEC.md).

Each server's note states what its objects and state are, so that nothing a labelled caller
influences is visible to a caller without that label:
- `fsd`: state is per volume; one label set per volume (NAMESPACES.md).
- `ipd`: a sink; refuses labelled callers (IO-ARCHITECTURE.md).
- `sshd`: state is per channel; each channel labelled with its session (above).
- **steward**: applies `check` to its own records. Labelled callers can only submit requests.
  Every id it hands out (request and session ids, connection ids, and any other) is unpredictable:
  random 64-bit, keyed, never a counter, which would tell every principal how many the others made.
  Audit records carry the request's labels and are read under `check`. **Each record is signed**:
  the steward asks `keyd` to sign it under the `audit` purpose, over the domain-separated preimage
  `"redoubt.audit.v1\0" || u64_le(len) || record` whose digest `keyd` computes itself
  (VERIFIED-BOOT.md owns the domain rule), and the audit file carries the signature beside the
  record, so a record cannot be altered undetected by anything that can write the file later. The
  steward holds no key: it holds a `keyd` grant for that one purpose, and `keyd` signs nothing else
  with it. Verifying the file is an **operator tool in milestone 2**; milestone 1 produces the
  signatures and stores them (question 125). Stated limit: per-record signatures catch edits, not
  records dropped or reordered wholesale; chaining them is for milestone 2, with the verifier.
  Ending a lease is always
  accepted from the sponsor, ahead of admission. Its manifest weight is large (RESOURCES.md), not a
  priority above the queue, so it bounds the work any one request can cause and relies on its caps.

## Crash blame
A server that faults, or exits while it holds open calls (a panic), reports in its exit notice the
account and labels of the **current call of the thread that failed** (KERNEL-SPEC.md): the call it
took most recently, or the one it named with `serve` when it resumed a parked call. A `send` is
never blamed, since it is never open, and a thread doing event work (a send, an interrupt) has no
current call. `init` passes the blame to the steward. **Three crashes blamed on the same (account,
label set) within 10 minutes destroy every budget of that (account, label set)**, sessions and
leases alike (their agents go with them), and the steward refuses new sessions for it until the
window passes, recording both in the audit file. A logout alone would not stop a principal logging
straight back in, or its agent carrying on. Keyed by the label set too, for the reason caps are
(below): a vault session crashing a shared server must not end its owner's unlabelled sessions,
which would be a channel out of the vault. Bystanders are not blamed: only the current call counts,
not every call the thread holds open (a `consoled` thread holds many readers' calls), and a thread
that fails with no current call blames nobody, even when other threads of its process hold calls.
Such a crash counts only toward the restart limit and, past it, the reboot (INIT.md). Stated
limits: a request that corrupts a server which crashes later, while serving someone else, blames
the wrong account, and one that crashes an idle thread later blames nobody; the consequence is a
logout or a restart, not data loss.
Restart and reboot rules: INIT.md.

## Covert and timing channels
**Covert communication is out of scope:** the canonical statement and the reason (on one machine,
power, heat, EM and the clock couple any two domains, so no OS can prevent or bound it; the only zero
is placement) are TENETS.md, The adversary — said once there and pointed at here, not repeated.
Software closes every *intentional* flow (the channel table below); the attacker is assumed to have a
perfect clock (TENETS.md, Timing).
- **Secrets are handled by constant-time code** (`keyd`, crypto everywhere), so there is nothing
  secret-dependent to time.
- **No microarchitectural state is shared between budgets:** one budget per core, RTL partitioning,
  `keyd` on its own core (PLATFORM-FPGA.md owns these).
- **Allocation failures reveal nothing about others:** limits are carved, never overcommitted
  (RESOURCES.md).
- **Labelled budgets get their own volume**, so shared filesystem metadata carries nothing between
  labels.
- **Caps are counted per (account, label set), not per account.** A vault session and its owner's
  unlabelled session share an account; a shared cap (the steward's pending requests, the kernel's
  `WAIT_CAP` and R2's turns, `admit`'s limits, crash blame) would let the vault signal by filling
  it. System callers (account 0) are grouped by budget as well, so a busy `fsd:data` cannot fill
  `blkd`'s `WAIT_CAP` for `fsd:alice-secrets`.
- **No global counters.** Message ids are unique only within the receiving process, and PIDs are
  drawn at random, so no process sees another's traffic or creation rate in the gaps; `ps` and
  `budget` show only the caller's own (account, label set).
- **Fixed sub-budgets per label set.** At boot the steward splits each principal's top budget into
  fixed sub-budgets, one per (principal, label set) named in the manifest. A vault session's leases
  are carved from its own sub-budget, so they never change what the unlabelled side can carve.
- **Notifications and audit.** A labelled request's "approval waiting" notification reaches only
  channels whose labels ⊇ the request's, and `approve@box`; audit records are read under `check`.
- **Server CPU.** Every budget runs in the one stride queue at its manifest weight, and a server
  working for users bounds the work of one request (RESOURCES.md). Stated residual: that work is
  paid by the server's weight, not the requester's; for the steward, whose weight is large, by the
  steward.
- **Bucket slots are the one shared cap.** A server tracks at most a fixed number of
  (account, label set) buckets at once, so that its caps fit its budget; a latecomer refused for
  want of a slot learns that others hold state, between two label sets of one account as much as
  across accounts. Each server's manifest sizes that number to the (account, label set)s it
  serves, so the cap never binds in normal use; a server sized smaller states it (question 118).
- **Residual, stated:** memory bandwidth, and the shared L2 across cores until the RTL partitions it;
  shared-server caches and the disk (a vault's reads warm a cache the unlabelled session can time);
  server CPU (above).
  On QEMU and ordinary hardware, none of the microarchitectural channels are closed.

**The channel table.** Software-closed rows are the OS's claim: an intentional path here is a hard
requirement, and a leak is a design hole. The rest are out of scope (RTL-reduced or physical), with
placement as the only zero (TENETS.md, The high/low pair).

| Resource | Closed by | For a protected label set |
| --- | --- | --- |
| `call`/`send` between user budgets | software (R1) | closed |
| writes, metadata (`check`), qids, directory reads | software | closed between the writer and a reader with the writer's labels; a shared unlabelled volume read by a labelled domain is read-down, refused for a confined domain (Push) |
| sinks (`ipd`, `gatewayd`) | software | closed |
| approval rendering and notifications | software | closed |
| global counters (PIDs, message ids, `budget_usage`) | software | closed |
| read-down from a shared unlabelled volume | software (policy, `check`) | forbidden in a confined manifest; input is a steward push (Push, above) |
| a shared system-server instance (CPU, caches, quota, admission slots) | policy: one instance per domain | no sharing |
| a shared endpoint (R2's round-robin cursor) | policy: one endpoint per domain | no sharing |
| the scheduler (one stride queue) | hardware: one budget per core | RTL (reduced, not zero) |
| CPU caches, L2, memory bandwidth | hardware | RTL (reduced, not zero) |
| disk, NIC, GPU | hardware, or one instance per domain | RTL or no sharing |
| power delivery, heat, EM emission, shared clock | **out of scope** — physical substrate | placement only: do not co-locate |

## The executable security model
Before the kernel is built, the design is a Rust crate implementing **exactly** the objects, system
calls, errors and invariants of KERNEL-SPEC.md, plus the steward's policy (principals, sessions,
vaults, declassification, the powerbox), with property tests over random operation sequences.
Red-team agents from several vendors attack it by writing counterexample sequences. The kernel is
then built to the model, and the bench checks conformance against it.
