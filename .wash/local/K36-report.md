# K36 report: a swept handle stays a dead entry until its holder closes it (I17)

k36-implementer. Branch `wp-K36`, base main 4750dd61c (train 92), head 9eafda195, four commits:

1. 8523b63ba kernel: a handle a destruction sweeps stays a dead entry until its holder closes it
   (kernel, libs/sys error rows, model + contract + mutation, fake kernel, rt `Budget::destroy`
   closes / `destroy_keeping`, client `Job::kill`, rt/ipd host tests; size ceilings)
2. 9d49d5736 init, steward, VM: close what a destruction left dead (init ceiling)
3. 2ca1f873c testbench: a swept index answers Dead until its holder closes it
4. 9eafda195 docs: I17, a swept index stays dead until closed

Survey: `.wash/local/K36-survey.md`; ruling: rulings file "K36-survey: a sweep never frees an index".
Main has since moved to f6ca3fe8a (train 93, B104: the bench's fingerprint only; not rebased, per the
orchestrator).

## What was delivered

**Kernel** (`kernel/src/handle.rs`, the only kernel file): a slot is `Empty`, `Dead` (kind 5, other
words 0, no chain link) or `Live`. `kill_handle` is every live-table sweep's close
(`close_chain` for S1 budget chains and S2 process-object chain, `kill_handles_where` for S3 device
sweeps): out of its chains, the dead mark written, still counted on its page. `handle()` answers
`BadHandle` (empty), then `Dead`, before any kind check; every call looks handles up there, as its
own or in a list, so a dead index anywhere fails the call before it acts. `install_handle` takes
the lowest `Empty` slot; `handle_close` frees a dead entry (`Ok`). `close_all_handles` unchanged
(no chain links in dead slots). `libs/sys`: every call whose row has `BadHandle` can return `Dead`,
but `handle_close` (the kernel's checked-build row assert caught this on the first boot).

**Model**: `Process::dead`, `indices()`/`holds()` for page accounting and lowest-free;
`sweep` moves to `dead` (or, under `I17SweptIndexFreed`, frees); `Ghost::swept` records every
(holder, index) swept, `i17_swept_indices` checks it; the R18 `device_info` oracle answers `Dead`
from the ghost; `budget_lifecycle` (I10) closes the destroyed budget's and the process's handles,
as a correct holder does; the steward model closes the object's scope on `DestroyBudget`. No new
RNG draws (only `HandleClose` ops added in the lifecycle check). New contract
`swept_index_stays_dead_until_closed`.

**Consumers**:
- rt `Budget::destroy(self)` closes its index on `Ok`/`Dead` (ruling 2); `destroy_keeping(&self)` for a
  holder whose record owns the handle (`Job::kill`, the VM's term). Fake kernel: `Object::Dead`.
- init: closes its old own handle at a server before re-minting (one dead slot per restart
  otherwise). Its console connection is attached on that same own handle, and instance budgets go
  through rt `destroy`, so nothing else leaks. ENDED/quarantine checks unchanged (quarantine reads
  `Dead`, still `is_err`).
- steward: `Made::Budget(B)` (deadline dropped); `Steward::destroy` = `kernel.destroy`, `Dead` → done;
  `DestroyBudget` always destroys by handle and `end_scopes` closes the object's scope; `forget`
  destroys budgets then scopes. **Install lock retired** (ruling 5). Sites read: `lib.rs` trait
  `without_installs` (one caller, `drive.rs` `Steward::destroy`); `steward.rs` field `install`, the
  impl, `launch_thread`/`start_job` (held across `start()` but inside reads), init `prepare()`, the
  latency-probe `lock_waited` line (in no `tests/*.toml`). The lock protected `()` and was taken only
  around the clock read + destroy and the launch thread's installs; the launch thread touches no
  `Machine` table and creates no budget; the launching object's own `end_lease` is held by `parked`,
  not by the lock. So it guarded nothing but index reuse.
- VM (`userland/otp/redoubt`): `Owned::drop` always closes (the `Life` gate went); `Life` stays for
  forgetting connections/launches; `budget_destroy` uses `destroy_keeping` (the term closes it).

**Tests**: model contract + mutation; steward Recorder models dead entries (no lock bracket); the
recycled-index host tests rewritten to their I17 forms
(a_leases_budget_its_deadline_took_keeps_its_index_until_its_end_closes_it,
an_end_lease_after_the_deadline_closes_the_dead_handle_and_fails_nothing,
a_scopes_destroy_after_its_leases_deadline_closes_its_dead_handle,
a_budgets_destroy_closes_its_scopes_dead_handle,
a_leases_end_destroys_by_its_handle_live_or_dead_and_leaves_no_dead_entry); VM, rt, ipd host tests read
`Dead` and close; bench programs (budget-destroy-attack, budget-table-attack, budget-deadline,
expiry-deadline-then-timeout, deadline-in-launch, device-owner-destroy, dma-reset-quarantine,
dma-destroy-quarantine, device-object-page, handle-chain-attack, budget-reap, proc-attack,
proc-review, proc-test, budget-test, sched.rs leases, init's keeper) read `Dead` and close.

**Attack case**: `budget-destroy-attack` (I17's): four swept indices, two rounds of 50 budget
create/destroy churn; every call through each is `Dead`, no handle the churn makes takes one, each
closes once. The verdict lines are the kernel's own return codes printed by the attacker, as the
case already pinned `BadHandle` "as progress"; the case's system verdict stays the kernel surviving
(I1 stop) and the victim served. The model mutation is the sharper check of "never reused".

## Gates (head 9eafda195 unless said)

Load run (orchestrator's go, after train 93), at ca3b27042, `make -k -f scripts/jobs.mk set CASES=...`
(log `.tmp/K36/load.log`), 48 cases, both widths where they boot: 91 PASS lines, one case failed:
`budget` rv64+rv32 (`create(a, spec(98,2,50)) = Ok(8), want Ok(7)`: the program's raw destroys left
dead entries it never closed; logs `.tmp/K36/fail-rv{64,32}-budget.log`). Fixed in commit 3 (each
destroyed budget's handle, and `quick`'s child, closed); then `budget` rv64 PASS, rv32 PASS and
`formatting` PASS at 9eafda195 (prebuilt rc=0). The rest of the run's tree is unchanged by the fix.

- Builds: prebuilt rv64 368 cases / rv32 354 cases, 0 failed (rc=0), at each head.
- Own cases, both widths, PASS: budget-destroy-attack, budget-table-attack, budget-forge-attack,
  budget-deadline, expiry-deadline-then-timeout, deadline-in-launch, device-owner-destroy,
  dma-reset-quarantine, dma-destroy-quarantine, device-object-page, handle-chain-attack,
  handle-chain-fault, process-chain-fault, budget-reap, process-attack, process-review, process,
  budget (after the fix), init-handed-revoked, redoubt-revoke, kernel-containment, init-restart,
  init-driver-restart, init-quarantine-reboot, steward-lease-ends, steward-end-lease,
  steward-launch-overlap, steward-restart, steward-session-ends, steward-boot, pipe-eight,
  pipe-sixty-four, pipe-stage-handle, beamlet-serve.
- Smoke set, both widths, PASS: userland-boot, init-boot, bench-net-peer, ipc-outcomes, sum-clear,
  lend-untouched-page (smp 1 and 4).
- Host cases PASS: model-host-tests (165.7 s), model-mutations (fanned, 177 incl.
  I17SweptIndexFreed, caught by ipc_contracts seed 0), steward-host-tests, steward-model-host-tests,
  rt-host-tests, client-host-tests, init-host-tests, ipd-host-tests. Also run directly:
  `cargo test -p redoubt-sys` (error_rows), fake-kernel servers (fileserver, littlefsd, keyd, sshd,
  consoled, bootfsd, erofsd, walfsd, piped, consrelay), VM `beamlet-redoubt --features fake`: pass.
- docs, formatting, unsafe-budget, size-budget (raised, see below), no-cruft: PASS.
- Not run: the whole bench (the train's).

## Documentation check

Owning pages changed: kernel/objects.md (Handles: kind 5, dead entries paragraph, lowest index
neither live nor dead, error order, `handle_close` row + paragraph, failure bullets, residual "A dead
entry holds its slot", attack text), kernel/invariants.md (I17 section, intro "seventeen", tables,
I12 pointer, I1 attack text), SECURITY.md (I17 row, Tested-by = section list, C7), kernel/abi.md
(Dead's meaning, stage 2 check order), kernel/budgets.md (R10 step 6, call table, error order),
kernel/ipc.md R4 (dead handle in a send's list), kernel/processes.md (process_start order),
kernel/devices.md (failure bullets), kernel/model.md (177 variants, I17 row), servers/steward.md
(deadline-taken lease and budget, launch thread without lock, scope passage, test names),
servers/init.md (own handle closed at restart; quarantine reads Dead), userland/beamlet.md (handle
terms closed dead or live; test names; outside-revoked residual removed; waiter residual reworded).
Summaries checked, no change needed: README.md, GETTING-STARTED.md (no handle/revocation claims:
grep "revoked|BadHandle|swept"), docs/README.md. io.rs Io::forget and native.md served-children:
B106's (left).

## Open risks / notes for review

- The hub-waiter window is narrowed, not closed: a dead entry is never reused, but the VM closes it
  when the connection's last term is collected, which can happen while the waiter still runs past
  `HAND_US`; then the index is free. Stated as the VM page's residual; holding the `Owned` until the
  waiter returns is B106's to weigh.
- steward.md's thread cost: the measured 24-page growth kept; the install lock's page is noted as
  still counted in the entry, not recomputed.
- Size ceilings raised: kernel +37 (10881), libs/sys +7, libs/rt +9, model +55, servers/init +3, each
  with its `Size budget:` line.

## Next

Reviews: sched5-red, sched5-simplifier, sched5-editor. Then B106 builds on this.

## Round 1 fold (head 7f9f1af91, one commit on main 4750dd61c)

Editor: (1) folded to one commit with its reason in the body, five Size budget lines kept;
(2) steward.md thread cost restated without an install lock (twenty-three pages, within the
twenty-four the entry grew by); P3: "as before this rule" dropped. Simplifier: init's
`connection()` count documented as a saving, not a guard; `Owned.life` removed (the carved
budget's `Life` is on `Kind::Budget`); `retired` kept; the repeated leave-chain lines left.
Red P2-1 (closed, not a residual now): `Io::forget` takes the forgotten connection's
`Arc<Owned>` and `Io` holds it until the waiter's hand-over (`hub.waiting` false, checked as each
wake-up is taken), so term collection cannot close the index under the waiter. Pinned by
`a_forgotten_connections_requests_reach_no_later_holder_of_its_index` (the fake's server outlives
its budget, so the waiter never hands over): with the hold the dropped term leaves the handle held
and the next reply's handle takes another index; red without it ("the handle is held for the
waiter", left 18). beamlet.md names the hold; the waiter residual is removed. Red P3-1:
objects.md states the steward's per-scope bound on dead entries.

Gates at 7f9f1af91: prebuilt rv64 368 / rv32 354, 0 failed; docs, formatting, size-budget,
unsafe-budget, no-cruft, init-host-tests PASS; pipe-eight, pipe-sixty-four, pipe-stage-handle,
beamlet-serve PASS on rv64 and rv32; beamlet-redoubt host tests (fake) PASS.
