# Containment: labels, shared servers, channels, the model

Designed, not built. Owns: information-flow labels, sessions and vaults, label checks and admission
in shared servers, crash attribution, covert and timing channels, the executable security model.
Capabilities and IPC: CAPABILITIES.md.

Capabilities contain *authority*: what a process can do. Agents also need *information* containment:
what a process can leak, including through authority it legitimately holds (an allowed `git push`).
This is the confinement problem (Lampson, 1973).

## Labels
Decentralized information flow control in the Flume/HiStar style, with labels fixed per budget.
- **A label is a name** such as `alice-secrets`, owned by a principal.
- **Labels live on budgets and volumes.** A budget's label set is fixed when it is created and never
  changes; it is both what the budget may read and what it is assumed to have read. Child budgets
  inherit their parent's labels. **Only the steward creates a budget with more labels than its
  parent**, after a high-stakes approval. A volume (`fsd:alice-secrets`) has one fixed label set.
- **Reading above your labels fails.** There is no taint at run time: an unlabelled agent that
  stumbles on labelled data gets an error and keeps its network. Nobody can taint someone else.
- **Between user budgets, the kernel checks:** a `call` (anything with a reply, including lends)
  needs equal label sets; a one-way `send` needs labels(sender) ⊆ labels(receiver).
- **System servers are exempt from the kernel check and enforce labels themselves.** They talk to
  every budget, and they are TCB. The kernel attaches each caller's label set, budget id and
  principal id; one shared server library applies, per request:
  - *no read up:* read an object only if its labels ⊆ the caller's;
  - *no write down:* write an object only if the caller's labels ⊆ its labels;
  - *no leaky state:* anything a labelled caller can influence (counters, spend meters, versions,
    timestamps) is visible only to callers whose labels ⊇ it.
- **Budget handles obey the same rule.** Reading a budget's usage, or receiving its exit messages,
  needs the reader's labels ⊇ the target's; otherwise nothing is returned. Destroying a budget is
  always allowed.
- **Sinks** (servers whose output leaves a principal or the machine: `ipd`, later `gatewayd`) are
  cleared for nothing by default. A local model on the FPGA's GPU card can be a sink cleared for a
  label, because the data stays on the machine.
- **Declassification** is per item: after a high-stakes approval that shows the item's content, the
  steward copies it from the labelled volume to an unlabelled one.
- **Defaults.** The steward mounts known-sensitive places (`~/.ssh`, credential directories) on the
  principal's labelled volume. Keys and tokens belong in `keyd`, which never releases them, so
  sessions use them without reading them.
- **Deferred:** dynamic taint-on-read (accidental reads cut agents off, labelled data can be planted
  on others, siblings started before the read leak); integrity labels (low-integrity data such as
  model output cannot reach high-integrity sinks without endorsement). Build either when needed.

Kernel cost: an immutable label set per budget, attached to every message, and a subset or equality
check on calls and sends between user budgets.

## Sessions and vaults
- **Normal sessions are unlabelled** (`ssh alice@box`): full network, all tools; they cannot read
  labelled volumes.
- **Vault sessions carry the principal's labels** (`ssh alice+secrets@box`): they read and write the
  labelled volume, run local tools and local models, and reach no external sink.
- **Terminal rule:** a terminal channel is cleared for exactly the labels owned by the principal
  authenticated on that channel (`sshd` knows who that is). A vault session's output can therefore
  reach only its owner's own screen. There is no owner exemption at any other sink.
- **An agent that must read secrets is started with that label** (the powerbox decides up front) and
  so reaches no external sink. "Read first, decide later" needs a new budget.

## Shared servers: admission and crashes
- **Limits are per principal.** Badges and sub-budgets are cheap to create, so per-badge or
  per-budget limits multiply away. The shared server library limits in-flight requests, open files
  and per-client state per principal id. Creating a budget costs pages (RESOURCES.md).
- **Crash attribution needs repetition.** A principal is blamed only if it had a request in flight in
  3 consecutive crashes of the same server (the kernel records in-flight callers when a server dies).
  One crash blames nobody.
- **Consequence:** `init` tells the steward, which destroys that principal's session budgets (its
  agents go with them) and records it in the audit file. The principal may log in again. A reboot
  happens only if crashes continue with no principal consistently present (INIT.md).
- The bench gets a flooding test and a deliberate-crash test.

## Covert and timing channels
They never reach zero; the goal is low bandwidth, stated, audited. The attacker is assumed to have a
perfect clock (TENETS.md).
- **Secrets are handled by constant-time code** (`keyd`, crypto everywhere), so there is nothing
  secret-dependent to time.
- **No microarchitectural state is shared between budgets:** all hardware threads of a core run one
  budget or idle; the RTL partitions or flushes caches and TLBs on a budget switch; `keyd` gets its
  own core once there is SMP (PLATFORM-FPGA.md).
- **Allocation failures reveal nothing** about others: no overcommit (RESOURCES.md).
- **Labelled budgets get their own volume**, so shared filesystem metadata carries nothing between labels.
- **Residual, stated:** memory bandwidth, and the shared L2 across cores until the RTL partitions it.
  On QEMU and ordinary hardware, none of the microarchitectural channels are closed.

## The executable security model
Before the kernel is built, the design is a Rust crate: handles, stamps, minting, revocation,
budgets, labels and volumes, calls and sends, and the powerbox, with property tests over random
operation sequences. Red-team agents from several vendors attack it by writing counterexample
sequences. Invariants:
- a principal never holds a capability not derived from its grants;
- after budget B is destroyed, no handle stamped with B or a descendant exists;
- data labelled L never reaches a budget, volume or sink not cleared for L without the owner
  declassifying;
- budget usage never exceeds its limit; children's limits never exceed the parent's; destroying a
  budget returns everything.
The kernel is built to the model, and the bench checks conformance against it.
