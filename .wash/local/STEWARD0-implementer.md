# STEWARD0: the steward's policy core

Tier A (the steward's policy), size L. Design: QA `STEWARD0-design` and
`docs/servers/steward.md`, "The policy core" (written for this package; read it first, it is the
contract). Needs nothing: it is library and model work, beside the init chain. It shares no file
with a running package except `Cargo.toml`'s member list; K16 touches `model/src/spec.rs` only.

Every cargo and bench command on this host runs inside the dev container:
`/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from `/home/mcloonan/redoubt/.worktrees/steward0`.

STEWARD1, after this package, adds the Elixir reference and the bench's way to run Elixir on
beamlet. Leave `libs/steward/elixir/` to it, except the generated clause skeletons, which this
package's generator writes.

## What it builds

- **`libs/steward`, crate `redoubt-steward`:** `no_std` with `alloc`, `#![forbid(unsafe_code)]`,
  no I/O. Its only dependency outside the workspace is the vendored `sha2`. Inside: `redoubt-sys`
  only for `MAX_LABELS`, if needed.
  - `Store`, `Domain` (`account: NonZeroU64`, a sorted label set) and `DomainState`. The
    one-domain handlers get `&mut DomainState`. One module, `edges`, holds the three two-domain
    functions, and only it can call `Store`'s method that borrows two domains (`pub(in ...)`
    visibility).
  - `Event`, `Effect`, batches with tokens, `Done`; the machines as plain enums; `decide`.
  - `Audit` records, whose one constructor takes `&Domain`.
  - The guards and the effects that carry rules, as functions reached through a `Policy` table
    of function pointers. `Policy::SHIPPED` is the only one in the crate.
  - `render` (printable ASCII, `FIELD_CAP`, the labelled free-text rule) and the binding hash.
  - The steward's constants (`PENDING_CAP`, `FIELD_CAP`, `DECLASSIFY_MAX`, `BLAME_COUNT`,
    `BLAME_WINDOW`, `MAX_LEASE`), moved from the model.
  - A read-only `inspect` module for the model's checks. Nothing in it mutates.
- **`libs/steward/tables/<machine>.md`:** one table per machine (session, lease, request,
  crossing, blame, approval channel), rows `| From | Event | Guard | To | Effects |`, under a
  marker line in the wire tables' style (`<!-- steward: NAME -->`). `steward.md` includes each
  file in place of its row in the Machines summary.
- **`libs/steward/gen`, crate `redoubt-steward-gen`**, in the wire generator's style
  (`libs/wire/gen`). It writes:
  - `libs/steward/src/gen/<machine>.rs`: the dispatch, an exhaustive `match` on (state, event)
    that calls the named guard and effects through `&Policy`;
  - `libs/steward/tables/<machine>.mermaid.md`: the state diagram. The page includes it, and it
    replaces the hand-drawn lease diagram;
  - `libs/steward/elixir/gen/<machine>.ex`: the clause skeletons.

  All of them are checked in, with `generated_files_are_current` and the wire generator's
  refusals (unknown guard or effect, a duplicate row, a silent row, a state no row reaches, a
  state no row leaves except a final one).
- **The model's embedder.** `model/src/steward.rs` stops being a policy and becomes the binding
  of the core to the kernel model. What stays:
  - the kernel plumbing (`Proc`, `run`, the `fsd` stand-in server, its `serve`, `hold`, `reply`
    and `crash_server`);
  - the volumes;
  - the ideal `keyd` (`AuditKeyd`, `AuditSignature`);
  - the entropy source, the keyed mixer.

  `ConnectionShares` and `AdmissionBucket` (R26, not the steward's) move to `model/src/serving.rs`
  unchanged. The policy (`login`, `submit`, `approve`, `end_lease`, `blame` and the rest) is
  deleted. The embedder turns each `PolicyOp` into events, runs each batch on the kernel model,
  and feeds back `Done`.
- **`model/src/policy.rs`:** the families P1 to P16 stay, and drive the embedder. Their checks
  read the core through `inspect`. P10 keeps its replay without the vault's events.
- **Mutations** (`model/src/mutation.rs`):
  - each `Policy*` mutation becomes a swapped `Policy` entry, or one in the model's embedder;
  - retire the four the types make unwritable (steward.md, "Guards and effects");
  - add `PolicyApproveOtherChannel`, `PolicyApproverExceeds` and `PolicyEndLeaseFromVault`.

  `mutations_are_caught` must catch every one that is left.

## Rulings to keep (steward.md says each; do not reopen)

- Ids come from the event's random words, never a counter in the core.
- A batch runs in order and stops at the first failure. A machine waiting on a batch has a state
  for it, so a failure always has a transition.
- An edge's audit record is stamped with the labelled side's domain.
- A narrowing handle is a `Scope` type, made only by `CreateScope` (zero limits). `Connect` takes
  a `Scope`, never a budget (R41).
- Leases are carved from the lease domain's sub-budget, outlive the login session, and are ended
  only from an unlabelled session of the sponsor. Declassify is submitted from a session carrying
  the label. Push snapshots the unlabelled source at submission.
- `EndLease` is marked ahead of admission in its table row; the embedder reads the mark.

## Reading list, in order

1. `docs/servers/steward.md`, whole; then `docs/servers/init.md` R33, R34 and "The confinement
   check"; `docs/servers/serving.md` R25 and R26.
2. `docs/servers/wire.md` "Wire tables and the generator", then `libs/wire/gen/src/lib.rs` and
   `libs/wire/tables/example.md`. One example of the work: commit `7f4468a11`, a generator rule
   landing with its refusal test.
3. `model/src/steward.rs` and `model/src/policy.rs` (the source of every rule's present form),
   `model/src/mutation.rs` (the `Policy*` entries, `is_policy`, the rule map).
4. `docs/kernel/model.md`, where it lists the families and mutations.

## Owned paths

- New: `libs/steward/` (crate, `tables/`, `gen/`, `src/gen/`, `elixir/gen/`),
  `tests/steward-host-tests.toml`.
- `Cargo.toml` (members), `Cargo.lock`, the bench's size budget entry for the new crate (it is
  trusted code: state its ceiling).
- `model/src/steward.rs`, `policy.rs`, `mutation.rs`, new `serving.rs`, `lib.rs`; the model's
  tests that name steward functions.
- Docs:
  - `docs/servers/steward.md`: the tables included; the generated diagrams; "The policy core"'s
    status line to built, with its tests; the mechanism sections' status lines stay planned
    (the server is not built);
  - `docs/kernel/model.md`: the family and mutation lists;
  - `docs/SECURITY.md`: only where it names a retired mutation.

Anything else is a question to the orchestrator first. A gap the page does not settle is a
blocking question on `STEWARD0-design`, never improvised.

## Deliverables, as commits in this order

1. **The tables**, as `libs/steward/tables/*.md`, written from the page and the model's present
   behaviour, every guard and effect named, nothing else. **Checkpoint.**
2. The generator and its tests, and the generated files.
3. The core: domains, machines through the generated dispatch, guards, effects, render, hash,
   with unit tests per guard.
4. The model's embedder, the families on it, the mutations; delete the old policy.
5. Pages, the bench case, the size budget.

## Acceptance

1. `cargo testbench steward-host-tests` (new: the core's and the generator's tests, the drift
   check) and `model-host-tests`, both green; `mutations_are_caught` catches every `Policy*`
   mutation left.
2. `cargo testbench` whole: size budget, unsafe budget (no `unsafe` in the new crate),
   formatting, docs checker.
3. The report says what was deleted from the model (lines), the new crate's size, every mutation
   retired or added and why, and any row a family never reaches.

## Early checkpoint

After commit 1, stop and report (member_update, at most 2000 bytes): the tables' path, each
machine's row count, and every place you had to choose something the page and the model did not
fix. The Architect reviews the tables before any code. Wait for the go-ahead.
