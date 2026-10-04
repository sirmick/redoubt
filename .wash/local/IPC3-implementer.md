# IPC3: delivery follows the endpoint, not every thread

Tier A (the kernel's IPC core), size L. Needs K16 merged; start from main after it. Run every
cargo and bench command as `/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the
worktree.

## Context rules (read these first)

- **Don't read whole files.** `kernel/src/message.rs` is several thousand lines. Run `grep -n`,
  then Read a range: `find_thread`, `pump`, `next_sender`, `receiver_on`, `irq_ready`,
  `fail_all`, R10 step 4's message reach (search "R10 step 4"), the endpoint destruction path,
  `poke_receivers`, `Slot` and `next_timeout`; in `kernel/src/time.rs` only `expire_due`.
- **Don't open `.wash/qa/*.md`, other reports or other briefs.** If you must open a QA file, read
  it only up to its checkpoint comment: `sed '/wash-qa-checkpoint/q'`.
- **Pipe bench output.** Read boot logs under `target/testbench/last/` only through `grep` or
  `tail`; a trace dump only through the bench's own summary.
- **Keep reports under 1900 bytes,** with detail in `.wash/local/IPC3-report.md`.

## Reading list (only these)

- `docs/kernel/ipc.md`: "What `receive` returns" (notices before messages), R2, R3, R4, R4a,
  R4b, and the residual "Delivery walks every thread, twice over".
- `docs/kernel/scheduling.md`, R12: the paragraph "A system call's kernel time is bounded by…".
- `docs/kernel/budgets.md`, R10's step 4 and "Residual risks" (the measured list).
- `model/src/kernel.rs`: `pump`, `next_sender`, and `Endpoint::receivers` (a `VecDeque`).
- `docs/todo/delivery-walks-every-thread.md` and `docs/todo/expiry-walks-once-per-wait.md`:
  their "Done when" is this package's acceptance.

## The problem

Every delivery walks every live thread of every process (`find_thread`), up to three times, and
for each receiver waiting on the endpoint `next_sender` walks them all again. At K16's full
occupancy (511 processes, 130,051 threads) one delivery costs O(receivers × threads), and K16's
worst-walk case measured it (its numbers are in ipc.md's residual). The same shape is in
`irq_ready` and in destruction's message reach (`fail_all`, which rescans from the start after
each thread it fails, and the reply-waiter pass). R12 says a call's kernel time "never depends on
what other processes hold"; these walks keep it only by the constants clause, and at 130,560
slots that is no longer true in fact.

## The settled design: lists on the endpoint

Every list is intrusive: its links live in the threads' IPC pages (the `Slot`), the endpoint's
frame (it uses 4 of 512 words), the open-call frames or the device object. No new allocation, no
table sized by a constant times endpoints, and nothing charged to a budget that is not already
paying for the page holding the link.

1. **Receivers.** Per endpoint, a FIFO of the threads waiting in `receive` there, in the order
   they began waiting: the model's `receivers` `VecDeque`. A message goes to the first receiver
   that can take it (R4a: one in a process at `MAX_OPEN_CALLS` takes only sends). Today the
   kernel picks in (PID, TID) order and the model in arrival order; the kernel now follows the
   model. If any case or the oracle depended on (PID, TID) order, report it by name.
2. **Senders, by R2 group.** Per endpoint, the groups with a sender queued there, kept in
   (due, group order), each with its senders in arrival order, sends and calls in two chains so
   a full receiver's pick (R4a) finds a group's oldest send without walking its calls. The pick
   is the first group's head; for a full receiver, the first group with a send. A take that moves
   a group's turn moves the group in the list. A group's node lives with its head sender and
   moves to the next one when the head is taken. Inserting a group walks the group list: at most
   one group per live process, a fixed constant.
3. **Notices.** Per endpoint, the threads owing an abandoned-call notice there, so the notice
   pick visits only those, and only a thread waiting in `receive` there takes one. Exit notices
   keep their per-endpoint queue.
4. **Interrupt waiters.** Per device object, the threads waiting in `receive` on its IRQ handle,
   so `irq_ready` reads one list.
5. **Destruction.** R10 step 4's message reach and endpoint destruction walk the dying
   endpoints' lists (receivers, senders, owed notices) and, for callers waiting for a reply
   through a dying endpoint, a per-endpoint list of its open calls, linked in the open-call
   frames. `fail_all`'s rescan goes. Nothing in a destruction walks every thread.
6. **The expiry.** `time::expire_due` (`kernel/src/time.rs`) calls `next_timeout` once per
   wait it ends, and each call walks every live thread of every due process: R waits on one
   deadline cost R walks. One walk collects every due wait in the order the timer ends them
   (earliest first, a timeout before a deadline at an equal instant), or the due waits come off
   a list kept by deadline; either way ending R waits costs one walk or R steps. The order and
   the billing of each ended wait are unchanged (`docs/todo/expiry-walks-once-per-wait.md`'s
   "Done when").
7. **Unchanged:** every rule's behaviour (R2's order, R3, R4, R4a, R4b, R13, notices before
   messages), the model, the oracle, every R2, R3 and R4 mutation, and the ABI. `WAIT_CAP`'s
   restamp may become one write per group; if so, say so and give ipc.md's `WAIT_CAP` row's new
   reason in the report.
8. **A checked build audits every list** against a walk of every thread at each kernel exit that
   changed one, as the PID index is audited (R12's index rule). The audit is the checked build's
   only, and excluded from the latency targets as K15's are.

**Design checkpoint.** Before building, write the layout (each list's head and links, in which
word of which page or frame, and how a group node moves) in `.wash/local/IPC3-layout.md` and stop
and report. One page, no code.

## Remaining walks

After this, list every walk in the kernel that still visits every live thread or process, with
its caller and why it stays (for example, `sched.rs`'s per-process passes, bounded by
`MAX_PROCESS_COUNT`, which K22 marks). Report the list; change none of them.

## The cases

1. **K16's worst-walk case**, at full occupancy and at its smallest fill: one delivery's kernel
   time from the trace is the same within noise at both, and R10's destruction at full occupancy
   is within 30 ms net. So is the expiry at the shared deadline that ends 250 waits (K16
   measured 28.9 s, the wakes' pumps nested inside it): report its time with the pumps apart
   from the timer's own walk. `worst-walk` loses `must_fail` and `whole_run = false` and runs
   on rv32 too; if rv32's RAM cannot hold full occupancy, run it at the most it holds and report
   the count. Give both widths' numbers.
2. **A host test on the kernel's lists** (the fake or the unit tests): receivers in arrival
   order; a full receiver skipping calls; a group moving on a take; a group node passing to its
   next sender; a destruction emptying every list. Each list's audit trips on a corrupted link.
3. **The model differential** (`the_crate_and_the_model_agree`, the model's contracts and every
   R2/R3/R4 mutation) passes unchanged.
4. **The whole bench, both widths**, and the gate's numbers on budgets.md and scheduling.md do not
   regress.

## Page lines (in the commit that makes each true; exact text in the report)

- **ipc.md**: the residual "Delivery walks every thread, twice over" goes. In R4 or "What
  `receive` returns", one sentence: a message goes to the receiver that has waited longest of
  those that can take it. The R2 text names the group list if it names a mechanism today.
- **scheduling.md**, R12: after "R10 (destruction) walks only the dying subtree, …", one
  sentence: delivery visits only the endpoint's own receivers, groups and notices, so it is no
  exception either. The status line's "attacked only for …" list gains delivery, naming the
  worst-walk case.
- **budgets.md**: the measured list's worst-walk numbers become the new ones; R10's step 4
  names the dying endpoints' lists in place of the thread walks.
- **timer.md / budgets.md residuals**: any sentence that says a destruction walks every thread
  for its notices; timer.md's expiry residual says what the code then does.
- **docs/todo**: `delivery-walks-every-thread.md` and `expiry-walks-once-per-wait.md` are
  deleted, with their SUMMARY.md lines and m1-separation.md's links.

Give the exact lines in the report for the Architect to check.

## Owned paths

- `kernel/src/message.rs`, `kernel/src/endpoint.rs`, `expire_due` in `kernel/src/time.rs`, the device object's waiter link
  (`kernel/src/device.rs` or wherever `DeviceRef` lives), and the open-call frame's layout.
- The kernel's host tests for these.
- The page lines above.

**Not yours:** `sched.rs`, `budget.rs` beyond the call into step 4's reach, the model and the
oracle (they must not change), `kernel/src/arch/**`.

**Hotspots:**
- SMP1 (M2) follows this package and does not own `message.rs`; its eviction hook is in the
  destruction path. If SMP1 starts first by override, it keeps out of `message.rs` and whichever
  lands second rebases.
- K19 ("pumps at the boundary", with the owner) changes *when* destruction pumps, not how a pump
  finds its parties. If it is approved and starts, the two split at `pump`'s signature.

## Gates

- The whole bench on both widths, alone (one whole bench at a time).
- The kernel's host tests, the model's tests and the stride differential.
- `cargo fmt --check`, the size and unsafe budgets, doccheck.

Report each command with its exit code, the layout, the remaining-walks list, the worst-walk
numbers before and after on both widths, and each page line as written.
