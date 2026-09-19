# Containment: labels, covert channels, shared servers, the model

Designed, not built. Owns: information-flow labels, covert channels, admission in shared servers,
the system signing key, the executable security model. Capabilities: CAPABILITIES.md.

Capabilities contain *authority*: what a process can do. Agents also need *information* containment:
what a process can leak, including through authority it legitimately holds (`gatewayd`, an allowed
`git push`). This is the confinement problem (Lampson, 1973).

## Labels (static per budget)
Decentralized information flow control, in the Flume/HiStar style, with labels fixed per budget.
- **Data carries secrecy labels**, set by its owner: files and directories in `fsd`, key material in
  `keyd`. A label is a name such as `alice-secrets`.
- **A budget's label set is fixed when the budget is created** and never changes. It is both what the
  budget may read and what it is assumed to have read. Child budgets inherit their parent's labels.
- **Reading above your labels fails.** There is no taint at run time: an unlabelled agent that
  stumbles on labelled data gets an error and keeps its network access. Nobody can taint someone
  else by planting labelled data.
- **The kernel checks one thing:** a message from budget A to budget B is delivered only if
  labels(A) is a subset of labels(B). This closes proxies and siblings.
- **Storage stamps the writer's labels** on what it writes (littlefs custom attributes), so
  anything a labelled budget writes needs the same labels to read back. Copies keep their labels.
- **Sinks check labels.** Servers whose output leaves a principal or the machine (`ipd`, `gatewayd`,
  output to other principals) refuse budgets carrying labels they are not cleared for. An external
  sink is cleared for nothing by default. A local model (on the FPGA's GPU card) can be a sink
  cleared for a label, because the data stays on the machine.
- **Only the owner declassifies**, per item, as an audited, high-stakes powerbox approval. There is
  no standing clearance for a whole label.
- **Defaults.** The steward labels known-sensitive places by default: `~/.ssh`, credential files,
  `.env` files, anything `keyd` manages. Labels protect only labelled data; secrets belong in `keyd`.
- **In practice:** an agent that must read secrets is *started* with that label (the powerbox
  decides up front) and so cannot reach external sinks. "Read first, decide later" needs a new budget.
- **Deferred:** dynamic taint-on-read (rejected for now: accidental reads cut agents off, labelled
  data can be planted on others, and siblings started before the read leak). Integrity labels (the
  dual: low-integrity data such as model output cannot reach high-integrity sinks such as system
  configuration without endorsement). Build either when needed.

Kernel cost: an immutable label set per budget and a subset check per cross-budget message.

## Covert channels
They never reach zero. The goal: low bandwidth, stated, audited; the rest is handled in hardware
(PLATFORM-FPGA.md).

| Channel | Fix |
| --- | --- |
| Shared store: add a blob, probe for it | Adding is always charged in full and answered the same way, whether or not the blob exists; a principal sees only its own profile's closure |
| Budget allocation failures revealing others' use | No overcommit anywhere (RESOURCES.md): success depends only on your own budget |
| Shared fs metadata (free space, 9P versions) | Budgets that hold secret labels get their own volume |
| High-resolution time | User mode cannot read `time`; user processes get 1 ms time from the kernel (RESOURCES.md) |
| CPU and cache contention | Fixed stride shares per budget; all hardware threads of a core run one budget; the rest is RTL |

## Shared servers: admission and crashes
- **Limits are per budget, not per badge.** Badges are free to mint, so per-badge limits multiply
  away. The shared server library limits each caller budget's queue slots, in-flight requests, open
  files and per-client state, using the budget id the kernel attaches to every message.
- **Crash quarantine.** If a server crashes while handling a request, `init` quarantines the caller's
  budget (drops its connection to that server, notifies its sponsor) before counting the crash
  toward a reboot. One principal cannot reboot the machine by crashing a shared server. Attribution
  can be wrong; quarantine is reversible, a reboot loop is not.
- The bench gets a flooding test and a deliberate-crash test.

## System signing key
One key owning every machine is a single point of failure. System packages need **M-of-N
signatures**; builds are **reproducible** and confirmed by independent builders; updates have
**rollback protection**. (Today's loader checks one development key: VERIFIED-BOOT.md.)

## The executable security model
The design is what must hold, so it gets its own attack surface before the kernel does: a small
Rust crate modelling handles, stamps, minting, revocation, budgets, labels and the powerbox, with
property tests over random operation sequences. Red-team agents from several vendors attack it by
writing counterexample sequences. Invariants:
- a principal never holds a capability not derived from its grants;
- after budget B is destroyed, no handle stamped with B or a descendant exists;
- data labelled L never reaches a budget or sink not cleared for L without the owner declassifying;
- budget usage never exceeds its limit; children's limits never exceed the parent's; destroying a
  budget returns everything.
The kernel is built to the model, and the bench checks conformance against it.
