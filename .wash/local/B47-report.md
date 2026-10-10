# B47 report

Branch wp-B47, worktree /home/mcloonan/redoubt/.worktrees/B47. Base main a72e774b8, head 8044a5c82.

Commits:
1. 80e35ae60 beamlet: a waiter whose server ended gives its place up (`Size budget: libs/client`, 1089 → 1106)
2. e29806529 beamlet: a handle gone with its budget is not closed again (the QA B47-stale-close ruling, option 1)
3. 8044a5c82 tests, docs: eight pipelines in one session, on both widths

## Bound 1: the VM's waiter table

`userland/otp/redoubt/src/io.rs` `MAX_WAITERS` = 6. `Io::connect` kept one hub connection and one
waiter thread for every namespace connection it had used, and never dropped one. Each pipeline
starts a new `piped` on a new send right bound at `/dev/pipe`. The session's own connections hold
2 places, so pipelines 1-4 filled the table. Pipeline 5's `File.mkdir("/dev/pipe/pN")` got
`TooManyThreads`, which `files.rs` maps to `eio`, so `make_pipes` returned `{:error, :refused}`.

Machine evidence (rv64, before the fix): runs 1-4 `"hi\n" [exited: 0]`, runs 5 and 6
`{:error, :refused}`. By hand after that: `Pipes.open` ok (piped up), mkdir `eio` on all three.
piped's own tables, `MAX_JOBS` and `MAX_SERVED` were not involved.

Fix:
- `libs/client/src/aio.rs`:
  - The hub drops a waiter's badge when its last hand-over arrives.
  - `Hub::release` frees an ended connection's record once no thread of the hub's is left in it.
  - The waiter closes the wake-up handle minted for it when it returns. Before, one handle leaked
    per ended waiter.
- `io.rs`: at the cap, take the waiting wake-ups, then drop the connections the hub releases.
  Badges are picked from 1..=6 among those no live waiter holds.

## Bound 2: stale closes of handles the kernel had freed

- Mechanism: `Budget.destroy` frees the slot of the budget's own handle. Destroying piped's budget
  also revokes the stage connections piped minted (R9 stamps). The kernel reuses those indices,
  and the VM's `Owned` closed the old index when the old term was collected.
- In pipelines the victim was a later piped's budget handle. Its destroy then failed, and its carve
  stayed out for good: `{10,2}`, then `{10,3}`, about +900 pages, on every other pipeline from the
  5th. Or the victim was a stage budget, and the next start gave `wrong_object`.
- Machine check: carve, destroy, carve, collect → `Budget.usage(second)` was `bad_handle`.
- So the lingering weight-1 budget is not piped's by design. piped.md and native.md now say no
  budget of piped's outlives its pipeline, citing `pipe-eight`.

Fix (option 1 as ruled):
- `system.rs`: a VM-carved budget carries a `Life`, marked by `budget_destroy`. The budget's own
  `Owned` is under it. So is every handle returned by calls on a server launched with `serve` in
  it, through `Cap.returns` and `pool::Call.returns`, transitively.
- `Owned::drop` closes nothing that has gone. The VM carves only from its own budget, so there is
  no chain.
- The residual (a handle revoked outside the VM) is stated in beamlet.md "Handles are resource
  terms"; the generation question went to the owner.
- Fake kernel (`libs/rt/fake`):
  - `budget_destroy` frees every handle to the budget and those below it (budgets now record their
    parent).
  - It also frees every endpoint handle stamped by a process playing a child in it (`Fake::play`;
    endpoints carry a stamp).
  - `Fake::destroyed` and `Fake::launched_in` still read a destroyed budget, through a freed-slot
    record. `Fake::last_launched` is new.

## Tests

Host, all through q:
- beamlet-redoubt `servers_bound_and_ended_one_after_another_never_run_out_of_waiters`:
  - Before the fix: `[Ok x5, Err(Eio) x4]`.
  - After: all `Ok`, waiters back to their count, VM handles flat. Without the waiter's handle
    close: 17 → 25.
- beamlet-redoubt `a_destroyed_budgets_index_is_not_closed_when_its_term_is_collected` and
  `what_a_served_server_returned_goes_with_its_budget_and_is_not_closed_again`:
  - Each fails without the platform fix (`bad_handle`) and passes with it.
  - The second drops only the returned handle, so it is not test 1 again.
- redoubt-client `an_ended_connection_is_released_once_its_waiter_has_returned`.
- `cargo test -p beamlet-redoubt --features fake`: rc 0. `--test system` five runs in a row, rc 0.
- Fake-kernel consumer sweep: `cargo test -p` redoubt-client, -fileserver, -rt, -fake-kernel,
  -littlefsd, -keyd, -sshd, -consoled, -bootfsd, -erofsd, -ipd, -piped, -walfsd. rc 0, 95 test
  binaries ok.

Machine (`make -f scripts/jobs.mk prebuilt` rc 0, then `set`, both widths, 101 runs):
- Cases: the `scripts/shell-cases` beamlet set (48 cases, including pipe-*, job-interrupt-ssh,
  beamlet-footprint, steward-*, userland-*), plus job-interrupt-line, job-interrupt-native,
  job-kill, test-shell, piped-host-tests, docs, size-budget, formatting, host-tests,
  unsafe-budget and no-cruft.
- First pass: 99 PASS. docs failed (C11, a package ID in a case and comments) and size-budget
  failed (libs/client 1106 against 1089).
- Both fixed and folded. After a fresh prebuild: docs, size-budget, formatting, rv64/pipe-eight and
  rv32/pipe-eight all rc 0.
- The other 97 runs are on a tree that differs only in those comments and the ceiling; the code
  is unchanged.
- pipe-eight, both widths: `{:base, {{100, 0}, {10, 1}}}`, then
  `{:run, N, "hi\n", [exited: 0], true, true}` for N = 1..8 (budget equal to base, piped not
  running), then `{:collected, :ok, :ok}`. About 20 s each.

## Summaries checked

- Changed:
  - docs/userland/beamlet.md: "Handles are resource terms" (rule and residual) and "Threads"
    (ended servers give up their place).
  - docs/servers/piped.md: "Started by a session" (status + pipe-eight, no budget outlives its
    pipeline).
  - docs/userland/native.md: pipes status (11) and the Pages bullet.
- Checked, no change needed (no claim about pipeline counts, waiters or collection):
  README.md, GETTING-STARTED.md, docs/plan/m2-usable-shell.md (progress names pipelines with no
  count), docs/userland/shell.md "Native programs and pipes", userland/otp/README.md and
  DESIGN.md, libs/client and fake (no README claims).

## Open

- Residual stated on the page: a handle revoked outside the VM. The kernel generation is the
  owner's decision; the Architect sent a decision_request.
- `sys.known` keeps a Known entry, and so the send right, for every piped bound: one VM handle and
  one record per pipeline, never freed while the session lives. That is slow (MAX_HANDLES is 4096).
  Not fixed here; it could be a follow-up: drop a bound connection's Known when its binding is
  replaced and the Io entry is released.

## Fix round (steward-red OK with notes on 8044a5c82)

Head 38602920f on main 050357e8a: 31dfc9b26 waiter, 8bb6adb5c Life, 38602920f pipe-eight.

- P2, stated as a residual (beamlet.md "Handles are resource terms", and the doc on
  `Cap::received`). A request's handles cannot be put under the sender's Life: the kernel tells a
  receiver the sender's badge, account and labels, never its budget. A budget the VM carves
  carries the session's own account and labels, and a badge is whatever the handle passed in
  `launch`'s `handles` carries, which several holders may share. So a program in a carved budget
  that sends a handle it minted, then has its budget destroyed, can make a later collection close
  a reused index. That reaches only its own session's table. A generation in the handle word,
  the owner's decision, would guard it.
- P3: beamlet.md now says "every handle the server ... minted". A forwarded handle is marked gone
  too and is never closed: it stays in the VM's table until the VM ends, the price. Cap.returns'
  doc says the same.
- sys.known per bound piped: the orchestrator's follow-up.

Gates after the rebase onto 050357e8a:
- prebuilt rc 0.
- jobs.mk set, both widths: pipe-eight, pipe-carries, pipe-hostile-output, pipe-interrupted,
  pipe-never-reads, pipe-no-authority, job-interrupt-line, job-interrupt-native,
  job-interrupt-ssh, job-kill, plus docs, size-budget and formatting (rv64): 23 of 23 rc 0.
- ./test-shell through q: rc 0, "every stage passed" (BEAM 339 passed, beamlet 279 passed and 60
  skipped).
- Before the rebase, on 523cc97b6: fake-kernel sweep (13 crates, 95 ok) and beamlet-redoubt host
  (10 ok) rc 0. Nothing since touches them: the rebase brought only others' commits, and
  merge-tree was clean.

Correction: `test-shell` is a script, not a bench case. In my earlier jobs.mk sets it got "No
rule to make target", so it was not run then; the earlier reports listing it as run were wrong.
It was run directly here.

rv32/nscr-hostile-text: B49 found the cause in the case itself; my A/B runs were stopped on
instruction.
