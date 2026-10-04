# SMP2: R12's shares and the targets across harts

Tier A (the kernel). Size M-L. It needs SMP3 merged.

**The owner's answers (QA SMP1-design).**
- One process's threads run on several harts at once, built as SMP1, then SMP3, then this
  package.
- The latency targets are gated at 1 and 2 harts, and recorded at 4.
- R12 is restated across harts once, with several runners per budget, here.

**What SMP1 and SMP3 left you.**
- Every hart runs user code under one FIFO lock.
- The pick takes the lowest-pass budget that has a runnable thread no hart is running.
- Each hart charges its own runner to the budget's one pass, and the floor counts each budget
  once.
- The cases that measure R12 or the targets are pinned at `smp = [1]`.
- The model and the oracle judge one hart only.

Run every cargo and bench command as `/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from
the worktree.

## Context rules (read these first)

- **Don't read whole files.** Run `grep -n`, then Read a range.
  - `sched.rs` and the model's `sched.rs` are long. Read only the functions you change.
  - `scheduling.md` is over 800 lines: read the sections named below.
- **Don't open `.wash/qa/*.md` or other packages' reports.** This brief holds the rulings. If
  you must open a QA file, read it only up to its checkpoint comment:
  `sed '/wash-qa-checkpoint/q'`.
- **Pipe bench output.** Use `cargo testbench --list | awk '{print $1}'`. Read boot logs only
  through `grep` or `tail`.
- **One whole bench at a time.** Sweeps run in parallel, never two whole benches.
- **Read a file right before you Write it,** and prefer Edit.
- **Keep reports under 1900 bytes,** with detail in `.wash/local/SMP2-report.md`.
- **If you hand off, keep the handoff short** and end it with "what consumed my context".

## Reading list (only these)

- `docs/kernel/scheduling.md`: "One flat stride queue", "The current minimum and ties",
  "Charging", "Responsiveness" (to the targets table), R12, and the residual "Measured on QEMU".
- `docs/kernel/model.md`: "Scheduler scenarios" and the differential's paragraph (grep
  `Scheduler\` through the same random sequences`).
- `libs/stride/src/lib.rs`: `Queue` and `Cpu`.
- `tools/testbench/src/sched_oracle.rs`: its entry points only.
- SMP1's report, for what `icount` does with several harts.

## The design

### 1. R12 across harts (the rule)

- A budget gets at least its weight's share of the harts the runnable budgets share.
- It never gets more harts than it has runnable threads.
- What a budget cannot use is shared by the others in proportion to their weights.
- Spreading its threads across harts gains a budget nothing.

These are the shares of water-filling. At one hart they are today's shares.

The shares are of hart time. They assume a hart runs at a rate that does not depend on what the
other harts run. That holds on QEMU, and on the FPGA platform's strict barrel, where a core's two
harts each take alternate cycles. So a barrel pair counts as two harts, each at half a core's
rate, and R12 holds there unchanged. A core that gave an idle hart's cycles to its sibling would
break the assumption; the fpga page keeps its barrel strict for that reason. The targets do not
carry to hardware: a hardware run characterises in cycles and does not gate (scheduling.md, the
residual "Measured on QEMU").

### 2. The floor leaves out capped budgets (Architect's ruling)

**The problem.** At 2 harts, take budget A at weight 900 with one thread, and B at 100 with one.
- A can use only one hart, so it holds hart 0, and its pass rises at 1/900 a tick.
- B has hart 1 to itself, so its pass rises at 1/100 a tick.
- The floor is the lowest pass, A's, so it falls further behind B's every slice.
- Now C wakes at weight 100. It enters at the floor, far below B, and takes hart 1 until it
  catches up. B gets nothing for about as long as it has run.

That breaks R12 for B. It does not happen at one hart, where the running budget is always the
lowest.

**The rule.**
- **Capped.** A budget is capped when its weight's share of the harts is more than it has
  runnable threads.
  - Take the queued budgets in descending order of weight per runnable thread, `w / k`.
  - The first is capped if `w × H > k × W`. `H` is the harts online and `W` is the free weight
    of every queued budget.
  - If it is capped, take its `k` out of `H` and its `w` out of `W`, and test the next.
  - Stop at the first budget that is not capped.
- **The floor** is the lowest pass among the queued budgets that are not capped, with the
  running ones included, each once. As before, it only rises, and it holds when no budget
  counts.
- **A budget that stops being capped is lifted to the floor,** `pass = max(own, floor)`, as a
  waker is. Its pass lagged only because it had no more threads to run, so it cannot bank that.
  Its tie is unchanged.

**Why it is right.**
- At one hart no budget is ever capped, because `w > k × W` cannot hold with `W >= w` and
  `k >= 1`. So nothing at one hart changes: the crate, the model, every mutation and every
  target stay as they are.
- A capped budget's test needs `H > k`, so at most `H - 1` budgets are capped and every pass of
  the loop leaves `H >= 1`.
- The budgets that are not capped share the harts left in proportion to weight. So their
  passes move together, and their lowest is the floor that a waker should enter at.

**Cost.**
- The queue is a fixed array already scanned for the floor (`Queue::raise_floor`). One scan
  sums `W` and keeps the top `H - 1` budgets by `w / k`; the cap loop runs over those; then the
  floor scan skips the capped.
- That is linear in the process count, a fixed constant (R12's kernel-time rule).
- Compare `w × H` with `k × W` in `u128`, with no division.

### 3. Prove the rule on the host first (checkpoint)

Before any kernel change, build the rule in `libs/stride` and in the model. Then run these four
scenarios in the model, with the share each budget should get from water-filling, at 50 per
thousand:

| Scenario | Harts | Budgets (weight, runnable threads) | Shares |
| --- | --- | --- | --- |
| late join | 2 | A (900, 1) and B (100, 1) run long; then C (100, 1) wakes | A 1 hart, B 0.5, C 0.5, from one slice after C wakes |
| second cap | 3 | A (1000, 1), B (100, 1), C (10, 5), D (10, 5) run long; then E (10, 1) wakes | A 1, B 1; C, D and E 1/3 each |
| uncap | 2 | A (900, 1), B (100, 1) and C (100, 1) run long; then A gains a second thread | A 1.64, B 0.18, C 0.18 |
| spread | 2 and 4 | A (100, 4) and B (100, 1) | at 2: 1 hart each; at 4: A 3, B 1 |

- The rule must pass all four.
- Under the floor as SMP3 left it, "late join" and "second cap" must fail. Send me both
  results.
- If the rule fails a scenario, stop and ask me. Don't adjust the rule yourself.

### 4. The stride crate, the model and the mutations

- **`libs/stride`.** Takes `H` harts and a running set in place of one running budget. It keeps
  `k` per budget, and the cap set and lift above.
- **The model's scheduler.** Runs `H` harts, 1 to 4. Each pick is one hart's. It computes the
  cap set its own way, by water-filling over every queued budget with no ordering shortcut, so
  it is an independent check of the crate's top-`H - 1` scan.
- **The differential.** Crate against model at `H` = 1, 2 and 4.
- **`scheduler_fairness`.** Gains the four scenarios above. Its checker computes each budget's
  water-filling share and requires it over each interval the budget was runnable.
- **New mutations,** each of which must be caught:
  - `R12CappedHoldsFloor`: the floor counts capped budgets.
  - `R12CapOnce`: the cap test stops after the first capped budget.
  - `R12UncapBanksCredit`: no lift when a budget stops being capped.
  - `R12OneRunnerPerBudget`: SMP1's one-runner predicate.
  - `R12SpreadChargesOnce`: only one runner of a budget is charged.
- **Contracts.** A scheduler contract asserts that no thread is running on two harts and that
  `k` matches the runnable threads.

### 5. The kernel, the trace and the oracle

- **The kernel.** Wires the crate's rule into `sched.rs`. The checked build audits the cap set
  against a water-filling scan of every queued budget, at each reconcile.
- **The trace.** Gains what the oracle needs: each pick's hart, and each change in a budget's
  runnable threads. List the records you add.
- **The oracle** (`sched_oracle.rs`) judges multi-hart traces.
  - It rebuilds each hart's picks, with its own reading of the eligibility rule (a runnable
    thread no hart runs), the ranks, and the cap set and floor.
  - It checks every pick.
  - It judges shares against water-filling.

### 6. Cases

- **Unpin.** Every case SMP1 pinned to `smp = [1]` runs at `[1, 2, 4]`. The share cases are
  judged against water-filling.
- **`sched-capped`.** Boot cases at 2 harts, and at 3 for "second cap", running the four
  scenarios above, on both widths.
- **`sched-lock-contention`,** at 2 and 4 harts.
  - On every hart but one, a thread calls in a loop the costliest call R12 bounds
    (`map_anon`'s search, as `map-anon-search-bound` drives it).
  - On the remaining hart, `sched-latency`'s driver stand-in waits for the RTC's alarm.
  - The trace records each lock acquisition's wait. The oracle checks the FIFO property: no
    wait is longer than the kernel sections queued ahead of it.
  - The driver wake meets its 1-hart targets at 2 harts. Its numbers at 4 harts are recorded.
- **`sched-latency` at 2 and 4 harts.**
  - Run its seed sweep at 2 harts.
  - The targets table's targets are gated at 2 harts. If one misses, report it and ask me:
    don't move a target.
  - At 4 harts, record the numbers.
  - Say what a millisecond of virtual time means under `icount` with several harts, from
    SMP1's finding.
- **The VM case.** A beamlet VM's two schedulers on two harts, each doing work, and the VM's
  throughput at 2 harts against 1. If the `beamlet-redoubt` step is not done when you reach
  this, ask the orchestrator to cut it into its own node, which needs that step. Don't wait for
  it.

## Pages (exact lines)

Read each anchor right before you edit. SMP1 and SMP3 changed some of these pages. If an anchor
is not as quoted, ask me.

- **scheduling.md**, R12, first paragraph. "A budget's CPU follows its free weight, in one queue
  with no priority. While it has a runnable thread, a budget gets at least its weight's share of
  the CPU the runnable budgets share." becomes:
  > A budget's CPU follows its free weight, in one queue with no priority, on every hart. While
  > it has a runnable thread, a budget gets at least its weight's share of the harts the
  > runnable budgets share, but never more harts than it has runnable threads; what it cannot
  > use goes to the others in proportion to their weights, so spreading its threads across
  > harts gains it nothing.
  Keep "No pattern of spinning, …" and the rest.
- **scheduling.md**, "The current minimum and ties". After the paragraph that defines the floor,
  add:
  > On several harts the floor leaves out a **capped** budget, one whose weight's share of the
  > harts is more than it has runnable threads. Running every thread it has, its pass lags, and
  > counting it would hold the floor below the budgets that share the other harts, so a waker
  > would enter far behind them and take their harts until it caught up. The cap is found as
  > the shares are: in descending weight per runnable thread, a budget is capped if
  > `w x H > k x W` (`H` the harts, `W` the queued budgets' free weight), and the next is tested
  > with the capped one's harts and weight taken out. At one hart no budget is ever capped. A
  > budget that stops being capped is lifted to `max(own pass, floor)`, as a waker is: it
  > cannot bank what it had no thread to run.
- **scheduling.md**, R12, after the rule's statement across harts, add: "The shares are of hart
  time: each hart runs at a rate that does not depend on what the others run, as on QEMU and on a
  strict barrel ([the FPGA platform](../beyond/fpga-platform.md#the-system-on-chip))."
- **scheduling.md**, R12's "It is attacked three ways". To the boot cases' list add "a heavy
  budget with one thread on two harts while others join, and one budget spread over four
  harts"; to the differential add "at 1, 2 and 4 harts".
- **scheduling.md**, the residual "Measured on QEMU, on one hart". Its title becomes
  "**Measured on QEMU.**". SMP1's "The queue and its accounting are judged on one hart until R12
  is restated across harts" becomes "The queue and its accounting are judged at 1, 2 and 4
  harts; the targets are gated at 1 and 2 and recorded at 4".
- **scheduling.md**, "Responsiveness": below the targets table add the 2-hart and 4-hart
  numbers as measured, in the table's form, with one sentence: "At 2 harts the targets above are
  gated; at 4 they are recorded." Add `sched-lock-contention`'s line: what it holds and its
  numbers.
- **scheduling.md**: any other sentence that assumes one running thread (grep "the running
  thread" and "the running budget"; `timer.md` too) is reworded for each hart's runner. List
  each in the report.
- **model.md**, "Scheduler scenarios": add the four rows (late join, second cap, uncap, spread),
  each with its property, and after the table: "The scheduler runs 1, 2 or 4 harts; each pick is
  one hart's."
- **m2-usable-shell.md**, "Progress": add one sentence: R12 holds across harts, judged by the
  oracle and the model at 1, 2 and 4 harts, and the targets are gated at 2 harts.
- **TENETS.md**: no change.

## Owned paths

- `libs/stride`, the model's scheduler and its mutations.
- `kernel/src/sched.rs`: the cap set and the lift.
- The trace's new records.
- `tools/testbench/src/sched_oracle.rs`.
- The cases above, and the `smp` lines of the cases SMP1 pinned.

**Hotspots:**
- B7 owns `tools/testbench/src/{main.rs,build.rs}`: ask before you change them.

## Gates

- The whole bench on both widths at 1, 2 and 4 harts, each run alone.
- The model's and `libs/stride`'s host tests, and the mutations.
- `cargo fmt --check`.
- The unsafe ratchet: expect none.
- The size budget.
- doccheck.

Report each command with its exit code. Also report the four scenarios' shares at the
checkpoint, the `sched-latency` sweep at 2 harts, and the 4-hart numbers.

## Checkpoint

Step 3 is the checkpoint. Send the scenario results, under both the old floor and the new rule,
before any kernel change, and wait for my answer.
