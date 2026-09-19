# Containment: labels, shared servers, channels, the model

Designed, not built. Owns: information-flow labels and their policy, sessions and vaults,
declassification, the shared server library, crash blame, covert and timing channels, the executable
security model. The kernel's rules: KERNEL-SPEC.md (R1, R6, `budget_usage`). Capabilities and IPC: CAPABILITIES.md.

Capabilities contain *authority*: what a process can do. Agents also need *information* containment:
what a process can leak, including through authority it legitimately holds (an allowed `git push`).
This is the confinement problem (Lampson, 1973).

## Labels
Decentralized information flow control in the Flume/HiStar style, with labels fixed per budget.
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
  a label from outside it: every write needs equal labels (`check`, below).
- **System servers are exempt from the kernel check and enforce labels themselves**, using the
  caller's label set the kernel attaches to every message (the shared server library, below).
- **A receive right is never handed across label sets.** R1 compares a sender with the endpoint's
  owner, not with whichever process receives (KERNEL-SPEC.md, I7), so handing a badge-0 handle to a
  budget with other labels would deliver messages nobody compared with it. Handing one out is
  delegation; a system server that hands one across label sets is buggy, not the kernel.
- **Budget observation obeys labels.** Reading a budget's usage, or receiving its exit notices, needs
  the reader's labels ⊇ the target's, unless the reader is class `system` (like the message check:
  `init` and the steward must see labelled processes exit). A parent's usage counts its children's
  limits, never their live usage, so a labelled child cannot signal through its parent's counters.
- **Sinks** (servers whose output leaves a principal or the machine: `ipd`, later `gatewayd`) are
  cleared for nothing by default, and refuse labelled callers. A local model on the FPGA's GPU card
  can be cleared for a label, because the data stays on the machine.
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
   steward has), which reads the item and returns the snapshot to the steward. The steward is class
   `system`, so R1 does not stop that message. There is no standing universal reader, and the
   steward itself stays unlabelled. Any other labelled read the steward needs (a labelled volume's
   `stat`) goes the same way.
2. The approval shows **all** of it. Items over a size cap, or not printable text, are refused.
3. On approval, the steward copies exactly that snapshot to an unlabelled volume.

Stated residual: text an agent wrote and a human approved can still carry a hidden message. No
system can prevent that.

## Sessions and vaults
- **Normal sessions are unlabelled** (`ssh alice@box`): full network, all tools; they cannot read
  labelled volumes.
- **A vault session carries exactly one label.** `ssh alice+X@box` opens a session labelled
  `{alice-X}`, only if the authenticated person owns that label. It reads and writes `fsd:alice-X`,
  may read (never write) unlabelled volumes, which is how data enters the vault, runs local tools and
  local models, and reaches no external sink.
- **Each SSH channel is labelled with its session's labels** (`alice@` -> none, `alice+X@` ->
  `{alice-X}`), and `sshd` applies `check` (below) to them. A vault session's output reaches only its
  own channel, which the steward opened for the label's owner. There is no owner exemption at any sink.
- Vault data can appear in one other place: the approval screen, for its owner (declassification).
  A labelled session's other requests show only text the steward generates (kind, target, size);
  its free text (a reason, a note) reaches the screen only through declassification
  (CAPABILITIES.md, approvals).
- **An agent that must read secrets is started with that label** (the powerbox decides up front) and
  so reaches no external sink. "Read first, decide later" needs a new budget.

## The shared server library
Every system server that serves more than one account links one small library of two functions:
- **`admit(badge, account, labels)`**: limits on in-flight requests, open files and per-client
  state, per (account, label set). Accounts, not badges or budgets, because both of those are cheap
  to create; with the label set, because caps are counted that way (below). Account 0 (every
  system-class caller) is admitted per badge, so one daemon cannot fill a bucket the steward needs.
  When the kernel's badge notice says a badge's last handle is gone (KERNEL-SPEC.md, Messages), the
  library frees that badge's state (fids, admission slots), so a dead client's quota comes back.
- **`check(caller_labels, object_labels, read | write)`**: a read needs the object's labels ⊆ the
  caller's (*no read up*); a write needs them **equal** (no write down, and no blind write up: a
  write up could truncate or remove what the writer cannot read, and `Tcreate`'s "exists" error
  would reveal names in a directory it cannot list).
- **Metadata is a read.** A 9P qid (with its version) and a `stat` are reads of their node: a walk
  into a node the caller cannot read is refused, and a directory read lists only the entries the
  caller can read. Otherwise every vault write would change what an unlabelled caller sees.
- **One connection per client.** A 9P connection is a badge; its fid table is keyed by (badge,
  account, label set), and launchers never pass their own connection on (CAPABILITIES.md).

Each server's note states what its objects and state are, so that nothing a labelled caller
influences is visible to a caller without that label:
- `fsd`: state is per volume; one label set per volume (NAMESPACES.md).
- `ipd`: a sink; refuses labelled callers (IO-ARCHITECTURE.md).
- `sshd`: state is per channel; each channel labelled with its session (above).
- **steward**: applies `check` to its own records. Labelled callers can only submit requests.
  Every id it hands out (request and session ids, and any other) is unpredictable: random 64-bit,
  keyed, never a counter, which would tell every principal how many the others made. The cap on
  pending requests is per (account, label set).

## Crash blame
A server that faults, or exits while it holds open calls (a panic), reports in its exit notice the
account and labels of the **most recently taken call still open on the thread that failed**
(KERNEL-SPEC.md, the serving account); a `send` is never blamed, since it is never open. `init`
passes them to the steward. **Three crashes blamed on the same (account, label set) within 10
minutes log out that account's sessions with that label set:** the steward destroys those session
budgets (their agents go with them) and records it in the audit file. Keyed by the label set too,
for the reason caps are (below): a vault session crashing a shared server must not log out its
owner's unlabelled sessions, which would be a channel out of the vault. Bystanders are not blamed:
only the one most recent call counts, not every call the thread holds open (a `consoled` thread
holds many readers' calls). A thread that fails holding no open call blames nobody, even when other
threads of its process hold calls: falling back to one of theirs would blame a bystander. Such a
crash counts only toward the restart limit and, past it, the reboot (INIT.md). Stated limits: a
request that corrupts a server which crashes later, while serving someone else, blames the wrong
account, and one that crashes an idle thread later blames nobody; the consequence is a logout or a
restart, not data loss.
Restart and reboot rules: INIT.md.

## Covert and timing channels
They never reach zero; the goal is low bandwidth, stated, audited. The attacker is assumed to have a
perfect clock (TENETS.md).
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
  it.
- **Residual, stated:** memory bandwidth, and the shared L2 across cores until the RTL partitions it.
  On QEMU and ordinary hardware, none of the microarchitectural channels are closed.

## The executable security model
Before the kernel is built, the design is a Rust crate implementing **exactly** the objects, system
calls, errors and invariants of KERNEL-SPEC.md, plus the steward's policy (principals, sessions,
vaults, declassification, the powerbox), with property tests over random operation sequences.
Red-team agents from several vendors attack it by writing counterexample sequences. The kernel is
then built to the model, and the bench checks conformance against it.
