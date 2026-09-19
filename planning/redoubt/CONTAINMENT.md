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
- **Between user budgets, calls and sends need equal label sets** (the kernel checks). Upward flow
  goes through a server: writing into a labelled volume is allowed (no write down, below).
- **System servers are exempt from the kernel check and enforce labels themselves**, using the
  caller's label set the kernel attaches to every message (the shared server library, below).
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
1. The steward **snapshots** the item when the request is submitted, and hashes the snapshot.
2. The approval shows **all** of it. Items over a size cap, or not printable text, are refused.
3. On approval, the steward copies exactly that snapshot to an unlabelled volume.

Stated residual: text an agent wrote and a human approved can still carry a hidden message. No
system can prevent that.

## Sessions and vaults
- **Normal sessions are unlabelled** (`ssh alice@box`): full network, all tools; they cannot read
  labelled volumes.
- **A vault session carries exactly one label.** `ssh alice+X@box` opens a session labelled
  `{alice-X}`, only if the authenticated person owns that label. It reads and writes `fsd:alice-X`,
  runs local tools and local models, and reaches no external sink.
- **Each SSH channel is labelled with its session's labels** (`alice@` -> none, `alice+X@` ->
  `{alice-X}`), and `sshd` applies ordinary no-write-down. A vault session's output reaches only its
  own channel, which the steward opened for the label's owner. There is no owner exemption at any sink.
- Vault data can appear in one other place: the approval screen, for its owner (declassification).
- **An agent that must read secrets is started with that label** (the powerbox decides up front) and
  so reaches no external sink. "Read first, decide later" needs a new budget.

## The shared server library
Every system server that serves more than one account links one small library of two functions:
- **`admit(account)`**: per-account limits on in-flight requests, open files and per-client state.
  Accounts, not badges or budgets, because both of those are cheap to create.
- **`check(caller_labels, object_labels, read | write)`**: *no read up* (read only if the object's
  labels ⊆ the caller's) and *no write down* (write only if the caller's labels ⊆ the object's).

Each server's note states what its objects and state are, so that nothing a labelled caller
influences is visible to a caller without that label:
- `fsd`: state is per volume; one label set per volume (NAMESPACES.md).
- `ipd`: a sink; refuses labelled callers (IO-ARCHITECTURE.md).
- `sshd`: state is per channel; each channel labelled with its session (above).
- **steward**: applies no-write-down to its own records. Labelled callers can only submit requests.
  Every id it hands out (request and session ids, and any other) is unpredictable: random 64-bit,
  keyed, never a counter, which would tell every principal how many the others made. The cap on
  pending requests is per (account, label set).

## Crash blame
A server that faults reports, in its exit notice, the account of the message the faulting thread was
serving (KERNEL-SPEC.md). `init` passes it to the steward. **Three crashes blamed on the same account
within 10 minutes log that account out:** the steward destroys its session budgets (its agents go
with them) and records it in the audit file. Bystanders are never blamed: only the message being
served counts. Stated limit: a request that corrupts a server which crashes later, while serving
someone else, blames the wrong account; the consequence is a logout, not data loss. Restart and
reboot rules: INIT.md.

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
  `WAIT_CAP` and R2's turns) would let the vault signal by filling it.
- **Residual, stated:** memory bandwidth, and the shared L2 across cores until the RTL partitions it.
  On QEMU and ordinary hardware, none of the microarchitectural channels are closed.

## The executable security model
Before the kernel is built, the design is a Rust crate implementing **exactly** the objects, system
calls, errors and invariants of KERNEL-SPEC.md, plus the steward's policy (principals, sessions,
vaults, declassification, the powerbox), with property tests over random operation sequences.
Red-team agents from several vendors attack it by writing counterexample sequences. The kernel is
then built to the model, and the bench checks conformance against it.
