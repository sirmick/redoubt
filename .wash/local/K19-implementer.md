# K19: a destruction's deliveries happen at its boundary, and its steps follow the dying subtree

Tier A (the kernel's destroy path and the model), size M+. Needs IPC3 merged: start from `main`
once it is, never from `wp-ipc3`. The owner chose this shape ("pumps at the boundary", the
recommended option of `.wash/local/destroy-simplify.md`, read its Summary and section (2) only).
Run everything natively on this host under the job pool's rules.

## Context rules (read these first)

- Read `docs/kernel/budgets.md` R10 whole, `docs/kernel/ipc.md` R4b, `docs/kernel/invariants.md`
  I15's "Kept in" paragraph, `docs/kernel/objects.md` "chains", and
  `docs/todo/destruction-walks-every-process.md` whole. Read code by function, never a whole
  file: `message.rs` is 2,300 lines.
- `bench:worst-walk` runs by name only (`whole_run = false`, ~5 min a width, 2,032 MiB); never
  put it in a whole bench. Read its trace with `awk` over the record kinds; never print the ring.
- Reports under 1,900 bytes, detail in `.wash/local/K19-report.md`.

## What IPC3 already did (do not redo; say so in the report)

IPC3's per-endpoint lists (`libs/ipclist`) removed `fail_all` and its rescan, `find_thread`,
`next_sender`, `drop_dying_notices`, the two walks of every thread in `message::budgets_dying`
and `process::endpoints_dying`'s pass over every process object (exit notices and reporters are
now the endpoint's own lists). The plan node's "rescan loop" item is therefore done. What is
left of the two structural facts of `destroy-simplify.md` is below.

## Part A: pumps at the boundary

**The rule.** Inside a destruction, from `begin_destruction` to its boundary, nothing is
delivered: no pump runs. Every place that would pump puts the endpoint on a bounded "to pump"
set instead, once; the destruction's last act, after step 9 and `end_destruction`, inside the
R10 trace bracket and before the payer is billed, pumps each listed survivor once. By then no
doomed process and no dying object exists, so a pump needs no dying or doomed check.

**The sites that pump mid-kill today** (IPC3's tip; recheck line numbers on `main`):
`message::process_ending` (one pump per served endpoint, per killed process, through its
`SERVED` static), `process::settle_notice` (`pump_endpoint` on the exit endpoint, per notice),
and the pumps a failed caller makes in step 4 (`fail_wait`, `fail_callers`).

**The set.** Not a static array of endpoints (a destruction can touch more endpoints than
threads): an intrusive list through the endpoint frames in `ipclist`'s style, head in the
kernel's words, a membership flag so an endpoint is listed once. `endpoint_dying` unlinks a
dying endpoint from it, so the drain meets only survivors; the checked build asserts each
drained endpoint is live with a live owner, and that a second drain pass finds nothing.
Outside a destruction nothing changes: `process_ending` keeps K13's one pump per endpoint
("the same rule for every kernel operation" is not here).

**The five checks** that exist because deliveries ran mid-kill. Count them in the code at
the start and account for each in the report: deleted, or kept as a checked-build assertion
with its reason. None may remain a branch that picks a receiver or sender by asking "dying":
1. `next_receiver`'s `process_is_doomed` (R4b's "a doomed thread takes nothing");
2. `owed_notice`'s `process_is_doomed`;
3. `abandon`'s no-notice-on-a-dying-owner;
4. `settle_notice`'s dying-creator branch and `allowed`'s dying-owner branch: step 2's
   "unless its process object is charged to a dying budget" may stay as the one site that
   implements that clause, or go if step 3's free of the object withdraws its recorded notice
   from the exits list; say which;
5. `message::budgets_dying`'s ordering comment ("every dying budget's queued messages first,
   so that no pump the failed callers below make takes one"): with no pump inside, the order
   is free; the comment goes.
`process_is_doomed` goes when nothing reads it.

**Commit 1 is a model trace,** before any kernel change: a model test that constructs a
dying-stamped send queued on a live endpoint whose receiver is a live process outside the
subtree, a server inside the subtree killed first, and checks whether the kill's pump delivers
the send (then step 4 could not fail it). Report confirmed or refuted; either way the model's
`destroy_budget` then pumps after everything, as the spec. R10 step 4's tightened sentence
needs a mutation the model can catch: write one that re-enables a pump inside the model's
destroy and delivers a dying-stamped message (`R10DeliveredMidDestruction`, or your name,
registered in `model/src/mutation.rs` and on the page). `destroy-simplify.md` expected an R4b
"doomed takes nothing" mutation to retire; `mutation.rs` has none (R4b lists only
`R4bDeadServerFakesReply`), so nothing retires: say so.

## Part B: the three walks follow the dying subtree

From the todo page: `destroy_subtree`'s loop over every live PID (`runs_in_dying`),
`process::budgets_dying`'s `find_process` over every process object once per object it frees
and once more, and `destroy_marked`'s `migrate_held_pids` over every process object. At 510
processes they are the 37–41 ms between a near-empty destruction (16.1/17.1 ms) and a full
one (53.5/58.3 ms); the threads' teardown (12.9/13.6 ms) and the pump stay.

**Chains**, heads in the budget frame, links in the process object frame (`ipclist` words,
like the exits list), audited by `check_lists`/`check_all_dying` in the checked build:
- **charged**: process objects join their creator budget's existing owner chain
  (`first_owned`, today endpoints and devices), so step 3 is the owner walk
  `message::budgets_dying` already makes: each process object met there is freed, its process
  ended first if alive, no notice. Or a separate chain keyed on `creator`, if the owner walk's
  order (devices to the head, endpoints behind) does not take a third kind cleanly; say which.
- **counted**: a chain keyed on `counted_in`. While a process lives, `counted_in` is the
  budget it runs in, so step 2 walks the dying subtree's counted chains for its live members
  (the caller last, as today), and step 8 relinks each chain's remaining members onto the
  parent's chain and recounts them, never reading an object outside the subtree.
- `init`, which the loader started, may have no process object: say how it is found. A walk of
  every PID is acceptable only when `root` is destroyed, where every PID dies anyway.

Done when `bench:worst-walk`'s R10 p99 at full fill is within 30 ms net of audits on both
widths with margin you report; its `must_fail` line and the todo page go.

## Checkpoints

1. After commit 1 and the chain layout (which words of the process object and budget frames,
   which head words), before kernel code: a message naming them, so `objects.md`'s layout
   review happens once.
2. If R10 at full fill is still over 30 ms with both parts in, stop and report the breakdown.

## Pages (land with the code, each in the commit that makes it true)

- `budgets.md` R10: step 4 reads "from the mark on, no message stamped with a dying budget is
  delivered"; after step 9 a sentence: every surviving endpoint that lost a receiver or gained
  a notice during the destruction is pumped once, at its end, and nothing is delivered while a
  budget is dying; status gains the new model test and mutation; the residual bullet "A
  destruction at full occupancy walks every process" becomes the measured sentence in the
  measured list (full fill within 30 ms, the numbers); the per-step cost list updated.
- `ipc.md` R4b: the doomed sentence becomes a consequence ("a thread of a doomed process is
  never offered anything: the destruction ends it before any delivery"), not a check.
- `invariants.md` I15: the "Kept in" list shrinks to what remains.
- `scheduling.md`: R12's status line loses "where a destruction is measured over its bound";
  its residual bullet goes.
- `objects.md` (and `processes.md` if it lists the object's words): the chain words and heads.
- `docs/todo/destruction-walks-every-process.md` deleted; `SUMMARY.md`, `todo/README.md` and
  `plan/m1-separation.md`'s follow-ups lose the link (`cargo test -q -p redoubt-doccheck
  --test docs` catches a dangling one).
- `testbench.md`: the `must_fail` example, if `worst-walk` is the one it cites.
Pages carry no dates, package IDs or review history.

## Owned paths

`kernel/src/{budget,message,process,ptable,mem,endpoint}.rs`; `libs/ipclist` (a new list kind
and words); `model/src/{kernel,mutation}.rs` and its tests; `tests/worst-walk.toml`; the pages
above. **Not yours:** `kernel/src/sched.rs` beyond trace constants (SCHED1/RECON1 own the
scheduler); `tools/testbench/src/sched_oracle.rs` (ask if a bound must move); `libs/rt`.
SMP1 is in flight in `kernel/`: `static`s like `SERVED` are its concern too; rebase early and
tell the orchestrator which words you took.

## The short gate

Both builds (rv64, rv32; release and checked); host tests of `redoubt-kernel`,
`redoubt-model` (the full suite, ~9 min), `redoubt-ipclist`; the docs checker, `cargo fmt
--check`, the size budget (the kernel grows by the chains: set its ceiling per commit at the
fold, with the delta in the report), the `unsafe` ratchet unchanged, the no-cruft gate; own
cases on both widths: `worst-walk` (by name), `endpoint-destroy-full`,
`endpoint-destroy-open-calls`, `budget-destroy-kills`, `ending-pumps-once`,
`destroy-keeps-notices`, `destroy-keeps-notices-creator`, `process-lifecycle`, `redoubt-dead`,
`sched-destroy-billing`, `pid-pinning-attack`, `handle-chain-attack`, `handle-chain-fault`,
`process-chain-fault`, `budget-deadline`, `timeouts`; and the smoke set (`userland-boot`,
`init-boot`, `bench-net-peer`, `ipc-outcomes`). The whole bench is the train's.

## Not here

Endpoint-owned queues (IPC3's, done); deferred reaping; the general "every kernel operation
pumps at its end" outside destruction; the destruction's billing (SCHED1/RECON1); per-hart
anything (SMP1); the model's scheduler.

## Checkpoint 1 ruling (Architect, on `.wash/local/K19-report.md`)

1. **Part A stands as the brief wrote it: the to-pump list and one drain, as a
   simplification.** The trace refuted the message case (eager pumps leave nothing a receiver
   could take at the mark), and found the case that remains: a mid-kill pump can let a doomed
   receiver take a notice owed to a survivor, which only `process_is_doomed` stops today. So
   the five checks may go only if no pump runs inside a destruction, and that needs the list:
   without it `process_ending`, `settle_notice` and `fail_wait` would still pump mid-kill, and
   dropping their pumps instead would leave a survivor's notice unpumped until some later
   operation touched its endpoint (K13, I15's immediacy). The list is the mechanism; the
   checked-build assertion in `pump` ("never while `deferring`") is its guard, not a
   substitute. The words of the report's Part A are confirmed (`K_PUMP` 2, 3; `K_TIMED` 4;
   `E_PPREV`/`E_PNEXT` 12, 13; membership by links; the dying endpoints unlinked in the owner
   walk; drain after `end_destruction`, before the bill, inside the R10 bracket).
   The report's reasoning (1)–(3) goes in the report and in `destroy_subtree`'s comment at the
   drain, as the reason the drain delivers notices and never a dying-stamped message.
2. **No page needs correcting.** No page claimed a mid-kill delivery: R10 step 4 says "a
   message still queued", R4b states the doomed rule, the todo page speaks of walks; the
   suspicion lived in `destroy-simplify.md` (a local file, marked unconfirmed) and now carries
   the refutation. The page sentences of the brief stand: step 4 "from the mark on, no message
   stamped with a dying budget is delivered" (now a consequence the contract checks, with
   `R10RevokedMessageDelivered` breaking it directly and `R10DeliveredMidDestruction` breaking
   "nothing is delivered while a budget is dying"); R4b's doomed sentence as a consequence
   ("a thread of a doomed process is never offered anything: nothing is delivered while the
   destruction runs, and it has ended before the deliveries"). Both mutations on R10's status
   list; nothing retires, and the report says why.
3. **Part B's words are confirmed:** separate charged chain (the owner chain is singly linked
   and `free_owned_endpoints` charges per member: the reason is right) and counted chain,
   `PROCESS_WORDS` 6 (list words 2–5), `CHARGED_WORD` 107 and `COUNTED_WORD` 108 as
   `Page::Budget` words 2, 3, linked in `process_create` once the handle is installed,
   unlinked in `free_object`. Two additions: a rolled-back `process_create` (budgets.md's
   "a rolled-back `process_create`" case) must unlink, and `check_lists` must catch a miss;
   and `objects.md`'s layout text names the new process-object words and the two budget
   heads where it names the chain heads. The `init` exception is confirmed: the walk of every
   live PID only when `root` is destroyed, the checked build asserting every other live PID has
   an object. Step 2 walks the counted chains for live members, caller last; step 3 pops the
   charged chains; step 8 relinks and recounts onto the parent: as written.
4. SMP1 is told the words (the orchestrator relays the report's last section).

## plan_set body for K19 (state todo, needs IPC3)

Brief: .wash/local/K19-implementer.md (architect-16). Tier A, size M+, needs IPC3 merged
(start from main). Part A, pumps at the boundary: no delivery inside a destruction; pumps
become entries on a bounded to-pump list of endpoints, drained once after step 9 and before
the bill; the five dying/doomed checks deleted or made checked-build assertions; commit 1 is a
model trace of the mid-kill delivery, and the model's destroy pumps last; R10 step 4 tightens
to "from the mark on, no message stamped with a dying budget is delivered" with a new model
mutation (no R4b doomed mutation exists to retire: say so). Part B, per-budget chains of
process objects (charged: the creator's owner chain; counted: a chain keyed on counted_in) so
destroy_subtree's kill, step 3's frees and step 8's migration follow the dying subtree. Done
when worst-walk's R10 p99 at full fill is within 30 ms net on both widths, its must_fail and
docs/todo/destruction-walks-every-process.md gone, the residual bullets rewritten. Short gate
plus the R10/R4b case lists and worst-walk by name. IPC3 already removed fail_all's rescan.
