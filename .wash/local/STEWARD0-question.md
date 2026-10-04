# STEWARD0: the steward's policy core

The owner's ask (2026-10-02): the steward worries them; it must be expressed elegantly enough to
rule out classes of bugs. Decided with the owner: the policy is **one pure state machine in Rust**
(`no_std`, no `unsafe`), in a crate of its own, shared by the steward server and the model.

## The shape to design

1. **State, keyed by domain.** The store is partitioned by `(account, label set)`. Functions see
   one domain; the only functions that take two are R34's three control-plane edges (the
   request/approval path, the one-item reader and writer budgets, lease supervision). The type
   system, not review, is what keeps R37. Say what the domain key is exactly (account 0, the
   budget for it; system-class callers), what lives per domain (sessions, leases, grants, pending
   requests, blame, the audit records that domain may read), and what lives in no domain.
2. **Machines, as plain enums.** One small machine per object: session, lease (the state diagram on
   steward.md), approval request (typestate: a request that was never rendered cannot be approved),
   the reader and writer budgets of declassification and push. Exhaustive `match`; no state-machine
   library or proc-macro.
3. **One decision function.** `decide(event, &state) -> (state', effects)` with effects as data
   (kernel calls the embedder makes, audit records, replies), no I/O in the core. Audit records are
   derived from the domain key, so none can lack its labels (IPC2's finding).
4. **Generated shape, hand-written policy.** Transition tables (state, event, guard name, next
   state, effect names) are the source for the Rust dispatch, the Mermaid state diagrams on the
   page and the Elixir reference's clause skeletons, generated in `tools/` as the wire generator
   does, with a drift check. Guards (R37, R7/R41 narrowing, R42 one item, R40 blame) and effects
   are hand-written in the core, each with a mutation the model's families must catch.
5. **Two embedders.** The steward server binds effects to the client library; the model binds the
   same crate to the model kernel, replacing `model/src/steward.rs`. The existing families (P10
   and the rest) and mutations attack the shipped core.
6. **The Elixir reference.** `decide/2` as multi-clause functions over `defstruct` state, on
   beamlet on the host, in the bench's host tests as a differential oracle: the same event traces,
   equal states and audit records. It is a test oracle in the sense tenet 3 allows on the build
   host; it is never authoritative and never runs on the box.

## What the design must settle

- Whether the page's rules R37-R42 (and R26, R39, R33 as they touch the steward) are complete
  enough to write the tables from, and what is missing.
- The event set: logins, requests, approvals and denials, declassify and push, lease ends and
  sponsor ends, crashes and blame, restarts.
- The boundary between the core and the server: what the server decides on its own (nothing, is
  the aim) and what the core assumes of its embedder.
- What of `model/src/steward.rs` and `policy.rs` survives, and how the two-world families drive the
  shared crate.
- The order of packages: STEWARD0 (the crate, the model embedder, the Elixir oracle, the
  generator), then the steward step binds it. What STEWARD0 needs (INIT chain? the client library
  is merged).
- Any owner choice: say so with a recommendation.

Write the design on `docs/servers/steward.md` (a section for the core, before the mechanisms) and
the brief at `/home/mcloonan/redoubt/.wash/local/STEWARD0-implementer.md`.
