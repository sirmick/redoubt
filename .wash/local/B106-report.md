# B106 report: a served child's endpoint is owned by the child's budget

Branch `wp-B106`, base `7575a4267` (main after train 91), head `31daa6c64`:

- `689d16212` kernel: endpoint_create names the budget that owns the endpoint
- `31daa6c64` beamlet-redoubt: a served child's endpoint is its own budget's

## Mechanism: (a), and why

`endpoint_create(budget?)`: the endpoint is owned by and charged to the budget named, which must be
the caller's own or below it (`NotPermitted`), and a caller not of class `system` whose labels do not
include that budget's gets `LabelDenied` (the receive-right holder takes what is sent to the owner:
a flow owner -> caller, R1/I7). With none, unchanged (a1 was an unused register that had to be 0).
Chosen over (b) because it adds no round trip or protocol step to every launch, keeps the start
protocol and every served program (piped, the agents' `sub`) unchanged, and is small in the kernel.

Design points the ruling did not spell out, decided from existing rules (say if any is wrong):
- **Stamp = the owner**, as the kernel already did (`stamp = endpoint.owner`), not the caller: so
  "every handle to an endpoint is stamped with its owner or below" (the R10 sweep's and
  `R10RevokedMessageDelivered`'s premise) stays true. objects.md R9 now says so.
- **The LabelDenied check** follows I7 ("a receive right is never handed across label sets"); without
  it a user process given a more-labelled child budget by a system caller could receive that label
  set's traffic.
- **A call made after the death gets `BadHandle`, not `Dead`** (the ruling says Dead): R10 step 6 sweeps
  every handle to an endpoint that died, the VM's send right included. The waiter treats any failure
  as its end, so the effect is the same; ipc.md R4b states BadHandle.

## What changed (paths)

Kernel/ABI (commit 1): `libs/sys/src/{call,error,tests}.rs`, `kernel/src/{endpoint,redoubt,budget}.rs`,
`libs/rt/src/ipc.rs` (`Endpoint::create_in`; `create()` unchanged), `libs/rt/fake/src/lib.rs` (endpoint
owner; a destroyed budget's endpoints die and their handles are swept), model (`syscall`, `kernel`,
`gen`, `trace`, `invariants`, `mutation`, call sites, `tests/common/contracts.rs`,
`tests/endpoint_create_contracts.rs`), `tests/programs/src/{rd.rs,bin/budget-syscall-attack.rs}`,
`tests/budget-syscall-attack.toml`, `tests/size-budget.toml`; pages: kernel/objects.md (calls row and
paragraph, cost table, R9 stamp rule, both status lines), abi.md (call and error rows), kernel/README.md,
budgets.md, invariants.md, model.md, SECURITY.md (R9 Tested-by = R9's status line).

Platform (commit 2): `userland/otp/redoubt/src/jobs.rs` (served endpoint `create_in` the launch
budget), `system.rs` (the VM's send right under the budget's `Life`: never closed after the sweep),
`io.rs` (`Io::forget` doc: true now, with why), fake (processes in a destroyed budget killed: their
open calls fail `Dead`, their host threads get `Dead` for every later call),
`userland/otp/redoubt/tests/system.rs` (two new tests; two existing adjusted: the send right now goes
with the destroy, and the forgotten-connection test no longer needs a HAND_US tick); pages: ipc.md
(R1 owner wording, R4b sentence beside init's root-owned exception), userland/beamlet.md (launch
native, "Handles are resource terms"), userland/native.md (piped paragraph, page counts),
servers/piped.md (start, "piped ends", page counts).

## Evidence (exit codes)

Fake-kernel host tests, red on main's platform code (this branch's fake and tests, jobs.rs/system.rs
as on main): both FAILED `the waiter handed its end over within the destroy` (left 2, right 1);
green on the fix:
- `a_served_servers_end_before_its_waiters_next_call_frees_the_place`
- `a_served_servers_end_fails_its_waiters_queued_call`

Host: `cargo test -p beamlet-redoubt --features beamlet-redoubt/fake` (userland/otp): 97 passed, 0
failed, at the head and at commit 1 alone (old tests, intermediate fake). Root: `cargo test` for
redoubt-sys, -rt, -fake-kernel, -client, -fileserver, littlefsd, keyd, sshd, consoled, bootfsd,
erofsd, ipd, piped, consrelay, walfsd: exit 0, 108 test binaries ok, at the head and at commit 1.
`cargo test --workspace --no-run` exit 0. Model: endpoint_create_contracts, current, device_info,
reap, traces, check_order ok.

Consumer sweep (Call::EndpointCreate shape, rt/fake contract): kernel, redoubt-rt, redoubt-fake-kernel,
redoubt-model, tests/programs (rd.rs, budget-syscall-attack), libs/sys tests; every crate linking
redoubt_rt or the fake built (root workspace `--no-run`, userland/otp, tests/programs via ./build
--programs both widths) and the fake's consumers' tests run, above. userland/native: not built (does
not link the fake or match Call; rt's API only gained a function).

Bench, `make -f scripts/jobs.mk` on prebuilt (prebuilt rc=0 both widths, 0 failed), base 7575a4267:
- Gate set, both widths, 31 jobs: PASS userland-boot, init-boot, bench-net-peer, ipc-outcomes,
  sum-clear, lend-untouched-page (smp 1 and 4), budget-syscall-attack, pipe-carries, pipe-eight,
  pipe-sixty-four, beamlet-launch, beamlet-serve; rv64 host: beamlet-lookup-host, model-host-tests,
  formatting, unsafe-budget, no-cruft.
- Failed first, fixed, rerun green: docs (C5: first R4b citation named, 3 pages); size-budget
  (ceilings raised with `Size budget:` lines in commit 1: kernel 10848->10856 after compacting 3
  lines; libs/sys 1040->1042, rustfmt's block for the error row; libs/rt 4118->4121, `create_in`;
  model 11389->11437, endpoint_create's checks, invariant, mutation, generator, trace). Rerun on the
  final head 31daa6c64 (prebuilt rc=0): size-budget PASS, docs PASS. The kernel line change after
  the gate set is the one-line `map_or` for the owner (same behaviour); budget-syscall-attack reran
  PASS both widths after it.
- model-mutations (fanned, release): PASS, 28.1 s; R9EndpointInAnyBudget caught by ipc_contracts
  seed 0 (`R9: ... created by budget 4, owned by 2, which is not it or below it`). No deadline moved.
  The generator change (r==33 of the endpoint_create draw names the caller's first budget handle) adds
  no draws.
- Measured: piped's budget after pipe-carries holds 87 pages rv64, 84 rv32 (was 86/83): its endpoint.

Attack case: budget-syscall-attack's new lines, `[NotPermitted, BadHandle, WrongObject, None,
BadHandle]` and `the budget below pays for its endpoint: Ok(1)`, are the kernel's own return codes and
usage record (as the case's other targeted lines). `LabelDenied` is attacked only in the model (the
attacker is system class); objects.md's status line says so.

Not run: pipe-sixty-four rv32 x10 on SCHED5's head (the orchestrator's witness); the whole bench.

## Summaries checked

- docs/kernel/objects.md, abi.md, README.md (kernel), budgets.md, invariants.md, model.md, ipc.md,
  SECURITY.md: updated (above).
- docs/userland/beamlet.md, native.md, servers/piped.md: updated.
- README.md, GETTING-STARTED.md: no endpoint ownership or launch claim; no change.
- docs/plan/* (milestone progress), docs/todo/*: no mention of the race or served endpoints; no change.
- Crate READMEs (libs/rt, libs/sys, userland/otp): none describe endpoint_create; no change.
- docs/kernel/objects.md "Because mint only narrows..." (servers' own endpoints stamped with their own
  budget): still true for endpoint_create with none; no change.

## Open risks

1. **Index reuse window.** At the destroy the kernel frees the VM's send-right index. If another VM
   thread installs a handle (a pool call's reply) between the destroy and the waiter's next call, that
   call goes to whatever holds the index (in-session only; the hub session is already ended, so no
   request is sent). The same window already exists for connections piped mints. Not closed here.
2. A launch with `serve` in a budget the VM did not carve (only the VM's own budget today) keeps the
   send right closable on collection; the VM's own budget's endpoint is the VM's, as before.
3. Every served child's budget pays one more page (piped 768-page carve: 87/84 used).

## Next

Review (sched5-red); the witness run on SCHED5's head.

## Round 2 (after the Architect's ruling on sched5-red P1-1 -> K36)

Head `eefad8a99` (kernel `513be51d0`), base 7575a4267. P2-1 folded: `R9EndpointInAnyBudget` now
breaks only the at-or-below check; new `R1EndpointOwnerLabelsIgnored` breaks only the label check
(Mutation::ALL 176 -> 178). Each caught at ipc_contracts seed 0 (R9 invariant; I7 invariant). Random
traces never reach the label path, so `endpoint_create_trace()` (the contract's world) joins the
replay test's fixed traces; both mutations now fail replay (#1793, #2007). Pages: ipc.md R1 status,
SECURITY.md R1 row, model.md R1/R9 rows, objects.md calls status. Model ceiling 11441 (+4 more).
Gates on eefad8a99 (prebuilt rc=0): size-budget, docs, model-mutations (29.1 s), model-host-tests
(140.8 s) PASS. Witness (orchestrator): pipe-sixty-four rv32 10/10 alone on this branch.
P3-1 and the "Dead, not BadHandle" promise wait for K36: on its landing, rebase and correct
ipc.md R4b and io.rs `Io::forget` to promise Dead. SCHED5 also bumps Mutation::ALL: the second to
land takes 179 and the summed model ceiling.

## Round 3 (rebased onto K36; sched5-red OK with notes)

Rebased onto K36 7f9f1af91; head 679c7362a (kernel db8bd93f2). Mutation::ALL 179 (K36 177 + 2);
ceilings summed (kernel 10889, libs/sys 1049, libs/rt 4130, model 11496). Dropped what K36 covers
(the served send right kept unclosed: every term now closes, dead or live). Dead promised in ipc.md
R4b and io.rs `Io::forget`; the fake makes handles to an endpoint dying with its owner dead entries.
P3-1 (fake module doc restated with dead entries) and P3-2 (the kernel's system-class exemption
stated at the fake's label check: every fake process is user-class; no class flag) folded.
Host gates on 679c7362a: both widths build; workspace tests build; 108 binaries ok (--no-fail-fast);
beamlet-redoubt all ok; the three mutations (R9EndpointInAnyBudget, R1EndpointOwnerLabelsIgnored,
I17SweptIndexFreed) caught at seed 0.
Flake seen once, not B106's: redoubt-client `an_admission_refusal_of_the_moment_is_tried_again`
("a walk refused at least once before the one served", 0.08 s vs 2.6 s normally) on the first
pinned run; 5/5 alone and green in the full rerun; libs/client untouched; log
.tmp/B106/p3-host-fail.log.
Pending: rebase onto K36's fixed head; then the load run (boot set, docs, size-budget,
model-host-tests, model-mutations) after train 94's rerun.
