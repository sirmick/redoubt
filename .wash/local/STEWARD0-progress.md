# STEWARD0 progress (handoff after commit 3)

Branch `wp-steward0`, worktree `/home/mcloonan/redoubt/.worktrees/steward0`, rebased on main
`a61477f7f` (the Architect's ruling on steward.md). Clean tree at `ed8347764`.

## Commits so far (oldest first)

1. `ba548c1f6` steward: the policy core's transition tables, one per machine (ruling applied)
   + fixup `8fbecba22` (crossing's `cross_item` split into `read_item`/`write_item`: no effect
   branches on kind).
2. `63ebb7841` steward: the tables generate the policy's dispatch, diagrams and clause skeletons
   + fixup `13b31066f` (vocabulary for the split, regenerated files). Also adds
   `libs/steward/src/gen/**` to `rustfmt.toml`'s ignore list (as wire's proto; told the
   orchestrator; outside the brief's owned paths).
3. `d0f42ab47` steward: the policy core (crate `redoubt-steward`), 28 tests in
   `libs/steward/tests/core.rs`.
4. `ed8347764` testbench: `tests/steward-host-tests.toml` (PASS, 0.6s). The brief puts the bench
   case in deliverable 5; reorder/fold at the end if wanted.

Fold the two fixups before the merge: `GIT_SEQUENCE_EDITOR=: git rebase -i --autosquash a61477f7f`.

## Gates run so far (all via .wash/local/in-dev, exit 0)

- `cargo test -p redoubt-steward-gen` (10 passed), `cargo test -p redoubt-steward` (28 passed)
- `cargo testbench steward-host-tests` PASS
- `cargo +nightly fmt --all --check` clean; `cargo clippy -p redoubt-steward -p redoubt-steward-gen --all-targets` clean
- `cargo build -p redoubt-steward --target riscv32imac-unknown-none-elf` and `riscv64gc-unknown-none-elf` OK
- Not yet: the whole bench, doccheck, size budget, model families.

## Table changes after the Architect's ruling (report these)

- Three `unreachable` rows the generator's coverage check demanded: session and lease
  `Running | Done, Failed`, request `Frozen, Rendered | Done, Failed` (no batch outstanding).
- `cross_item` split into `read_item` (read row) and `write_item` (push write row).
- Blank line before `<!-- ANCHOR_END: table -->` (the table ends at a blank line).

## The core's shape (read `libs/steward/src/lib.rs` first)

- `Store::boot(&Manifest, Policy) -> Option<(Store, Vec<Carve>)>`; `decide(&mut Store, Event) -> Effects`.
- `Event { now, random: [u64; 8], reply: ReplyTo, kind: EventKind }`. Roles by variant; sessions
  and leases name themselves by badge; `Exited`/`Done` name an `Object {domain, kind, id}`.
- `Effects { outputs, batches, exit }`. One `Batch { owner: Object, steps }` per object that waits.
  The embedder runs steps in order, stops at first failure, reports `Done { object, result:
  Result<Vec<Produced>, StepFailed> }`. A destroy of a token never made must fail (destroy_partial
  relies on it). Tokens are `(owner Object, slot)`: slots 0 budget, 1 scope / read, 2 process,
  3 steward connection, 4.. shared servers.
- `Policy` (generated, `libs/steward/src/gen/mod.rs`): every guard (`fn(&Cx) -> Result<(),
  Refusal>`), effect (`fn(&mut Cx)`) and `audit_visible`. The model makes broken ones from the
  pub `Cx` API plus pub helpers: `effects::carve_lease_in`, `effects::show`,
  `render::screen(.., &Rules)` with `Rules::SHIPPED`, `guards::{session, lease, request}`.
- Domains private to the store; `Store::pair` needs `edges::TwoDomains`, only edges makes it.
- `inspect`: read-only (`domains`, `domain`, `index`, `fixed`, `policy`, `exited`, `audit_view`).
- Manifest: `PrincipalSpec { name, account, login_keys, approval_keys, owned, label_sets, top }`,
  `Sizes`, `servers`, `keyd_keys`. Boot refuses a key in two roles or held by keyd, account 0,
  duplicates. keyd's keys are fixed (no KeydAdd any more).

## Next: deliverable 4, the model's embedder (not started)

- `model/Cargo.toml`: depend on `redoubt-steward` (path); rewrite its "No dependencies" comment.
- `model/src/steward.rs` becomes the binding: keep `Proc`, `run`, the fsd stand-in server
  (`serve`, `hold`, `reply`, `crash_server`, `start_server` incl. `PolicyServerHoldsSystemBudget`),
  volumes, `AuditKeyd`/`AuditSignature` (sign every `Output::Audit`), the keyed mixer (random
  words; `PolicySequentialIds` makes them a counter). Delete login/submit/approve/... policy.
  Map steps to syscalls: CreateBudget (parent: `Parent::Sub(domain)` -> the carve's handle,
  `Budget(token)`, `Users` = steward slot 1), CreateScope (zero limits), Connect = Mint from srv
  (Shared(0)) or a steward endpoint (Steward) narrowed to the scope's handle, Launch =
  ProcessCreate + ProcessStart with the connections, DestroyBudget, Read/Write on the volumes
  (record the through-budget's kernel labels for P6/P11). Exit notices: the server's -> `Blame`
  event (account 0 blames nobody); a session's/lease's process -> `Exited { object }`.
  `PolicyEndLeaseAdmitted`: the embedder's admission ignores `EventKind::ahead()`.
  `PolicyWriteUp`: the volume's own write check (session item writes) stays in the embedder.
- `ConnectionShares`, `AdmissionBucket` move unchanged to new `model/src/serving.rs`.
- `model/src/policy.rs`: families P1-P16 drive the embedder; checks read `inspect`. Approvals
  must open a channel and `Pending` (render) before `Approve`/`Deny`. Notices are something a
  session observes (P10 catches `PolicyNotifyLabelledToAll`). P6 compares what was written to the
  unlabelled volume with the snapshot. Test manifest: give bob label set {7} he does not own (C1).
- `model/src/mutation.rs`: retire `PolicyBlamePerAccount`, `PolicyCapPerAccount`,
  `PolicyCarveFromUnlabelled`, `PolicyNarrowToSessionBudget`; add `PolicyApproveOtherChannel`,
  `PolicyApproverExceeds`, `PolicyEndLeaseFromVault`, `PolicyApproveWithLoginKey`,
  `PolicyLabelledStartsAgent`, `PolicyDeclassifyUnfit`, `PolicyDeclassifyFromUnlabelled`,
  `PolicyNotifyLabelledToAll`. `PolicyLoginWithKeydKey`'s broken guard accepts keyd's keys as
  login keys (the manifest has no overlap). `PolicyDeclassifyLive`'s broken `copy_out` reads the
  item live (`Step::Read` then `Write { bytes: Bytes::Read(token) }`). Update `is_policy` users
  and the rule map. `mutations_are_caught` must catch every one left.
- Hotspot: K16 touches `model/src/spec.rs` only; do not edit it.

## Then deliverable 5

- `docs/servers/steward.md`: include each `libs/steward/tables/<m>.md:table` and its
  `<m>.mermaid.md` (replace the hand-drawn lease diagram); "The policy core" status to built with
  its tests (doccheck's `<details><summary>Status: built ...` format, see wire.md); mechanism
  sections stay planned.
- `docs/kernel/model.md` family/mutation lists; `docs/SECURITY.md` only where it names a retired
  mutation. Size budget entry for `redoubt-steward` (docs/testbench.md "The size budget"; ceiling
  with reasons in the commit; model's ceiling if it moves). Whole `cargo testbench --allow-skip`
  (one SKIP: bench-ssh-loopback-openssh), `cargo run -q -p redoubt-doccheck`.

## Traps

- Run every cargo command through `/home/mcloonan/redoubt/.wash/local/in-dev` from the worktree.
- Generated files: change a table or the vocabulary in `libs/steward/gen/src/lib.rs`, then
  `cargo run -q -p redoubt-steward-gen`; the drift test fails otherwise.
- A row from a waiting state answers the call stored on the object (`reply`), not the event's.
- `Frozen | Approve` refuses with `Unknown` (no guard reason), by design of the rows.
