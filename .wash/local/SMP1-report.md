# SMP1 report (smp1-implementer)

## Early checkpoint, 2026-10-05 (branch wp-SMP1 at a787bd3d2, nothing written yet)

### Step 1 edit list (every hart runs the scheduler at 2 harts, under the ticket lock)

- `kernel/src/cell.rs`: ticket lock (`next`, `serving`, AtomicU32, wrapping; Relaxed fetch_add,
  Acquire spin with `spin_loop()`, Release `serving + 1`; marked Zawrs hook). `KernelCell` keeps
  its RefCell borrow check under the lock; checked build asserts the current hart holds it.
  `smp` cfgs and the per-cell `SpinLock` go; doc comment rewritten (architect-15 note).
- `kernel/src/arch/riscv/hart.rs` (new; replaces `smp.rs`): the per-hart block (hart id, dense
  index, trap stack top, scratch word, context pointer, current pid/thread, kmain's saved
  context, eviction flag placeholder for step 7), `sscratch` points at it; HSM start of the
  others with the trampoline from the spike, a per-hart kmain stack and trap stack in `.bss`.
- `kernel/src/arch/riscv/asm.rs`: `_start_trap` takes `csrrw sp, sscratch, sp`, saves through
  the per-hart block's context pointer, switches to the per-hart trap stack, restores sscratch.
- `kernel/src/arch/riscv/irq.rs`: acquire the lock at entry from U-mode (after the save);
  S-mode entries (kmain's switch, idle window) already hold it.
- `kernel/src/arch/riscv/syscall.rs`: release just before `sret` to U-mode; `sret` to kmain
  (S-mode) keeps it.
- `kernel/src/arch/riscv/mod.rs` `idle()`: release, `wfi`, acquire, then the SIE window.
- `kernel/src/arch/riscv/process.rs`: `current_pid` and the context pointer per hart (`set_tid`
  writes the per-hart block); `PID_SLOTS.current` goes.
- `kernel/src/arch/riscv/mem.rs`: idle/blocked hart moves `satp` to PID 1 (rule 7 part needed
  for step 1 only: no hart's satp names a process it does not run).
- `kernel/src/time.rs`: the slice end per hart; each hart arms its own SBI timer.
- `kernel/src/sched.rs`: `user_since`/`billing`/`paused` per hart; the pick's one predicate
  "not running on another hart"; reschedule IPI to an idle hart on a wake (step 4 part).
- `kernel/src/main.rs`: `kmain` per hart; secondaries enter it after boot.
- `kernel/src/args.rs` + loader argument block: hart ids from the device tree.
- `kernel/Cargo.toml`: `smp` feature removed; `sched-test-and-set-entry` added (later step).
- `tests/smp-boot.toml` (+ program), replacing `tests/smp-spike.toml`.

### Contradictions / gaps (blocking questions)

Q1. **libs/stride's `Cpu` is one runner.** `Cpu { q, cur, pending }`: `bill`, `settle`,
`switch`, `reconcile(running = cur)`, `destroy` all assume one `cur`. The brief says "none of
the arithmetic changes" and owns only `kernel/src/sched.rs`. Several harts need a `cur` and
`pending` per hart, `reconcile` given the set of running budgets, and the pick's predicate.
Options:
 (a) `libs/stride` splits the per-CPU part: `Cpu<B,N>` -> a `Queue` plus `[Runner{cur,pending};
     H]`, methods taking a hart index; the one-runner API (H = 1) unchanged, so
     `the_crate_and_the_model_agree` and the model stay as they are. **Recommended.**
 (b) the kernel swaps `cur/pending` in and out of the one `Cpu` per hart at lock acquire and
     release; libs/stride untouched, but `reconcile` then protects only this hart's runner, and
     a budget running elsewhere that "lost" its last ready thread leaves the queue while it runs.
Needs libs/stride as an owned path for (a).

Q2. **The trap entry's context pointer and kmain's context are per process, not per hart.**
`_start_trap` saves through `PROCESS_AREA` header slot 1, and kmain (PID 1) saves into PID 1's
header `no_thread`; every idle hart shares PID 1's header (slot 1, `hardware_thread`,
`no_thread`). memory-layout.md "Per-process kernel data" states slot 1 is the context address.
Proposal: the context pointer, kmain's saved context and the current PID/TID move to the per-hart
block (rule 3's "its current thread"), reached through `sscratch`; header slot 1 and `no_thread`
go (SMP3 needs it per hart anyway). That is a page edit to memory-layout.md "Per-process kernel
data" and `satp` (`current_pid` is "the kernel's one record"), not in the brief's page lines.

Q3 (minor). The case `smp-boot` at 2 harts can be run with the existing per-case `smp = [2]`;
`--smp N` touches `tools/testbench/src/main.rs` (B7's). I will ask before that, at step 3.

## Step 1 checkpoint, 2026-10-05 (wp-SMP1 at 5a8a5496d, on a787bd3d2)

Commits: 91e76274b stride (running set, pick exclusion, `Harts`/`Runner`, `Cpu` the 1-hart
wrapper); a77d4bfbb layout+loader (`HART_STACKS` per Q4, slot 0 the boot hart's; `Hart`
argument; stale link.x asserts removed, CRLF kept); 5a8a5496d kernel (per-hart block, trap
entry via sscratch, ticket lock, HSM start, per-hart runner/billing/slice/timer, reschedule
IPI, PID 1 always Running, `smp` feature/spike/per-cell spinlock deleted; smp-boot case;
boot.md, TENETS.md, memory-layout.md pages).

Commands and exit codes (all through the pool):
- `cargo test -p redoubt-stride`: 0 (19 unit + 2 differential).
- `cargo test -p loader -p redoubt-layout`: 0 (loader 11 = 7 old + 4 new; layout 4).
- `cargo check -p redoubt-kernel --features qemu-virt`, riscv64gc and riscv32imac (+loader): 0.
- `make -f jobs.mk rv32/smp-boot rv64/smp-boot`: 0. PASS rv32 smp=2, smp=4; rv64 smp=2, smp=4.
  Logs: "harts: N of N ran user code"; "kernel lock: most waited 1 section(s), 2 hart(s)" and
  "3 section(s), 4 hart(s)" on both widths. Spinner counts ~2x at 4 harts vs 2 (parallel).

Not run yet: whole bench (1 hart, --smp 2), unsafe ratchet, size budget, doccheck, fmt gate
case (formatted with nightly rustfmt per CONTRIBUTING; `git diff --check` clean), the
`sched-test-and-set-entry` recorded negative, kernel `Sync` bound review.

Deviations / notes for review:
- Lock API is acquire/release, not `with(f)`: held from trap entry to sret, not lexical.
- `zihintpause` not enabled: only knob is the shared bare-metal rustflags (.cargo/config.toml),
  not owned; `pause()` is `spin_loop()` with the Zawrs hook comment.
- `kernel_frame` (mem.rs, hotspot) un-gated from `sched-trace` for the hart stacks.
- `marks.rs` and `src/tests.rs` in libs/stride changed with the running predicate.
- R78 page edits wait for the rebase (R78 is on main 6d0ce2090, not on this base).
- Steps still to come: 7 (stale mask, shootdown at destruction, satp to PID 1 on idle,
  per-hart audit log), 8 (trace hart field, oracle), smp-evict, --smp N (ask B7), pinned cases,
  remaining page lines (scheduling.md, memory.md, memory-layout residual, m2 progress).
- Follow-up for SMP2 (per the Q1 ruling): the model's per-hart current, set reconcile,
  exclusion pick and an H-hart differential against `Harts`.

## R78 mentions to restore at the rebase (R78 is on main, not on a787bd3d2; doccheck C11)
kernel/Cargo.toml (sched-test-and-set-entry comment), tests/smp-boot.toml description,
kernel/src/cell.rs (module doc "kernel/scheduling.md, R78", TicketLock doc, assert comment and
message), tests/programs/src/bin/smp-boot.rs doc, kernel/src/arch/riscv/hart.rs report() doc.

## Step 7 checkpoint, 2026-10-05 (wp-SMP1 at c56a085f4)

Commit c56a085f4 (step 7 + red round-1 P2s + the step-8 kernel half + budgets). All through the pool:
- smp-evict + smp-evict-mttcg: PASS rv64 and rv32 (2 harts). Log: "shootdown: PID 375 stopped on
  hart(s) 0b1 before any of its frames is freed"; checker's 512 pages zero; writer killed; P's read
  of its page unmapped on another hart faulted; Q's page kept its marker.
- smp-boot: PASS rv32 and rv64 at 2 and 4 (kernel now prints "harts: all N in the tree ran user
  code", or "harts: FAIL ..." which is forbidden: red P2 4).
- Recorded negatives (temporary case files, removed; consoles in .wash/local/smp1-evidence/):
  sched-test-and-set-entry: smp-boot FAILS rv32/rv64 at 2 and 4, panic at cell.rs:124 (the FIFO
  assert); smp-no-stale-mask: smp-evict FAILS 4/4 at mem.rs:233 (stale-mask debt audit);
  smp-no-evict: smp-evict FAILS 4/4 at hart.rs:267 (audit_left: a hart still runs the PID as its
  frames are about to be freed). The checker itself never saw stale writes under smp-no-evict: the
  writer's hart traps at its slice end (<= 1 ms) before the checker reads, so the verdict is the
  kernel's audit reading the harts' blocks, independent of the shootdown's own record.
- size-budget PASS, unsafe-budget PASS, docs PASS (after removing R78 mentions; see above).
- Unsafe: Sv39/SBI/PLIC 13 (=), RISC-V arch 13 -> 11, core 18 -> 16.
- Size: kernel 8823 -> 9174, loader 868 -> 913, layout 83 -> 94, stride 522 -> 650.

QEMU (10.2.1): `-icount` with `-accel tcg,thread=multi` is refused ("No MTTCG when icount is
enabled"); with icount the harts round-robin in one host thread; smp-evict-mttcg runs without
icount, QEMU's default MTTCG for riscv guests.

Kernel-half mappings removed after boot: none (the DMA register window and the hart stacks are
mapped at boot and never unmapped), so `shootdown` has no every-hart caller.

Red P2s: (3) checked builds draw, read and release `serving` SeqCst: the count can only
under-count, never over; (4) the kernel judges ran == started == found; (5) kernel README says a
release build does not check the lock in KernelCell::with.

Lock shape, acquire/release not with(f): the lock is taken in the trap handler and released in the
diverging resume paths (`-> !`, the asm `sret`), and kmain holds it across its loop and its S-mode
ecall into the trap handler; no lexical scope spans a hold, so a closure cannot express it.

zihintpause proposal (one line, shared .cargo/config.toml, every bare-metal crate):
`[target.'cfg(target_os = "none")'] rustflags = ["--cfg", 'getrandom_backend="custom"', "-C", "target-feature=+zihintpause"]`
(a FENCE hint; a no-op on a core without it).

Step 8: each trace record's kind word carries the hart index above the kind byte (0 on one hart).
The dump and the oracle are unchanged: the oracle requires exactly 5 fields and is off-limits until
SCHED1 merges; printing the hart and the oracle's one-hart judgement follow the rebase.

Remaining: rebase; R78 turned built (page, register row, R78 mentions restored); step 8 oracle side;
--smp N (ask B7); pinned cases; whole bench at 1 and --smp 2 both widths; final logical commits
(the loader/layout/stride ceilings and red P2s belong in their owning commits).

## Step 8 + red round 2 checkpoint, 2026-10-06 (wp-SMP1 at HEAD, after 54ef72d65 amended)

Red 2 P1 (irq.rs): the trap handler tests the hart's own mark (`hart::shot_down`: block.left ==
block.pid, set by serve, cleared by set_pid), never the process table; a checked build asserts a
shot-down hart never resumes or returns to user mode (syscall.rs resume, irq.rs return_registers).
The ack-then-PID-reuse interleaving cannot be driven from a guest; both smp-evict case files say
so. P2 2: the cases and program say the recorded negatives' verdict is the kernel's audit (QEMU
empties a hart's TLB at every satp write; the writer's hart traps at its slice end before the
checker reads). P2 3: rv64 stale-mask consoles added (smpneg-stale{,-mttcg}-rv64-smp2.log, panic
at mem.rs:233, the debt audit); evidence now holds rv32 and rv64 for all three negatives' runs.
P2 4: smp-evict-mttcg's description fixed.

Reruns through the pool (all PASS): smp-evict and smp-evict-mttcg rv64 and rv32; smp-boot rv64
and rv32 at 2 and 4. size-budget PASS (kernel 9174 -> 9184, line in the commit), unsafe PASS,
docs PASS.

smp-evict numbers (2 harts, consoles in .wash/local/smp1-evidence/):
| run | iterations/ms | shootdown line | checker |
| rv64 icount | 20241 | PID 361 stopped on hart(s) 0b1 | 512 pages zero, 0 words |
| rv64 MTTCG | 511170 | PID 134 stopped on hart(s) 0b10 | 512 pages zero, 0 words |
| rv32 icount | 6850 | PID 273 stopped on hart(s) 0b10 | 512 pages zero, 0 words |
| rv32 MTTCG | 362557 | PID 86 stopped on hart(s) 0b1 | 512 pages zero, 0 words |
Each also: writer killed with B; P's read of its page unmapped on another hart faulted; Q's page
intact.

Step 8: in the tree, each trace record's kind word carries its hart (c56a085f4). Staged, not
committed (the oracle is SCHED1's until the merge): .wash/local/SMP1-step8-dump-oracle.patch:
the dump prints the hart as a sixth field; the oracle reads 5 or 6 fields and refuses a trace
another hart wrote ("the oracle judges one-hart runs"); a host test for both. With it applied,
`cargo test -p testbench sched_oracle`: 32 passed.

sched-ties: SCHED1's case, failing on their tree at a787bd3d2 (clause 2), reported; not SMP1
evidence.

## R78 and the logical-commit fold, 2026-10-06 (wp-SMP1 at 5f4772dc4, on a787bd3d2)

Commits: 2de9264bc stride; d1ad50099 layout+loader; 46b1896d5 kernel (every hart, one lock,
shootdown, stale mask, red-2 own mark, smp-boot/smp-evict, pages); 5f4772dc4 R78 (checked FIFO
count + assert, most-waited line, sched-test-and-set-entry, Zawrs stub `wait_for_change`, the
Architect's section + diagnostic clause + residual + SECURITY row). Pre-fold head kept locally as
`smp1-prefold` (979429f49); fold differs from it only by the R78 text/mentions, the kernel
ceiling (9184 -> 9189 for the stub) and red-3 note 1's reword.

At the tip, via the pool: smp-evict + smp-evict-mttcg PASS rv64/rv32; smp-boot PASS rv64/rv32 at
2 and 4; size-budget PASS; unsafe-budget PASS; doccheck PASS (`jobserver bounded cargo test -p
redoubt-doccheck --test docs`, since alone cases are held).

Deviation from the Architect's text: the SECURITY.md row's status is `built`, not `built, tested`
(doccheck C7: the register writes a fully tested row as `built`).

Red round 3: note 1 reworded in both smp-evict tomls ('not driven by a case (it needs 3+ harts and
a creation queued ahead of the shot-down hart)'), not driven: a 3-hart fixture is a follow-up.
Note 2 (retry_reset skipped on the shot-down path) left: harmless.

## Rebase onto main 777bba164 (SCHED1 merge), 2026-10-06: wp-SMP1 = 402c2a10a

`git rebase --signoff --onto 777bba164 a787bd3d2 wp-SMP1`: 3fdc90825 stride; 072f54d7d
layout+loader; a93fc9636 kernel; 402c2a10a R78. Stride and layout/loader applied clean.
Hunks: docs/kernel/README.md (kernel commit), 1 hunk: kept main's new size bullet ("The table's
inclusive line counts are snapshots") and SMP1's "Several harts, one lock" (replacing "One hart").
docs/kernel/scheduling.md (R78 commit), 3 hunks: status planned -> `built · tested:
bench:smp-boot`; section body -> the Architect's built text (main's **Open:** line dropped); the
diagnostic clause -> built form. docs/SECURITY.md, 1 hunk: the R78 row, built side (status
`built`). Resolved per hunk (SCHED1's other paragraphs kept); R78 section and its residual once
each. Everything else, kernel sources included, auto-merged. size-budget kernel 9190 measured
exactly 9190 (main 8823). No manifest entries needed (test programs are not in
image/manifest.json).

Gate via the pool: build-rv64 0, build-rv32 0; host (bounded) stride 19 + 2 differential, layout
4, loader 11; docs, formatting, size-budget, unsafe-budget, no-cruft PASS; smp-boot rv64/rv32 at
2 and 4 PASS; smp-evict + smp-evict-mttcg rv64/rv32 PASS; sched-test-and-set-entry negative FAILS
as required at cell.rs:124 (the FIFO assert) rv64/rv32 at 2 and 4; rv64 sched-share,
sched-large-weight, sched-server-busy, sched-carve-inflation, sched-wake-no-preempt PASS; smoke
userland-boot, init-boot, ipc-outcomes, bench-net-peer (+ its 3 siblings by substring) PASS on
both widths. Temporary negative case file removed; tree clean.
