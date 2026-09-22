# Architect notes

The architect's own durable memory, not the design record. `QUESTIONS.md` and `ANSWERS.md`
are the record; the design notes are the truth. This file exists so a *new* architect
session starts warm instead of re-deriving what earlier sessions learned: what has been
decided, what binds, and the traps a question is likely to walk into.

Terse, append-only. One line per fact. Newest at the bottom of each section. Anything that
is actually a design decision belongs in the notes, not here — link it, do not restate it.

## Decisions that bind

- The **label set, not the capability set, is the isolation unit** (answer 150). Capabilities
  bound authority; labels bound information flow. Equal label sets = one trust domain.
- **Covert communication between co-located budgets is out of scope** (answer 151), like
  physical attacks and microarchitectural side channels. Claim is exactly "zero intentional
  paths"; placement is the only zero. Canonical statement: TENETS.md, The adversary.
- **`confined` is a per-boot manifest boolean** (answer 152); `init` compares label sets and
  refuses the boot when differing sets share a server, volume, endpoint, `ipd`/`netd`
  instance or core, or a labelled domain reads a shared unlabelled volume.
- **The steward push** (answer 153) is the mirror of declassification: one item, owner
  triggered, out-of-band approval, short-lived writer budget with exactly the target label
  set. `check` unchanged.
- **`tenets` outrank everything; `KERNEL-SPEC.md` alone owns the ABI.** A design change needs
  a HISTORY.md entry.

## Traps (things a question will otherwise re-learn)

- **`RESERVED_TYPES` in the wire generator is correct, not over-broad.** Generated types
  carry `#[derive(..., Copy, ...)]`, so a message named `copy` would produce `struct Copy`
  and shadow the derive. A message name that collides with a derived trait must be renamed,
  not the reservation narrowed. (Surfaced by WP-W3a.)
- **A docs table edit can break generation without any code changing.** The `fsd` typed
  tables were added to NAMESPACES.md with a `<!-- wire: fsd ninep -->` marker the generator
  did not yet understand, so `cargo test -p redoubt-wire-gen` was red on `redoubt` and no
  package owned it, because no bench case runs the generator.
- **`redoubt-rt`'s records are already backed**: `Record<const N>(pub [u64; N])` is
  `Record([0; N])`, a written stack array. An "untouched page is `InvalidArgument`"
  assertion is kernel behaviour and is not reachable through the `HostKernel` fake.
  (Surfaced by WP-W3b; see its question.)
- **Long-lived roles must be bounded.** An architect or implementer session that reads
  without a write deadline will spend its whole budget researching and return nothing. Warm
  context (this file, `resume`) is what makes a later question cheap.