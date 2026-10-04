# SMP3: one process's threads on several harts at once

Tier A (the kernel). Size M. It needs SMP1 merged.

**The owner's goal.** A beamlet VM is one process in one budget, and its two scheduler threads
should run on two harts at once. The design is in three packages:
- SMP1 (merged): the harts, the lock, and the shootdown routine with destruction as its one
  caller. A budget runs on one hart at a time.
- SMP3 (this one): a budget runs on as many harts as it has runnable threads. The shootdown
  gains every caller it needs.
- SMP2 (after this): R12 and the targets restated, modelled and judged across harts.

Nothing here replaces what SMP1 built. SMP3 changes one predicate in the pick, adds callers to
`shootdown`, and adds a trace record. No ASIDs: K16's ASID 0 stays. Don't start until SMP1 is
merged.

Run every cargo and bench command as `/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from
the worktree.

## Context rules (read these first)

- **Don't read whole files.** Run `grep -n`, then Read a range.
  - `message.rs`, `budget.rs`, `sched.rs` and `mem.rs` are each over 1,000 lines. Read only the
    functions you change.
- **Don't open `.wash/qa/*.md` or other packages' reports.** This brief holds the rulings. If
  you must open a QA file, read it only up to its checkpoint comment:
  `sed '/wash-qa-checkpoint/q'`.
- **Pipe bench output.** Use `cargo testbench --list | awk '{print $1}'`. Read boot logs only
  through `grep` or `tail`: they begin with hex dumps.
- **One whole bench at a time.** Run sweeps in parallel, never two whole benches.
- **Read a file right before you Write it,** and prefer Edit.
- **Keep reports under 1900 bytes,** with detail in `.wash/local/SMP3-report.md`.
- **If you hand off, keep the handoff short** and end it with "what consumed my context".

## Reading list (only these)

- `docs/plan/m2-usable-shell.md`: "Several harts", step 4.
- `docs/kernel/scheduling.md`: "One flat stride queue", and "The current minimum and ties" up to
  the figure.
- `docs/kernel/memory.md`: "Instruction fetch after mapping", "Lending at the page-table
  level", the R11 bullet on W^X, and the residual risks "Several harts" and "A lend within one
  process".
- SMP1's `shootdown` routine, its per-hart block, and its pick predicate (grep for them).

## The design

### 1. The pick

- **The predicate.** SMP1's predicate, "not running on another hart", becomes "has a runnable
  thread that no hart is running". Keep it one predicate.
- **The thread.** The budget's next runnable thread that no hart is running, after its cursor,
  in (pid, tid) order, wrapping. The cursor moves to it.
- **The arithmetic is unchanged.** The stride state stays per budget: one pass, one remainder,
  one tie and one entry.
  - Each hart counts its own runner's ticks from its own last return to user mode.
  - When a runner leaves its hart, that hart charges the runner's budget, under the lock, with
    the rule as it is today (at least one tick).
  - A budget stays queued while any of its threads is runnable or running.
  - A requeue takes `back + 1` at each charge that leaves the budget queued.
  - The floor is the lowest pass among queued budgets, each budget counted once at its one
    pass, however many harts run it.
- **The slice.** The slice end and the deadline are each hart's, for its own runner.
- **Reschedule IPIs.** When a wake leaves a thread runnable that no hart is running, and a hart
  is idle, the waker sends the idle hart an IPI. This includes a thread in a budget that is
  already running elsewhere. A wake still never preempts a running hart.
- **Running counts.** If you keep a per-budget count of running threads, the per-hart blocks
  stay the one source of truth. A checked build audits the count against them at every pick.
- **At one hart nothing changes.** The predicate reduces to today's. `the_crate_and_the_model_agree`
  and every stride host test pass unchanged, and the model is not touched. Say so in the report.

### 2. Shootdowns at every removal

The TLB holds only a process's valid entries, and only on the harts running it now (SMP1's
residency). So the harts to flush are the *other* harts whose per-hart block names the process.
Call `shootdown(P)` before the call returns, and before any frame it frees or moves is reused.
Call it at each of these sites:

| Site | P |
| --- | --- |
| `unmap` | the caller's process |
| `set_flags` that removes any of `READ`, `WRITE` or `EXECUTE` from an entry | the caller's process |
| `process_map` | the source, which is the caller's process |
| a message sent with a lend or a transfer (the sender's entries lose `VALID`) | the sender's process, before a receiver can take the pages |
| a reply returning a lend (the borrower's entries are cleared) | the borrower's process, which is the replier's |
| destruction | SMP1's; unchanged |

- **Clearing an entry already invalid needs no flush.** That covers a transfer's delivery, an
  abandoned lend going to the server, and the lender's entry at the return. A flush is needed
  only where a `VALID` entry is cleared or narrowed.
- **Find every site.** Grep the kernel for every place that clears `VALID` on a user entry or
  narrows its flags. Each must be in the table or call `shootdown`. List every site in the
  report, with the file and function. A site not in the table is a finding: report it.
- **The common case costs a check.** When no other hart runs P, `shootdown` sends nothing.
  Measure `unmap`'s cost at one hart before and after, and report both.

### 3. Instruction fences

- **When code becomes executable.** A call that installs an executable entry in a process also
  calls `shootdown` on that process. SMP1's routine already runs `fence.i` on each target before
  it acknowledges. The calls are `map_anon`, `map_fixed` and `set_flags` with `EXECUTE`, and
  `process_map`, whose target is the destination process. Don't add a second kind of request.
- **A thread that moves needs no fence of its own** (Architect's ruling). A RAM frame has at most
  one user entry, and no entry is writable and executable (R11, W^X). So a process's code
  changes only through a call that adds `EXECUTE`. That call has fenced every hart then running
  the process. Any other hart runs `fence.i` before it runs the process (SMP1). This replaces
  the node's "and on a hart a thread moves to".

### 4. The trace

- **A shootdown record.** In a kernel built with the scheduling trace, each `shootdown` that
  sends a request writes one record: its hart, the target pid, the set of harts it asked, and
  the set that acknowledged.
- **The oracle.** It keeps judging only one-hart runs, and ignores the record. Only the cases
  below read it.

## The cases (both widths, in a checked build)

1. **`smp-shootdown`**, at 2 harts, with `icount` and without. Three variants, one verdict
   each:
   - **unmap.** Thread W of process P writes its page X without end on one hart. Its sibling U
     unmaps X on the other hart. A checker in another budget then maps pages until it gets X's
     frame (as `smp-evict` does) and watches the frame stay zero. P ends faulted, code 15, by W's
     next store.
   - **lend returned.** Server S has a receiving thread R and a writer W, on different harts.
     A client lends S a page holding a pattern. W writes a counter into the borrowed page without
     end, and R replies. After its call returns, the client reads the page over 10 ms and finds
     it unchanged. S ends faulted, code 15.
   - **lend within one process.** Process Q holds thread A, which calls an endpoint Q itself
     receives on and lends a page; thread B, which receives and replies; and thread W, which
     writes the borrowed page on another hart. The case passes if Q ends faulted, code 15, by
     W's store, with no line from A saying it saw the page change after the reply.
   - **Recorded negative.** The feature `smp-no-shootdown` (off in every default build) skips
     the calls this package adds, but not destruction's. It must fail every variant: the
     checker or the client sees a stale write, or the case times out.
2. **`smp-fence`**, at 2 harts, in a trace build.
   - Sibling S of process P spins on one hart, polling a word.
   - Thread T, on the other hart, maps a page writable, writes code into it, uses `set_flags`
     to make it read-execute, and stores the page's address into the word.
   - S jumps to the code, and the code prints a line.
   - The verdict is from the trace: S's hart was running P at the `set_flags`, and the
     `set_flags`'s shootdown record lists S's hart as acknowledged.
   - Under `smp-no-shootdown` the record is missing, and the case must fail.
   - QEMU keeps instruction fetch coherent, so no case can see a stale fetch. Say so on the
     page (below).
3. **The whole bench at `--smp 2`** on both widths, apart from the cases SMP1 pinned. Report
   what passed. A failure is a finding to fix, never a case to pin.

## Pages (exact lines)

SMP1 wrote some of these sentences. Read each anchor right before you edit. If one is not as
quoted, ask me rather than guess.

- **scheduling.md**, "One flat stride queue". SMP1's sentence "On several harts each hart picks
  the lowest-pass budget not running on another, so a budget runs on at most one hart at a time
  and its stride state has one runner." becomes:
  > On several harts each hart picks the lowest-pass budget that has a runnable thread no hart
  > is running, so a budget runs on as many harts as it has runnable threads. Its stride state
  > stays one per budget: each hart charges its own runner, under the kernel lock, to the
  > budget's one pass.
- **scheduling.md**, same section: "Each pick runs the budget's next runnable thread after the
  one it ran last, in (pid, tid) order, wrapping." becomes "Each pick runs the budget's next
  runnable thread that no hart is running, after the one it last picked, in (pid, tid) order,
  wrapping."
- **scheduling.md**, "The current minimum and ties": "the running one included at the pass it was
  last charged" becomes "the running ones included, each once, at the pass it was last charged".
- **scheduling.md**, "Charging": "the kernel adds the user time since the last return to the
  running budget's pending runtime" becomes "the kernel adds the user time since that hart's
  last return to the pending runtime of the budget whose thread it ran".
- **memory.md**, "Instruction fetch after mapping". "The fence covers the one hart the kernel
  runs on. A kernel on several harts must also fence the others, and fence when a thread moves
  (M2 (usable shell): [several harts](../plan/m2-usable-shell.md#several-harts))." becomes:
  > On several harts the call also shoots the process down on every other hart running it,
  > and that hart runs `fence.i` before it acknowledges; a hart also runs `fence.i` before it
  > runs a process. A thread that moves between harts needs no fence of its own: a RAM frame
  > has at most one user entry, so a process's code changes only through these calls.
  The status line becomes: "Status: built · partly tested: no case can see a missing
  `fence.i`, because QEMU keeps instruction fetch coherent with stores; `smp-fence` shows from
  the trace that the other hart fenced · tested: bench:smp-fence".
- **memory.md**, "Lending at the page-table level": the status drops "partly tested: a lend
  within one process is not attacked across harts", and `bench:smp-shootdown` joins its list
  (tested (9)).
- **memory.md**, residual risks. SMP1's "Several harts" bullet becomes:
  > - **Several harts.** `fence.i` and the TLB flush act on the hart that runs the call, and
  >   every address-space switch flushes the whole TLB, so a process's translations live only on
  >   the harts running it now. Every call that clears or narrows one of a process's entries, or
  >   makes one executable, first shoots the process down on each other hart running it, which
  >   flushes, runs `fence.i` and acknowledges before the call goes on. So no stale translation
  >   reaches a page unmapped, lent or returned, or a frame's next owner. A missing `fence.i`
  >   cannot be seen on QEMU; `smp-fence` checks that it was taken, from the trace.
  Delete the bullet "A lend within one process": `smp-shootdown` attacks it.
- **memory-layout.md**, residual "Every change flushes everything". SMP1's "It flushes this hart
  only. Another hart holds a process's translations only while it runs that process, and a
  destruction shoots it down first ([memory](memory.md#residual-risks))." becomes:
  > It flushes this hart only. Another hart holds a process's translations only while it runs
  > that process, and every removal from the process, and its destruction, shoots it down there
  > first ([memory](memory.md#residual-risks)).
- **m2-usable-shell.md**, step 4: "An instruction fence goes to the harts running a process when
  a page of it becomes executable, and when a thread moves" becomes "An instruction fence goes
  to the harts running a process when a page of it becomes executable, and a hart fences before
  it runs a process; a thread that moves needs no fence of its own".
- **m2-usable-shell.md**, "Progress": after SMP1's sentence, add "One process's threads run on
  several harts at once, and every unmap, lend and return shoots the process down on the
  others (`smp-shootdown`, `smp-fence`)."
- **TENETS.md**, "Harts": leave it as it is. It already states the goal.

## Owned paths

- `kernel/src/sched.rs`: the pick predicate, the cursor, per-hart charging and the IPI on wake.
- `kernel/src/mem.rs` and `kernel/src/message.rs`: the `shootdown` calls at the sites above.
- The trace record and its writer.
- The cases above.
- `tools/testbench`: only the cases' verdicts that read the shootdown record.

**Hotspots:**
- K19, if the owner approves it, rewrites destruction's walks. Don't touch destruction's
  shootdown.
- B7 owns `tools/testbench/src/{main.rs,build.rs}`: ask before you change them.

## Gates

- The whole bench on both widths, at 1 hart and at `--smp 2`, each run alone.
- The kernel's and `libs/stride`'s host tests.
- `cargo fmt --check`.
- The unsafe ratchet: give each new `unsafe` site and its reason. Expect none.
- The size budget.
- doccheck.

Report each command with its exit code. Also report: the list of removal sites, `unmap`'s cost
before and after, and whether QEMU without `icount` showed a stale write under
`smp-no-shootdown`.

## Checkpoint

After the pick change (two threads of one budget seen on two harts in `smp-boot`'s trace), send
one progress line with the branch.
