# STEWARD11 report: the destroy-by-index guard holds at every site (the Scope arm)

Branch `wp-STEWARD11`, worktree `.worktrees/STEWARD11`, on origin/main `5bfbd2270` (train 74,
STEWARD10 in). Head: `3d248be61`, one commit, 4 files (its message states the cause and
carries the `Size budget:` line). Reviewer steward-red-5. Ruling: rulings 'STEWARD10 review
notes' (2); brief `.wash/local/STEWARD11-implementer.md`. Logs under
`.worktrees/STEWARD11/.tmp/STEWARD11/` (`host-tests.log`, `check-*.log`, `gate-size1.log`,
`gate-fmt-docs.log`, `gate-set.log`, `gate-size2.log`) and `.worktrees/STEWARD11/target/jobs/`.
STEWARD10's shape note (`.worktrees/STEWARD10/.tmp/STEWARD10/shape.md`) is gone with its
worktree; its report and the rulings' amended mechanism were read instead.

## The cause, as found

`drive.rs`'s `DestroyBudget` arm resolved a `Made::Scope(b)` to `(b, FOREVER)`: a scope is
minted with no deadline of its own, so STEWARD10's guard (`now >= deadline` makes no call)
never fired and the destroy always went by index. Unreachable today: the core's two
`DestroyBudget` emitters name a lease's or session's `BUDGET` token and a crossing's `through`
budget, never a scope, and `forget()` drops a scope's record without a destroy (the budget's
destroy takes it, R41's order). But the kernel destroys a budget's whole subtree with it
(`kernel/src/budget.rs`, the children-first walk), at the budget's deadline (R10) or at the
steward's own `destroy`, so a scope's handle is revoked and its index freed whenever its parent
goes; the first change to ask for a scope's destroy after that would have destroyed whatever
took the index, with `WrongObject` for a non-budget and silently for a budget (a new lease's
budget or scope, the common case: the two host tests show both).

## The rule (steward-only, no kernel change, one idiom)

- `Made::Scope(B, Token)`: the scope's record carries the token of the budget it was made in.
- `Steward::deadline_of(token) -> Option<u64>`: a budget's own deadline; a scope's is its
  parent's (recursively); none when the parent's record is gone (the steward destroyed it, and
  the scope with it). The witness STEWARD10's `destroy` takes, for every kind of record.
- The `DestroyBudget` arm: `budget(token)` for the handle (BadHandle for no record, as before),
  then `destroy(kernel, b, deadline)` if `deadline_of` gives one, else no call; the record is
  removed either way. So a destroy is made only while the record's handle is current: for a
  budget while its deadline is ahead (STEWARD10); for a scope while its parent's record is
  present and its parent's deadline ahead, the two events that revoke a scope's handle. A
  `BadHandle`/`WrongObject` with the deadline ahead stays the failure STEWARD10 settled on
  (a lost handle, an invariant tripwire): nothing in the arm distinguishes a scope.
- `run` is `pub` (doc: every batch an event names runs here, and a host test runs a batch of
  its own). This is the one visibility change: the arm is unreachable through `event`, and the
  node's test feeds the step the core does not emit. The embedder is the binary's own trust
  domain; `bin/steward.rs` does not call it.
- `forget()` is unchanged: a scope's record is dropped, never destroyed, as the doc says.

## Delivered (paths)

- `servers/steward/src/drive.rs`: as above; `Made::Scope`'s doc, `deadline_of`'s doc, the
  arm's comment, a sentence on `destroy`'s doc for the scope case.
- `servers/steward/tests/steward.rs`: the Recorder's `deadline_takes` shares a `takes` with a
  new `destroy_took(budget)` (the kernel's cascade on the steward's own destroy: the children
  leave the live set and their indices are freed; the budget's own index stays, as the field
  says); `scope_of`, `destroy_step` helpers; two tests:
  - `a_scopes_index_recycled_after_its_leases_deadline_is_not_destroyed_as_it`: a 2 s lease,
    its deadline takes budget and scope, a new lease's budget takes the budget's index and its
    scope the scope's; the old scope's `DestroyBudget` makes no call and the new scope is live
    (the verdict); then a live lease's scope destroy ahead of the deadline is by its handle.
  - `a_scopes_index_recycled_after_its_budgets_destroy_is_not_destroyed_as_it`: a live lease's
    `DestroyBudget{BUDGET}` by the steward, `destroy_took` frees the scope's index, a new
    lease's budget takes it; the old scope's destroy makes no call and the new budget is live.
  The budget slot is written as `0` in the test with a pointer to the core's private `BUDGET`
  (`libs/steward` is out of scope). Both fail on the old arm (run with the patch reverted and
  only `run` made public) at the "survives" assertion, i.e. the silent form, and pass with it.
- `docs/servers/steward.md`: "Failure and restart"'s deadline bullet states the rule holds at
  every destroy by a kept handle, the scope's witness and that the core asks for none today;
  the section's status line names the two tests.
- `tests/size-budget.toml`: servers/steward 3549 -> 3553 (the sum rule; the line in the commit).

## Gates (exit codes)

- Host tests: `scripts/q run -- cargo test -p redoubt-steward-server`: 67 passed, rc 0.
- Release checks `cargo check --release -p redoubt-steward-server --bin steward --target
  <triple> [--features latency-probe]` through q: rv64 and rv32, both forms, rc 0.
- `make -f scripts/jobs.mk set CASES="formatting docs size-budget unsafe-budget" -k`:
  formatting PASS, docs PASS, unsafe-budget PASS; size-budget FAIL only for the raise not yet
  committed (`gate-fmt-docs.log`); on the commit: size-budget PASS, docs PASS, rc 0 (`gate-size2.log`).
- The steward set, every `steward-*` case but `steward-latency` (STEWARD9's slot), both widths,
  40 cases, `make -j8 -f scripts/jobs.mk set CASES=... -k`: 78 verdicts, 78 PASS, 0 FAIL (`gate-set.log`; the two host-only cases, steward-host-tests and steward-model-host-tests, run once each, so 78 is the whole set); steward-server-restart PASS on both widths.
- Not run: `steward-latency`; the kernel is untouched.

## Documentation check (summaries)

- `README.md`, `GETTING-STARTED.md`, `docs/README.md`, `docs/milestones/*`,
  `docs/userland/agents.md`, `docs/kernel/budgets.md`, `docs/kernel/objects.md`: no claim on
  how a scope is destroyed or on the steward's handle rule; no change.
- `docs/servers/steward.md`: the owning page, the one bullet and its status line. R41's section
  (line ~1316, "destroying the scope ends the handle in every table") is the kernel's fact and
  stands. The "Leases" bullet (STEWARD10's) is about a lease's budget and stands.
- `docs/SECURITY.md` R41/R10 rows cite no test this package changed; the docs gate passes.

## Open risks and notes

- The scope witness rests on a token naming one object once: a parent token re-inserted with a
  new budget while an old scope's record survived would make the scope's parent "present". No
  path does that (a batch's slots are fixed per owner, and the core forgets an owner whole);
  stated here, not on the page.
- `destroy_took` is the test's own cascade for the steward's destroys, which the Recorder's
  `destroy` still does not model (so every other test's handle numbers stay the order of
  making, as its field comment says).
