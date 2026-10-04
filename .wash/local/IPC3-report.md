# IPC3 report (ipc3-implementer-2)

## Commits on wp-ipc3 (from main 53bcd9704)

1. `f2b6de13c ipclist: the kernel's IPC lists, host-tested` — new crate `libs/ipclist`
   (redoubt-ipclist; no_std, forbid(unsafe), no deps): `Words` trait, `List` (receivers, IRQ
   waiters, notices, open, queued/taken stamp chains, due list), R2's two group lists with node
   moves (`enqueue`, `pick`, `served`, `dequeue`), stable bottom-up merge sort, audits
   (`List::audit`, `audit_groups`). Workspace member; kernel/Cargo.toml dependency; tests in
   tests/host-tests.toml; size-budget 435, unsafe-budget 0.
2. `dcae23afe kernel: delivery follows the endpoint, not every thread` — message.rs (lists
   plumbing `Frames`, `unlist`, `unqueue`, `unlist_call`; pump/`next_receiver`/`owed_notice`/
   `pick`; deliver's take = `lists::served`; R10 step 4 from dying endpoints' lists then dying
   budgets' queued chains then taken chains; `fail_all`, `find_thread`, `drop_dying_notices`,
   `next_sender`, `W_SEQ`/`W_DUE` gone, thread words renumbered); endpoint.rs (`LIST_WORD`=4,
   `Group::order` removed: dues are distinct counter values, no tie arises); device.rs
   (`LIST_WORD`=12, audit after `irq_ready`); budget.rs (`QUEUED_WORD`=104, `TAKEN_WORD`=105);
   sched.rs (`AUDIT_IPC_LISTS = 3`); redoubt.rs (audit at end of `handle`); time.rs (audit at end
   of `expire_due`). Size budget kernel 8115 -> 8313. Pages: ipc.md, devices.md, scheduling.md,
   budgets.md (see below).
3. `fead0ac09 kernel: the expiry walks once, however many waits it ends` — `collect_due` (one walk,
   due list, sort), `first_due`, `pop_due`; `end_thread` unlinks from the due list; `expire_due`
   rewritten: EXPIRY bracket around collect+sort only, shares billing. Size budget 8313 -> 8350.
   Pages: timer.md, scheduling.md.

Outside my listed owned paths, each by ruling or condition: root Cargo.toml (member),
kernel/Cargo.toml (dep), Cargo.lock, tests/host-tests.toml, tests/size-budget.toml,
tests/unsafe-budget.toml, docs/kernel/model.md (the crate's paragraph after stride's and its test on the status line, folded into commit 1 on your word), sched.rs (one const), redoubt.rs (one audit call), budget.rs (two
consts + one doc ref `message::next_timeout` -> `message::collect_due` at `earliest_timeout`).

## Tests run (all from the worktree via in-dev)

- `cargo test -p redoubt-ipclist`: 8 passed, exit 0.
- `cargo testbench host-tests` (filter = every host-tests case, before the crate moved into
  host-tests.toml): all PASS incl. model-host-tests (521.8 s), stride-host-tests, host-tests
  (testbench's own), exit 0. Rerun after: see the next report.
- Second full host run (verdicts only): every host-tests case PASS (model 553.2 s), exit 0.
- `cargo testbench unsafe-budget`: exit 0 (redoubt-ipclist 0 unsafe; kernel 13+13+18 = 44, unchanged).
- `cargo testbench size-budget`: PASS at each commit's ceiling.
- `cargo testbench docs`: PASS at commits 2 and 3.
- Kernel builds, release and checked (sched-trace, walk-trace), rv64 and rv32: all Finished at
  commits 2 and 3. (rv32 `dev` profile overflows FLASH; not a profile the bench uses.)
- `cargo +nightly fmt --all`: no change outside these files.
- **No QEMU case has run** (orchestrator's hold). The model and oracle are unchanged; the kernel's
  behaviour is unverified on target until the bench runs.

## Host tests in the crate

receivers_wait_in_arrival_order, a_full_receiver_skips_calls, a_group_moves_on_a_take,
a_node_passes_to_its_next_sender, a_destruction_empties_every_list, the_groups_follow_the_model
(200 seeds x 300 random ops against R2 as the model states it, checking both picks and the audit
after every op), the_due_list_sorts_stably, each_audit_trips_on_a_corrupted_link (every list
kind: prev, next, tail corruptions; groups: chain link, member node, count, order). A head-only
list cut short reads as a shorter list; the kernel's audit catches that by its count of threads.

## Behaviour that moves (to the model's)

Receivers by arrival, not (pid, tid); a full receiver passed over while no group has a send; an
abandoned-call notice to the receiving holder that began waiting first, its lowest rid; IRQ
waiters FIFO; destruction fails in list order. Which cases relied on (pid, tid): unknown until the
bench runs.

## Design notes / departures to check

- Ties: R2's keys are distinct values of one counter (seqs and takes), so the kernel never meets
  the "lower group key" tie; `Group::order` removed. ipc.md keeps the rule's tie sentence.
- `WAIT_CAP`: there is no "WAIT_CAP row" with a reason in ipc.md; R2's text now says a take is
  one write whatever the group holds.
- Expiry `next`: each walked process's cache is set to its earliest deadline still to come; a
  wait a deadline's destruction fails mid-expiry leaves it early (one extra early interrupt,
  billed as a wait that ended early). Stated in timer.md's residual.
- Expiry billing: items = popped waits (expired or found ended). A thread killed mid-expiry
  leaves the list unbilled; its share goes to the budget billed last. With no wait due, billing
  as before (walk with the first deadline, or the stale budget).
- Audit: dirty flag set on every list write; runs at end of `redoubt::handle`, `expire_due`,
  `device::irq_fired`. A fault that kills (trap handler, not mine) leaves it to the next. The audit
  finds `root` by walking up from any live process's budget.
- R10 step 4 now fails dying-stamp queued messages before any taken-call failure pumps, so no
  pump takes a message whose stamp is dying (today's walk could, in pid order).

## Pages (exact lines in the diff of each commit)

- ipc.md "What receive returns": "Receivers waiting on one endpoint are served in the order they
  began to wait; a receiver at `MAX_OPEN_CALLS` is passed over while no group has a send. An
  abandoned-call notice goes to the holder that began waiting first."
- ipc.md R2: the restamp sentence replaced by the group turn and the two per-endpoint lists.
- ipc.md residual "Delivery walks every thread, twice over" removed (commit 2); "Why" sentence on
  the thread walk replaced.
- devices.md: "Threads waiting on one interrupt are woken in the order they began to wait."
- scheduling.md R12: "Delivery visits only the endpoint's own receivers, groups and notices, so
  it is no exception either."; charging list: "each with its share of the walk that found it",
  "its last walk" dropped.
- budgets.md: item 4 rewritten ("The endpoints' teardown reads their own lists."); intro "two
  thread walks" -> "the dying endpoints' and stamps' own lists".
- timer.md R12 for timer work: the Architect's sentence; "its last walk" dropped; residual
  "Expiry walks threads" rewritten.
- Still to do with measurements (commit 4): scheduling.md status line naming worst-walk; budgets.md
  measured list's full-occupancy numbers; deleting the two todo pages, SUMMARY.md lines,
  m1-separation.md and testbench.md links; worst-walk toml (must_fail, whole_run, rv32).

## Remaining walks of every live thread or process

- `message::collect_due` (`time::expire_due`): the threads of processes whose earliest timeout
  has come, once per expiry.
- `message::check_lists` (checked build only): every thread and open call, the audit's totals.
- `budget::destroy_subtree` (`budget.rs:1288`): live PIDs (<= 510) for processes in a dying budget.
- `budget::holds_process`: live PIDs (<= 510).
- `sched.rs:167` runnable fill and `sched.rs:389` `next_thread`: per-process passes, <= 510 (K22).
- `MemoryManager::sweep_handles` (`handle.rs:381`): every process's table, quarantined DMA device
  destruction only.
- `destroy_quarantined_devices` (`message.rs`) and `device.rs:210`, `process.rs:273`, `budget.rs:1084`:
  object-frame scans (DMA quarantine; checked-build index audits).
- Per process only (not every thread): `poke_receivers`, `process_ending`, `release_ipc_frames`.

## Commit 4, prepared, uncommitted, not run

- tests/worst-walk.toml: `arch = ["rv64", "rv32"]`, `whole_run` and `must_fail` gone,
  `memory_mib = 2032` (the most rv32's physmap maps; one key serves both widths, so rv64 drops
  from 4608: open until the first rv64 run shows whether 2032 still holds 509 holders; if not,
  a second case file for rv32 is the fallback, since the case has one `memory_mib`), expects
  generic in the counts.
- tests/programs/src/bin/worst-walk.rs: fills what RAM holds (a holder's `budget_create` refused
  with `OutOfMemory` ends its parent's run), keeps the last 250 reports in a ring, prints
  `every PID in use: <bool>`. Builds rv64 and rv32.
- docs: todo pages deleted with SUMMARY.md lines and m1-separation.md items (reconcile's stays);
  testbench.md's worst-walk example of `whole_run = false` dropped; scheduling.md status line
  (delivery, expiry and destruction at full occupancy by `bench:worst-walk`, listed, tested 41)
  and the reconcile residual no longer names delivery and expiry (K22's paragraph: one clause).
- Still to write with the numbers: budgets.md measured list's full-occupancy sentence (its link
  to the deleted todo page is what the docs case fails on until then); timer.md residual's
  measured expiry time.

## Next

Final rebase, after the cases and commit 4, before the whole bench (orchestrator): main is
f9135ce8a (ABI1, ABI2; no kernel change): `git rebase --onto f9135ce8a 53bcd9704 wp-ipc3`.

QEMU, with your word: the IPC, timer, destruction, device and scheduling cases on both widths
(sched_oracle), then worst-walk by name on rv64 and rv32, then commit 4 (case + measured pages +
todo deletions) and the whole bench.

## Handoff

See /home/mcloonan/redoubt/.wash/local/IPC3-handoff-2.md for the state at the tip fead0ac09 (audit per ruling 3b, endpoint count, focused/gate results, worst-walk owed).

## ipc3-implementer-3: rebase onto K22 (128bd45d9), before any QEMU run

`git rebase --onto 128bd45d9 53bcd9704 wp-ipc3` (orchestrator's word; supersedes f9135ce8a and
5646950a5). Tip baa6e8158: 3779a4472 ipclist, 5fee0923f delivery, de2d7fa28 expiry, then commit
4's prep parked as a WIP commit (to be rewritten as commit 4 with the measured numbers; gone by the
report).

- **Frame word clash, resolved:** K22's `READY_WORD = 104` (budget.rs) and our `QUEUED_WORD = 104`.
  Ours move in the delivery commit to `QUEUED_WORD = 105`, `TAKEN_WORD = 106`, after READY_WORD;
  no other `*_WORD` const in the kernel uses 105/106. Stated in the delivery commit's message.
- sched.rs: `AUDIT_IPC_LISTS = 3` beside K22's `AUDIT_MARKS = 4`, both kept.
- size-budget kernel: 8455 (delivery), 8492 (expiry): K22's 8157 + our +298/+37; to confirm with
  the size-budget case.
- Docs: K22 deleted the reconcile todo, so SUMMARY.md's and m1-separation.md's kernel follow-up
  items all go; scheduling.md's residual "A delivery and a timer expiry walk every thread" goes
  (commit 4); R12's status: "and at full occupancy, every PID in use with every thread, for a
  delivery, a timer expiry, a reconcile and a destruction (`bench:worst-walk`) · tested (41)";
  K22's "reconcile on rv64 only (on rv32 the deadline's waits end one per entry)" to be restated
  from the rv32 measurement.

Then `git rebase --onto 0d207732f 128bd45d9 wp-ipc3` (HW1), clean: tip 35af23838 = f0591c8f1
ipclist, c113ae950 delivery, 7ce086c68 expiry, WIP. HW1's kernel change is one comment line and a
widened const assert in arch/riscv/process.rs; it left the kernel's ceiling alone.

Then `git rebase --onto ac84b87b3 0d207732f wp-ipc3` (FSD4, BEAM2): one conflict, SUMMARY.md's
todo list (BEAM2's new beamlet-refused-module line kept, our two lines gone). No kernel change in
the base. Tip 0d5c6a38e = 72a9c5b0a ipclist, 0641b7bea delivery, b7189820d expiry, WIP.

## Focused list at 0d5c6a38e (on ac84b87b3), both widths: 79/79 PASS

`in-dev cargo testbench <f>` for f in redoubt-ipc, ipc-fair-label-sets, ipc-outcomes, timeouts,
budget-, process-, endpoint-destroy, destroy-keeps-notices, irq-, device, sched-timer-flood,
deadline-flood-billed: every exit 0; 79 PASS lines, 0 FAIL/SKIP (39 cases, 38 on both widths,
redoubt-ipc-attack matched twice). Kernel rebuilt in the run (19:23).
- endpoint-destroy-full R10 15,726 µs rv64, 16,484 µs rv32 (as at fead0ac09: K22's reconcile runs
  outside the destruction's span).
- sched-timer-flood: 79 (rv64) / 77 (rv32) timer interrupts finding a wait ended early; audits
  2,434 / 2,259, 1.39 s / 1.68 s.

## worst-walk rv64, first run (0d5c6a38e): FAIL on the case's own deadline, not on R10

`in-dev cargo testbench worst-walk --arch rv64`: exit 1, 289 s. Console: "509 holders of 539
pages, 129796 threads live", "every PID in use: true" (2032 MiB holds every holder on rv64: one
case file serves both widths), "250 holders waited for one deadline: false", destroyed killed
true, WORST-WALK DONE, "SCHED-TRACE-END 1805322 dropped 0". From the trace (awk over U/V, M/m):
- the IPC lists' audit (id 3, ruling 3b's (a)): 1,536 audits, max 359 ms, 345 s in all, one walk
  of 129,796 threads per audited exit (id 2, the PID index: 512, max 650 ms; id 1, destruction: 2,
  max 1.14 s; id 4, K22's marks: 13,680, max 1.1 ms).
- So answering the 250 held reports took ~96 s of virtual time and the program's 1 s DEADLINE
  passed first: the expiry (one EXPIRY walk, 2,181 µs) ended only some waits.
- Ruling 3b item 3: no trace records dropped. Fix in the case, not the audit: DEADLINE = 300 s,
  documented (icount sleep=off: the idle wait costs no wall time). Whether the gate moves decides
  the audit's form, per the ruling.

## worst-walk rv64 rerun (DEADLINE 300 s): FAIL on R10, 74.2 ms > 30 ms

All four console lines true ("250 holders waited for one deadline: true"), dropped 0. Oracle:
"R10's p99 is 74158 µs over 2 destructions, above 30000". Probe build (throwaway `Z 101..106`
records in destroy_subtree, reverted), the victim's destruction, µs: X->T (destroy_subtree's
live_pids/runs_in_dying loop) 5,934; T->t (255 threads end) 12,200; t->pump 2,544; pump 6,083;
->process::budgets_dying 2,744; process::budgets_dying 14,431; message::budgets_dying 15,414 (its
lists/chains ~0; process::endpoints_dying's walk of every process object); lift_dying 111;
destroy_marked 14,705; end 3. The probe's (low fill) destruction: 15.4 ms (pump 35 µs, tail 0.6 ms).
Pumps: 20-35 µs at low fill, 6-13 ms at full (each runs process::pending_notice, a find_process
over every process object, once or twice). Question sent to the orchestrator (options a/b/c).

## worst-walk rv32 (probe build): FAIL on R10, 81.2 ms; the shared-deadline expiry 262/280 ms

rv32 at 2032 MiB holds full occupancy too: "509 holders of 536 pages, 129796 threads live",
every PID in use true, waited true, killed true, dropped 0. R10 81,211 µs: live-PID loop 6,474;
threads 12,846; pump 6,802; process::budgets_dying 15,936; message::budgets_dying 17,004;
destroy_marked 16,219. Both widths end all 250 waits in ONE expiry; its EXPIRY walk (collect +
sort only, no audit inside) is 261,642 µs rv64, 279,452 µs rv32: collect_due walks every thread of
each process whose earliest timeout has come (250 x 255). Question 2 sent: (d) per-process list of
timed waits, or (e) residual.

## Option (a) built (orchestrator's word: build meanwhile; Architect ruling pending)

ipclist: `Page::Process`, member kind (Thread/Call/Process), `P_PREV/P_NEXT` (PROCESS_WORDS 2),
endpoint `E_EXITS` (head, tail, FIFO) and `E_REPORTERS` (head), both counted in E_WAITING
(ENDPOINT_WORDS 12). Crate tests 9/9 (new exit_notices_wait_in_the_order_they_came; destruction
and corruption tests cover both). Kernel: process.rs LIST_WORD = WORDS (30), F_QUEUED; create joins
reporters after the handle installs; settle_notice moves to exits (after R1) and pumps; free_object
unlinks; pending_notice = exits' head (FIFO, the model's order, was lowest frame); endpoint_dying(e)
from message::endpoint_dying (inside the owner walk; endpoints_dying's pass over every process
object is gone). Audits: check_lists also walks process objects; check_all audits both lists in
the endpoint count. Kernel release + checked/trace builds rv64/rv32 clean. Focused list with (a):
79/79 PASS, every filter exit 0.

## Architect-14's ruling on R10 (a + residual), applied (uncommitted; numbers marked @@)

- (a) as above. Exit-notice order moves to the model's (the order processes ended): ipc.md's
  receive-order paragraph and processes.md's "Delivered" bullet say so.
- docs/todo/destruction-walks-every-process.md (new): destroy_subtree's live-PID loop,
  process::budgets_dying's find_process, destroy_marked's migrate_held_pids; Done when: K19's
  per-budget chains, worst-walk's R10 within 30 ms, must_fail gone. Listed in SUMMARY.md and
  m1-separation.md's kernel follow-ups.
- budgets.md: measured list's "a walk of every thread" and "thread walks, 1.3 ms" rewritten; new
  residual bullet "A destruction at full occupancy walks every process" (@@ ms rv64).
  scheduling.md: same bullet in its residuals; R12's status: delivery, expiry and reconcile at full
  occupancy, "where a destruction is measured over its bound".
- tests/worst-walk.toml: whole_run = false kept (by name; ~5 min a width), must_fail on R10's line,
  description names the todo; post_check gains walk bounds (pump/expiry/reconcile_max_us, @@ from
  the measurement), judged BEFORE R10 so the pump stays must-pass. Expects pin 509 holders and
  129796 threads; the program is main's but DEADLINE (300 s).
- tools/testbench/src/sched_oracle.rs: `<walk>_max_us` bounds (net of audits) before R10's; new
  test a_walk_is_bounded_net_of_audits_before_r10; `cargo test -p testbench sched_oracle`: 16
  passed. testbench.md's post_check paragraph says so.
- fmt clean. size-budget: kernel 8566 (+74 for (a)) and ipclist 493 over the current ceilings,
  set per commit at the fold.

## (d) built (Architect-14's expiry ruling), uncommitted

- ipclist: T_DPREV/T_DNEXT = 8/9 (group words shift by 2, THREAD_WORDS 20); `List::timed(slot)`,
  head in the kernel's own words after the due list's (`kernel_words(slots)`); `sort` keyed by any
  Ord. Crate tests 10/10 (new a_timed_wait_leaves_by_every_path: each path's unlink, another
  slot's list apart, a missed unlink caught by the member check, the process's end empties it).
- Kernel: `KERNEL_WORDS` static (was DUE): due head/tail + 511 slots' timed heads (.bss). Departure
  from the ruling's "head in the process's per-thread state": Account cannot take list writes
  through `Frames(&MemoryManager)`; the head is a kernel word indexed by the PID slot, as the due
  list's. Join in `mark` when the deadline is finite (not at block: a process-object free audit
  inside a receive would see a marked, unlisted thread); leave in `wake` and `end_thread`.
  `collect_due` reads only each due process's timed list; sort key (deadline, thread ref) keeps the
  old stable (pid, tid) order. Audits: check_lists counts every thread waiting with a deadline and
  audits its process's list at its head; check_all audits all 511 slots' lists.
- Pages: timer.md's "Expiry walks threads" residual becomes "Expiry walks the processes" (511
  earliest timeouts, then only waits with a deadline; full-occupancy figure @@); :162's text.
- Docs-rule fixes: SECURITY.md's R12 row gains bench:worst-walk (C7); no package ID (C4): the todo
  says "the package that re-cuts the destroy path"; first citations (C5).
- Host gates now: fmt clean; unsafe-budget PASS; docs PASS; size-budget kernel 8599 (a+d), set
  per commit at the fold. Kernel release + checked/trace rv64/rv32 build clean.

## On the host (orchestrator's word), at 0d5c6a38e + uncommitted (a)+(d)+commit-4 work

- Focused list both widths with (a)+(d): 79/79 PASS, every filter exit 0. endpoint-destroy-full
  R10 15,778 µs rv64, 16,535 µs rv32.
- worst-walk measuring runs (temporary post_check `r10_p99_us=100000000`, no must_fail, to get the
  oracle's whole report): PASS both, 323 s / 318 s.
  - rv64: R10 2 destructions p50/max 16,136/53,518 µs (low fill / full), threads' ending
    12,924/12,936; walks net of audits p50/p99/max: pump 1027, 23/1016/1016; expiry 3,
    254/26,832/26,832; reconcile 541,842, 35/75/7,641; pumps inside each destruction 22, 23 µs.
  - rv32: R10 17,143/58,316 µs, threads 13,595/13,633; pump 29/986/986; expiry 334/28,919/28,919;
    reconcile 42/88/8,695; pumps inside destructions 28, 29 µs.
  - The ~1 ms pumps are one per holder start, at low fill (the 2nd pump, before the probe's
    destruction) as at full: a delivery costs the same at both fills.
  - Expiry at 250 waits: 26.8 ms rv64, 28.9 ms rv32 (was 262/280 ms): within 30 ms, the margin on
    rv32 is 1.1 ms.
- Final case: post_check `pump_max_us=2000 expiry_max_us=30000 r10_p99_us=30000`, must_fail on R10;
  no reconcile bound (none ruled; reported). Pages filled with these numbers.

## FINAL (ipc3-implementer-3): tip c9b77fd59 on fbd03208c

1. `9a7b235ac ipclist: the kernel's IPC lists, host-tested` — + exit/reporter lists (Page::Process),
   timed waits (T_DPREV/T_DNEXT, List::timed, kernel_words), generic sort key; tests 10/10;
   ipclist ceiling 499; model.md's paragraph names the new lists.
2. `4b1d79ad9 kernel: delivery follows the endpoint, not every thread` — + (a): process.rs
   (LIST_WORD 30, F_QUEUED, reporters/exits through create/settle/free, pending_notice = exits'
   head, endpoint_dying(e) from message::endpoint_dying), audits; QUEUED/TAKEN_WORD 105/106 after
   K22's READY_WORD 104; ipc.md and processes.md state the exit-notice order (the order processes
   ended; was lowest frame). Kernel 8529 (+372).
3. `083191041 kernel: the expiry walks once, however many waits it ends` — + (d): per-process timed
   waits (head in KERNEL_WORDS static by PID slot; join in mark, leave in wake/end_thread);
   collect_due reads only them; sort (deadline, thread ref); timer.md. Kernel 8599 (+70).
4. `c9b77fd59 tests, docs: the worst walk, measured at full occupancy on both widths` — worst-walk
   both widths at 2032 MiB, DEADLINE 300 s, whole_run = false kept (Architect's ruling 6 /
   orchestrator), must_fail on R10, pump_max_us=2000 expiry_max_us=30000 judged before R10
   (sched_oracle `<walk>_max_us`, new test); todo destruction-walks-every-process.md (SUMMARY,
   m1-separation); the two closed todo pages deleted with their lines; budgets.md, scheduling.md
   (R12 status, residual), SECURITY.md (R12 row), testbench.md.

Commands (worktree, via in-dev), at the measured tree (= tip but for the rebase and ceilings):
- focused list (12 filters) both widths: 79/79 PASS, every exit 0.
- `cargo testbench worst-walk --arch rv64`: exit 0, PASS 320.8 s; `--arch rv32`: exit 0, PASS 315.5 s.
- `cargo testbench kernel-containment --arch rv64`: exit 0, PASS 104.2 s; R10 p50/p99/max
  22,499/23,321/23,321 µs over 18 destructions; threads' ending p99 3,472; driver wake p50/p99
  9,196/10,781; timer 8,306/8,991; decision 6,493/6,515; deadline notice p99 25,120 (<= 40,000);
  lease end 29,836 (<= 125,000); dropped 0; bystander share kept.
At the tip: fmt clean (exit 0); unsafe-budget PASS (crate 0 unsafe; kernel unchanged); size-budget
PASS; docs PASS; `cargo test -p redoubt-ipclist` 10 passed; `cargo test -p testbench sched_oracle`
16 passed; kernel release builds rv64/rv32, 0 warnings; `cargo testbench model-host-tests`: exit 0, PASS 401.4 s.
Not run at the tip: the testbench-own host-tests case (QEMU; host held by ASID1) and the whole bench.

Report items: Group::order removed (dues are distinct counter values); no WAIT_CAP row in ipc.md
(R2 text instead); endpoint frame lists 4..15 (ENDPOINT_WORDS 12: E_WAITING at 12, exits 13-14,
reporters 15) beyond the layout's 4-11; process frame list words 30-31; thread lists 20 words.
Departures for the Architect's check: timed-wait heads in a kernel static by PID slot (not
Account: Frames writes through &MemoryManager), orchestrator agreed; join at mark, not at block
(a process-object free audit inside a receive would see a marked, unlisted thread); the pages name
no package (doc rule C4), so "K19" is "the package that re-cuts the destroy path".
Open risks: rv32's expiry at 250 waits is 28.9 ms against 30 (1.1 ms margin); R10 at full
occupancy 53.5/58.3 ms (residual, must_fail, todo); the checked build's per-exit audit costs
~0.36 s at 129,796 threads (accepted, ruling 6: gate held); worst-walk runs by name only.

## FINAL (ipc3-implementer-4, review fold): tip 2d6bf0db7 on fbd03208c

Commits: 750f3f546 ipclist · 269990ea1 delivery · 2e13c1057 expiry · 2d6bf0db7 worst-walk.
Tree = the fixups' tree (checked: `git diff` pre-rebase vs tip empty).

1. RED P1 (into 2e13c1057). `budget::destroy_subtree` skips its full audit when `deadline_since`
   is Some (only `time::expire_due` passes it); the audit moved to `pub fn
   budget::audit_destruction()` (check_object_indexes + message::check_all, AUDIT_DESTRUCTION
   stamp unchanged); `expire_due` runs it once after its loop, after the final bill and the
   per-exit `message::audit`, `if destroyed`. `budget_destroy` (redoubt.rs, None) audits as
   before. The due-list assert in check_lists is kept as is, so the per-exit audit and every full
   audit still require it empty. Docs of check_all and AUDIT_DESTRUCTION say so.
   New case `expiry-deadline-then-timeout` (rv64, rv32; checked build, icount shift=3):
   calibrates a hand destruction of a 2-process budget (cost C), builds its twin; lease X with
   deadline D (+1 blocked process); R in its own budget holds a handle stamped with X, receives
   on an idle endpoint with timeout (D+1)-now (so T >= D+1). Main spins to D-C/2 and destroys the
   twin by hand: that call straddles D and T, so the expiry at the next entry lists both, ends D
   first (the pre-fix panic point), then T. Verdicts: the call straddled D (began<D, ended>D+1);
   X revoked, 3 Killed + 1 Exited notices; R got Timeout and its stamped send then got BadHandle.
   The order inside one expiry is not visible to an unrelated thread: the case shows survival,
   both ended, and X's revocation before R ran again. Status line: timer.md Expiry names it.
   No host test: the defect is the kernel's audit placement, which no host crate holds.
2. EDITOR. Rewrapped (into owning commits): budgets.md residual bullet and item 4, ipc.md R2
   paragraph (269990ea1); model.md ipclist paragraph (750f3f546); m1-separation.md kernel bullet
   (2d6bf0db7). No added prose line >100 columns left (status lines and tables aside).
   size-budget: libs/ipclist 499 of 499, PASS. 750f3f546's message: over-long line rewrapped,
   "Size budget: libs/ipclist: a new crate of 499 lines: ...".
3. SIMPLIFIER. time.rs Hints reworded to what collect_due reads. Not done (optional): skipping
   check_all's check_lists at idle needs a cached `Listed` or a second flag, since check_all
   compares the objects' lists with check_lists' counts; checked-build only.
Kernel 8599 -> 8609 (+10: audit_destruction and the expiry's call); 2e13c1057's Size budget line
says +80.

Host gates at 2d6bf0db7 (worktree, via in-dev), all exit 0: size-budget PASS; docs PASS;
formatting PASS; unsafe-budget PASS; `cargo test -p redoubt-ipclist` 10 passed; `cargo test -p
testbench sched_oracle` 16 passed; `./build --arch rv64|rv32 --programs` exit 0, only warning the
untouched kernel-half-attack's `mut`; checked kernel (`--profile checked --features qemu-virt`,
and `+sched-trace`) both widths exit 0, no new warnings. model-host-tests not rerun: no model/ or
ipclist source changed since its PASS, and it is ~400 s of CPU beside ASID1's bench.
Not run (QEMU, waiting for the word): expiry-deadline-then-timeout rv64+rv32; focused list rv64;
gate (kernel-containment) rv64; worst-walk rv64. Offered: the new case rv64 on the tip with the
kernel fix reverted, to show it fails there.

### QEMU runs (orchestrator's word), at 2d6bf0db7, one at a time, via in-dev
- `cargo testbench expiry-deadline-then-timeout --arch rv64`: exit 0, PASS; straddle began
  10,928 µs before D, cost 21,912 µs. `--arch rv32`: exit 0, PASS; 12,740 µs, cost 25,894 µs.
- Same, rv64, fix reverted locally (destroy_subtree audits unconditionally), then restored: exit
  1, FAIL, "PANIC in PID 2: panicked at kernel/src/message.rs:2069:5" (the due-list assert,
  "I1: the due list outlived its expiry"). Tree restored clean at 2d6bf0db7.
- Focused list rv64 (the 12 filters above): every exit 0; 40 PASS, 0 FAIL/SKIP.
- `kernel-containment --arch rv64`: exit 0, PASS 105.4 s; driver_wake p50/p99 9,196/10,782,
  timer 8,306/8,992, decision 6,486/6,516, deadline_notice p99 25,120 (<= 40,000).
- `worst-walk --arch rv64`: exit 0, PASS 314.5 s.

### Rebased onto main e9c36bc96 (ASID1): tip c08a6e320
c0c972661 ipclist · 9afcf4896 delivery · faac1c103 expiry · c08a6e320 worst-walk.
Conflicts: tests/host-tests.toml (description and packages: paging and redoubt-ipclist both);
tests/size-budget.toml kernel ceilings, measured on the new base: 8733 at 9afcf4896 (8361+372),
8813 at the tip (+80); the messages' deltas hold. mem.rs and main.rs merged with no conflict;
range-diff shows no other change.
Host gates at c08a6e320, all exit 0: size-budget PASS (kernel 8813/8813, ipclist 499/499); docs;
formatting; unsafe-budget; ipclist 10 passed; sched_oracle 16 passed; `cargo test -p paging` 9
passed; `./build --programs` rv64/rv32 (only the untouched kernel-half-attack warning); checked
kernel both widths, 0 diagnostics.
QEMU at c08a6e320: expiry-deadline-then-timeout rv64 exit 0 PASS (10,907 µs before D, cost
21,912); kernel-containment rv64 exit 0 PASS 103.5 s (deadline_notice p99 25,190 <= 40,000;
driver_wake p99 10,894). Whole bench: not run (follows).

## ipc3-implementer-5: the whole bench's sched-latency / sched-sleep-gaming, bisected (rv64)

All via in-dev, one run at a time; temp worktree .worktrees/ipc3-main (detached), restored clean.
Logs /tmp/ipc3b-<commit>-{lat,gaming}-rv64.log; serial copies and scripts in /tmp/ipc3b/.

| build | sleep-gaming 20 us (gamer/victim) | sched-latency N=16 driver p99 net (timer) | audits |
|---|---|---|---|
| e9c36bc96 main | 33 / 492 ok | 21,590 (22,672) PASS | 7,010 / 8.37 s |
| c0c972661 ipclist | — | 21,590 (22,672) PASS (identical) | — |
| 9afcf4896 delivery | 33 / 495 ok | 74,837 (95,483) FAIL | 12,194 / 10.54 s |
| 9afcf4896, list audits off (exp1) | — | 53,061 (42,574) FAIL | 6,755 / 9.18 s |
| faac1c103 expiry | 26 / 430 FAIL | 117,820 (74,187) FAIL | 13,885 / 11.45 s |
| faac1c103, per-exit list audit off (exp2) | 30 / 495 ok | — | — |

### sched-sleep-gaming: the expiry commit's per-exit audit on every nap
Any write through `Frames` sets CHANGED (message.rs:519); faac1c103 puts every timed wait on its
process's timed list (join at mark, `untime` at wake), so each 1 us nap's system call and the
expiry that ends it now run `check_lists` (a walk of every thread). That time is unbilled and
extends the gamer's slice (sched::audit), i.e. wall time the program's own share check (counts
over the window, no audit netting: the case has no trace or post-check) takes from the victim.
exp2 (only `if CHANGED` disabled) passes: 30/495.

### sched-latency: the delivery commit stops staggering the spinners
Driver budget 466 waits behind spinners with a lower pass. At each N=16 wake its lead over the
floor is the same in every build (median 0, worst 5-14 M pass units = ~1.1 ms of its CPU), but
how many spinners sit below it differs: worst wakes main 3 ahead; delivery 13, 11, 7, 6; expiry
17, 8, 8 (each ahead = one 10 ms slice). Cause: on main each spinner's start-up (the go's IPC)
cost grew with the live threads (the old O(threads) delivery), so their first passes at the go
step +8.6 M each (972, 1062, 1149, ... x100k; ~82 us of a 100-weight budget per process); at
9afcf4896 the step is +0.55 M (479, 485, 490, ...), at faac1c103 the same. Equal slices keep
that spread for the run; a bunch 0.55 M apart puts a driver 12 M ahead behind all of it. With
the list audits off (exp1) it still misses (53 ms), so the audits are not the cause there; they
add on top (75 -> 118 ms at the expiry commit, the per-exit audit on the steward's timeouts).
The scheduler is unchanged; main met the N=16 targets on an O(n) artefact IPC3 removes.

### Proposed (nothing changed yet)
1. Gaming, into faac1c103: the timed lists' join (mark) and leave (`untime`) do not set CHANGED,
   so a plain sleep and the expiry that only ends it run no per-exit `check_lists`; the timed
   lists stay audited whole by every `check_lists` another change triggers and by `check_all`
   (idle, destruction, object free), and the expiry asserts what it reads. Small (save/restore
   the flag around the two calls). Expected: gaming back to exp2 (495), latency back to the
   delivery commit's level (75 ms), still a miss.
2. Latency: a design decision (not an IPC3 defect; the stride wake rule meets a bunch the old
   O(n) costs had spread; TCG runs on main already reach 152-172 ms max at N=16). Options:
   (a) a scheduler package for a waker's place behind a tight bunch, with the N=16 driver/timer
   p99 recorded as a residual meanwhile; (b) re-sweep the N=16 targets on this kernel; (c) make
   the workload stagger its spinners (not recommended: it would test the artefact). Recommend (a).

### Fix 1 applied (orchestrator's go): tip a0fbbcab1
c0c972661 ipclist · 9afcf4896 delivery · 55f7ca6ac expiry (amended) · a0fbbcab1 worst-walk.
message.rs: `timing(f)` keeps CHANGED as it was around the timed waits' and due list's writes
(untime, mark's join, collect_due's push and sort, pop_due); a paragraph in the expiry commit's
message says so; kernel ceiling 8813 -> 8823 (+90 in that message). Tree = the measured tree
(WIP diffed empty). No case's expectation changed.

Measured on this tree, via in-dev, one at a time:
- sched-sleep-gaming rv64 exit 0 (20 us: gamer 30, victim 494); rv32 exit 0 (50 / 492).
- sched-latency rv64 exit 1: N=16 driver net p99 96,212 (max 151,173), timer 54,018: missed;
  N=1/N=4 met; decision, deadline notice, lease end met; audits 12,210 / 10.80 s (was 13,885 /
  11.45 s). rv32 exit 1: N=16 driver 86,550, timer 84,591: missed; the rest met.
  (The bunching of the spinners, ruling with the Architect / owner.)
- rv64, every exit 0, 43 PASS, 0 FAIL/SKIP: the 12 focused filters, expiry-deadline-then-timeout,
  kernel-containment (103.7 s; gate driver_wake p99 11,321, deadline_notice p99 25,213),
  worst-walk (289.8 s).
- Host gates at a0fbbcab1: size-budget (kernel 8823/8823), docs, formatting, unsafe-budget: PASS.
