# Assurance

## Idea

Raise the security claims from "attacked and argued" toward "proven where it matters", without a
seL4-scale proof of the whole kernel. Two tracks, each covering what the other cannot:

- **Formal methods, applied where they are cheap and the payoff is high:** verified components
  taken whole, the steward's protocols model-checked, the parsers of hostile input proved free of
  panics, the kernel's core invariants proved in Verus, and the gateware checked against the ISA
  and the DMA boundary.
- **Adversarial agent review, run as a game:** frontier models given the whole stack and set to
  capture planted flags, from each starting position the threat model names.

A proof shows the code meets its spec; it cannot show the spec is the one wanted, or that its
assumptions hold. Adversaries attack exactly that, but cannot show the absence of a bug. Findings
from the games say where an invariant is missing; invariants the games never break are where a
proof is cheapest to add.

## Why it is not a goal

Every milestone's claims are carried by the attack suite and stated arguments
([the security register](../SECURITY.md)), and none needs a proof. The work below adds assurance
to claims already made; it adds no claim.

## What it would need

### Verified components, taken whole

The cheapest wins: reuse what is already proven instead of proving it again.

- **Verified cryptography that ships as Rust:** HACL\* through `libcrux` (X25519,
  ChaCha20-Poly1305, SHA-2, ML-KEM) and fiat-crypto's field arithmetic, for `keyd` and any TLS
  or channel server. They are proven functionally correct and memory safe, and constant-time by
  construction ([R45 (constant-time signing)](../servers/keyd.md#r45-constant-time-signing)).
- **Verus's `vstd`, and the RustBelt and Iris results,** as the foundation the kernel proofs
  below build on, rather than our own axioms.
- **seL4's invariants as a checklist:** its capability derivation tree, untyped memory and
  no-aliasing invariants map closely onto Redoubt's frame ownership and handles. They are written
  down against our design before any proof, so a missing one is found by reading, not by attack.

### Model-checking the steward's protocols

Leases, crossings and sessions are already state machines with traces
(`libs/steward/trace`). A TLA+ (or similar) model of them, checked exhaustively at small bounds,
shows that **no label crosses a boundary under any interleaving** except through an approved
crossing, and that a lease's end leaves nothing running. The recorded traces are checked to be
behaviours of the model, so the model cannot drift from the code unnoticed.

### Kani on the parsers of hostile input

Bounded proofs, written like unit tests, that the code facing hostile bytes cannot panic, overflow
or index out of bounds for any input up to a size: the 9P server (`libs/rt/src/server/ninep.rs`,
already fuzzed), the virtio descriptor handling in the drivers and in the host's backend, and
`gatewayd`'s HTTP handling. The existing fuzz targets become Kani harnesses where they fit.

### Verus on the kernel's core invariants

Pre- and postconditions and loop invariants on the kernel's own state, discharged by an SMT
solver, starting with the memory manager since
[R11 (memory)](../kernel/memory.md#r11-memory) is already stated as an invariant:

1. **Frame ownership:** every frame has at most one owner, and a frame is zeroed before any
   translation to it exists.
2. **Handle tables:** a handle names only an object its process was given.
3. **ASID and PID tagging:** a translation tagged with a PID is never used by another process,
   across reuse of the PID.
4. **The IPC state machine:** a message is delivered at most once, to the endpoint it was sent to.

Each is an inductive invariant: it holds at boot and every system call preserves it. Agents can
write the proofs, since the checker, not the author, is trusted; the owner reviews the specs.

### The gateware

- **riscv-formal with SymbiYosys** checks the VexiiRiscv core against the ISA specification. It
  needs the core's RVFI trace port.
- **The DMA confinement decoder** ([the FPGA platform](fpga-platform.md#what-the-card-guarantees))
  is proved never to route a master other than the cores into main memory, and never outside the
  windows the kernel programmed. It is small, and the property is exactly what the card claims.

### Adversarial agent review

Frontier models from more than one vendor, given every line of the stack (kernel, servers,
beamlet, the host's backend, the gateware's source) and told to break out. Run as a game, so the
result is a score and not an opinion:

- **Flags, scored mechanically:** a secret under another principal's label read; a protected file
  written; a send attempted past `gatewayd`'s policy; a DMA write outside its window; something
  persisted across a reboot; a capability used that was never granted.
- **Starting positions from the threat model,** each its own campaign: an unprivileged agent in a
  session; a compromised driver server; a malicious virtio device; a malicious host backend; a
  compromised `gatewayd`.
- **Many runs in parallel on QEMU,** whitebox and blackbox, with different models and prompts;
  findings deduplicated.
- **Every finding becomes an attack case** in the bench before its fix lands, so the suite grows
  from the games.
- **The discovery curve is tracked:** valid new findings per unit of attack effort. When fresh,
  stronger attackers stop finding anything, that is evidence, never proof.
- **The red team is itself contained:** it writes working exploits, so it runs sandboxed, with no
  network, and logged.

Limits, stated: models that built the system share blind spots with models attacking it, so the
attackers are mixed by vendor and generation, with people among them; timing and electrical
channels need a laboratory, not a reading of the code; and a cooperative red-team model is a lower
bound on a patient, adversarial one.

**Attack cases:** none new of their own; this page's work produces them. Each proof that holds
names its rule in [the security register](../SECURITY.md) beside the attack case that tests it.

**Undecided:**
- TLA+ or a Rust-native model checker for the steward, and whether the model is generated from
  `libs/steward/tables`.
- Whether Verus's proofs live beside the kernel's code or in a separate crate checked by the
  bench.
- How far past the four invariants the kernel proofs go, toward a full refinement of an abstract
  specification.
