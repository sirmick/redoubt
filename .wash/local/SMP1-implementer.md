# SMP1: the kernel runs on every hart, under one big lock

Tier A (the kernel). Size L. It needs K16, K21, IPC3, K22 and ASID1 merged, and the owner's answers on QA
`SMP1-design`.

**The owner's answers.** One process's threads will run on several harts at once, so a beamlet
VM's schedulers run in parallel. It is designed for that now and built in three packages:
- SMP1 (this one): the harts, the lock, and the shootdown built for a set of harts. A budget
  runs on one hart at a time, as an interim step.
- SMP3: a budget runs on several harts.
- SMP2: R12 and the targets across harts. The targets are gated at 1 and 2 harts, and 4 is
  recorded.

Nothing SMP1 builds is removed later. Write the one-hart pick as one rule that SMP3 lifts, and
the shootdown so that SMP3 only adds callers. ASID1 (merged before this) made each PID its process's ASID: a switch flushes nothing, and
every flush is by address and ASID with the kernel's global entries standing. Item 7 is written
for that kernel. Don't start until
the node's needs are met.

Run every cargo and bench command as `/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from
the worktree.

## Context rules (read these first; context ran out four times on INIT2)

- **Don't read whole files.** Run `grep -n`, then Read a range.
  - The kernel's big files (`message.rs`, `budget.rs`, `sched.rs` and `mem.rs`) are each over
    1,000 lines. Read only the functions you change.
  - The trap entry is `kernel/src/arch/riscv/asm.rs` and `trap*.rs`.
  - The spike is `kernel/src/arch/riscv/smp.rs` (200 lines); read it whole, once.
- **Don't open `.wash/qa/*.md` or other packages' reports.** This brief holds the rulings. If
  you must open a QA file, read it only up to its checkpoint comment:
  `sed '/wash-qa-checkpoint/q'`.
- **Pipe bench output.** Use `cargo testbench --list | awk '{print $1}'`. Read boot logs only
  through `grep` or `tail`: they begin with hex dumps.
- **One whole bench at a time.** Run sweeps in parallel, never two whole benches.
- **Read a file right before you Write it,** and prefer Edit.
- **Keep reports under 1900 bytes,** with detail in `.wash/local/SMP1-report.md`.
- **If you hand off, keep the handoff short** and end it with "what consumed my context". Your
  successor reads that handoff and this brief, not the reading list again.

## Reading list (only these)

- `docs/plan/m2-usable-shell.md`: "Several harts".
- `docs/kernel/scheduling.md`: "One flat stride queue", "Preemption points", and R12's first two
  paragraphs.
- `docs/kernel/memory.md`: "Instruction fetch after mapping" and the residual-risk bullet "One
  hart".
- `docs/kernel/memory-layout.md`: "`satp`" and "Per-process kernel data", as K16 left them.
- `docs/kernel/boot.md`: the paragraph "The other harts stay parked…".
- `kernel/src/cell.rs` (89 lines) and `kernel/src/arch/riscv/smp.rs`.

## The design

### Scope: steps 1 to 3 of "Several harts", and the part of step 4 they need

User code runs on every hart. Step 5, finer locking, is later. R12's shares across harts, the
latency targets on several harts, and running every case at 2 and 4 harts are SMP2's.

1. **Any boot hart, any count up to `MAX_HARTS` (8).**
   - The kernel starts the other harts through SBI HSM. It finds their ids in the device tree,
     which the loader already reads, and the argument block carries them.
   - Nothing assumes hart 0. A hart past `MAX_HARTS` stays parked, and the kernel prints that
     once.
   - Hardware bounds (`.wash/local/hardware-bounds.md`): per-hart state is indexed by a dense
     boot index below `MAX_HARTS`, never by the hart id, which the platform may number sparsely
     and widely. At boot, the PLIC S-mode context of every hart started lies inside
     `KERNEL_PLIC_BASE`'s window, checked against the PLIC's reported size; a hart whose context
     does not is parked and reported like one past `MAX_HARTS`. A host test for each.
2. **One kernel, one build.** The `smp` feature goes, and so does the spike, `smp.rs`'s
   counter test. A one-hart boot runs the same code with the lock uncontended. Deleting the
   configuration is the rule.
3. **Per-hart state.**
   - A small per-hart block holds the hart's id, its trap stack, its current thread, and its
     eviction flag (rule 7). `sscratch` reaches it at trap entry.
   - K16 already puts each thread's saved registers in its own IPC page, so a context is never
     shared between harts.
4. **One big kernel lock.**
   - A ticket lock, so it is FIFO: a hart waits behind at most `MAX_HARTS` - 1 kernel sections,
     never for ever.
   - Every spin (for the lock, and the shootdown's wait for acknowledgements) runs the pause hint
     each time round: `core::hint::spin_loop()` with `zihintpause` enabled for the kernel's
     targets. Its encoding is a FENCE hint, which a core without the extension runs as a no-op.
     It costs nothing on QEMU, and on a core whose harts share issue slots it gives them to the
     sibling.
   - It is taken at trap entry, after the user registers are saved, and released just before
     `sret` and before an idle `wfi`.
   - Every `KernelCell` is reached only under it. `KernelCell` stays a run-time-checked cell, and
     a checked build asserts that the current hart holds the lock.
   - The spike's per-cell spinlock goes. Locks per global would nest (the process table inside
     the memory manager) and would need an order; one lock needs none.
5. **Interrupts.**
   - Device interrupts stay on the boot hart's PLIC context, as the loader already sets up.
   - Every hart takes its own timer and software interrupts.
   - Each hart arms its timer for the earlier of its slice's end and the earliest timeout, when
     it returns to user mode or idles. The timer queue is the one global queue.
6. **The scheduler: a budget runs on at most one hart at a time** (interim, until SMP3).
   - A hart picks the lowest-pass budget that is not running on another hart. Keep that
     condition one predicate in the pick, which SMP3 replaces with "has a runnable thread not
     running on any hart". Ties, the wake
     rule, charging and the floor are as today: the stride state is per budget, and a budget
     has one runner, so none of the arithmetic changes.
   - When a wake makes a budget runnable and a hart is idle, the waker sends it a reschedule
     IPI (SBI IPI).
   - A wake never preempts a running hart (as today: `R12PreemptOnWake`).
7. **The TLB and the instruction cache: ASIDs, the running set, and a stale mask.**
   - After ASID1 a process's PID is its ASID, a switch flushes nothing, and a hart keeps a
     process's translations across switches. So which harts *run* a process (exact: the per-hart
     current process) and which may still *hold* its translations (any hart that ever ran it)
     differ. The shootdown goes to the first set only; the second is handled lazily, with no IPI:
   - **The stale mask.** Per PID, one byte (`MAX_HARTS` 8 bits): the harts that must flush that
     ASID, `(x0, pid)`, before they next install it. A hart about to write a process's `satp`
     checks its bit; if set, it flushes the ASID and clears the bit. Whenever a hart removes or
     narrows a mapping in a process's tables, it flushes locally (ASID1's rule) and sets every
     other hart's bit. When a PID is given out, the allocating hart flushes locally (ASID1's rule
     6) and sets every other hart's bit; a destruction sets every hart's bit. On one hart the
     mask is always empty.
   - A hart that stops running a process (block or idle) moves `satp` to the kernel's own space
     (PID 1), with no flush, and runs `fence.i` before it runs a process. Switching straight to
     another process writes that one's `satp`. So `satp` names a process on a hart only while the
     hart runs it, and no hart's walker reads a dying process's tables (ASID1's rule 7).
   - "On a hart" means in that hart's view. Two harts that share one TLB (a barrel core) keep
     their translations apart by hart and ASID, as the privileged spec requires, and a hart's
     flush that also empties its sibling's entries costs time, never correctness. So none of
     this changes there (SMP1-multihart.md).
   - Every call that removes a mapping removes one of the caller's own (`unmap`, a lend, a
     transfer, `process_map`'s move, a reply returning a lend), and the caller's budget runs on
     no other hart. The local flush plus the stale mask is enough.
   - Adding a mapping into another process (a lend arriving, a reply, a transfer taken,
     `process_map` into a child) needs no remote flush. A hart that faults on a translation
     already valid flushes that address in that ASID and retries the instruction.
   - **The shootdown, built for a set of harts.** One routine, `shootdown(target)`, where the
     target is a process or, for kernel-half mappings, every hart:
     1. the calling hart (holding the lock) sets a flush request on every *other* hart whose
        per-hart block names the target as its current process (every other hart, for the
        kernel half), and sends each an IPI;
     2. each such hart, at trap entry, before it takes the lock (and also while it spins for
        it), switches `satp` to the kernel's own space, flushes the target's ASID `(x0, pid)`
        (for the kernel half, the address in every ASID, `(addr, x0)`), runs `fence.i`, and
        acknowledges. On its way back to user mode, once it holds the lock, it installs `satp`
        again for whatever it then runs, checking the stale mask as any install does;
     3. the caller waits for every acknowledgement, under the lock. No target needs the lock to
        acknowledge, so there is no deadlock.
   - **SMP1's one caller: a destruction.** A dying process running on another hart is shot
     down before any of its frames is freed. That hart then finds its thread dead once it holds
     the lock, picks again, and never touches the process's memory again. Harts that ran it
     earlier hold only translations under its ASID, which no `satp` names until the PID is given
     out again, and the stale mask flushes it first on each. In SMP1 the set has at most one
     hart; the routine must not assume so. SMP3 adds the other callers: `unmap`, a lend's end, a
     transfer, `process_map`'s move, and a reply returning a lend.
   - The per-hart block's current process is the one source of the set. Keep it exact: set it
     before `satp` is written, and clear it after `satp` moves away.
   - **Why this is load-bearing:** K21's free-frame bitmap writes nothing into a freed frame,
     so a stale translation cannot reach the kernel. But it would let the dying process, or the
     next process given its PID, read or write the frame's next owner's data, across budgets and
     labels.
   - **Kernel-half mappings** removed after boot (the process area and any per-process kernel
     window) are flushed on every hart before their frame is reused, by `shootdown` with every
     hart as the target. List every such site in the report. If there are none, say so.
   - **The audit.** ASID1's checked-build log of unflushed changes is per hart and stays; it
     gains one check: a hart installing a `satp` whose PID has its stale bit set has flushed
     that ASID first.
8. **The trace ring.**
   - One ring, written only under the lock, so there is still one writer at a time.
   - Each record gains its hart's id. The bench's oracle reads it and, until SMP2, judges only
     one-hart runs.
9. **What K19 is not.** Under one lock a destruction runs whole, and no other hart can observe it
   half done. So K19 ("pumps at the boundary") is not SMP1's need. It becomes one before step 5.

### The bench

- **`cargo testbench --smp N`** runs every boot case at N harts. Today a case without `smp` runs
  at 1.
- A case that measures R12's shares or the latency targets pins `smp = [1]` until SMP2. List
  the cases you pin.
- `icount` with several harts runs them round-robin in one host thread, which is deterministic.
  The eviction case also runs without `icount` (multi-threaded TCG), for real parallel
  interleavings. Check what QEMU does, and say so.

### The cases (both widths, in a checked build)

1. **`smp-boot`**, at 2 and 4 harts.
   - Every hart reaches the scheduler and runs user threads: one spinner per hart, each
     reporting its hart.
   - It replaces `smp-spike`.
2. **`smp-evict`** (the shootdown at a destruction), at 2 harts, with `icount` and without.
   - A spinner in budget B writes its own page without end. Another hart destroys B, then a
     checker in a new budget maps pages until it gets B's freed frame (or many pages), and
     watches them stay zero.
   - The checked build's free-list audit passes.
   - A recorded negative, the feature `smp-no-evict` (off in every default build, as
     `alloc-first-fit` is), must fail: either the checker sees the stale writes or the audit
     trips.
   - **The stale mask.** A process maps and touches a page on one hart, then (its budget moved)
     unmaps it on the other, and moves back and reads the address: a fault, never the old frame.
     The recorded negative `smp-no-stale-mask` must fail, by the read or by the audit's stale
     check. If QEMU empties a hart's TLB on every `satp` write (ASID1 reports what it found), it
     will be the audit: say which.
3. **The whole bench at `--smp 2`**, apart from the pinned cases, on both widths. Report what
   passed. A failure is a finding to fix, never a case to pin.
4. **A lock-held assertion.** In a checked build, every `KernelCell` access asserts that the
   current hart holds the lock. Running cases 1 to 3 is its test.

The model is unchanged: it is one hart, and the scheduler's per-budget rules are unchanged. Say
so in the report.

### Pages (exact lines)

Read each anchor sentence right before you edit; if it has moved (K16 and K21 edit these
pages), ask me rather than guess.
- **scheduling.md**, "One flat stride queue". After "So over any stretch in which budgets stay
  runnable, each gets CPU in proportion to its weight.", add:
  > On several harts each hart picks the lowest-pass budget not running on another, so a budget
  > runs on at most one hart at a time and its stride state has one runner.
- **scheduling.md**, the residual "Measured on QEMU, on one hart": "The queue and its accounting
  drive one hart until M2 (usable shell)" becomes "The queue and its accounting are judged on
  one hart until R12 is restated across harts".
- **memory.md**, the residual "One hart" becomes:
  > - **Several harts.** `fence.i` and the TLB flush act on the hart that runs the call. A
  >   process's translations carry its ASID, and a hart that ran it may keep them; it flushes
  >   that ASID before it next runs the process if any of its mappings were removed meanwhile or
  >   its PID was given out again. A budget runs on one hart at a time, so a process's own unmap,
  >   lend or reply needs no other hart's flush. A destruction first shoots the process down on
  >   any other hart running it, which flushes its ASID and acknowledges before any of its frames
  >   is freed, so a stale translation never reaches the frame's next owner. One process on several
  >   harts at once needs that shootdown at every unmap, lend
  >   and reply (M2 (usable shell): [several harts](../plan/m2-usable-shell.md#several-harts)).
- **memory-layout.md**, ASID1's residual "A flush acts on this hart only" becomes:
  > - **A flush acts on this hart only.** Each change flushes its address or its ASID on the hart
  >   that makes it. Another hart that ran the process flushes that ASID before it runs the
  >   process again, and one running it now is shot down first at a destruction
  >   ([memory](memory.md#residual-risks)).
- **boot.md**: "The other harts stay parked in the firmware until M2 (usable shell) (...)."
  becomes:
  > The kernel starts the other harts, up to 8, through SBI's hart state management; a hart past
  > the eighth stays parked, and the kernel says so once
  > ([several harts](../plan/m2-usable-shell.md#several-harts)).
- **TENETS.md**, "Harts": "Through M1 (separation and containment) Redoubt runs on one hart; the
  others stay parked in the firmware. A checked-build case starts a second hart to test the
  kernel lock, and a few cases boot with two or four harts to show the extra harts change
  nothing." becomes:
  > The kernel runs user code on every hart, up to 8, under one big lock, with a budget on one
  > hart at a time until one process's threads may run on several.
  Keep the rest of the bullet.
- **m2-usable-shell.md**, "Progress": replace the spike's sentence ("For several harts: a
  two-hart spike, ...") with what SMP1 built, in one sentence: every hart runs user code under
  one FIFO lock, and a destruction's shootdown is attacked (`smp-boot`, `smp-evict`).

## Owned paths

- `kernel/src/arch/riscv/**` (trap entry, the per-hart block, HSM, IPIs) and `kernel/src/cell.rs`.
- `kernel/src/sched.rs` (the pick, the reschedule IPI, the hart field in the trace) and the
  eviction in the destruction path. That is a hook, not a rewrite: K19 may follow.
- The timer arming.
- The loader's argument block, for the hart ids.
- `tools/testbench`: `--smp`, and the oracle's hart field.
- The cases above.

**Hotspots:**
- K16, K21, IPC3, K22 and ASID1 are merged before you start. K22 changed `sched.rs`'s reconcile to
  follow marked budgets; your pick predicate and IPI sit beside it. IPC3 owns `message.rs` (delivery and
  destruction's message reach walk per-endpoint lists); keep the eviction hook out of it.
- If K19 is approved, it rewrites the destruction's walks. Keep the eviction hook small, and
  rebase after whichever of you lands first.
- B7 owns `tools/testbench/src/{main.rs,build.rs}`: ask before you change them.

## Gates

- The whole bench on both widths, at 1 hart and at `--smp 2`, each run alone.
- The kernel's host tests.
- `cargo fmt --check`.
- The unsafe ratchet: the per-hart block and the lock add `unsafe`, so give each site and its
  reason.
- The size budget.
- doccheck.

Report each command with its exit code. The report lists what was deleted (the `smp` feature,
the spike and the per-cell spinlock).

## Checkpoint

After step 1 (every hart running the scheduler at 2 harts, `smp-boot` green on one width), send
one progress line with the branch.
