# BEAM18 design checkpoint: a session's VM runs N schedulers on N harts

Base main 050357e8a, branch wp-BEAM18 (no code yet). Author: beam18-implementer, 2026-10-09.

## What exists already

The VM is already built for several schedulers, behind its `std` feature, and runs so on the host
(`beamlet --schedulers N`, `$BEAMLET_SCHEDULERS`; `prime_smp.erl`, `test_system_flag.erl`):

- `vm/src/sync.rs`: `Lock<T>` (a `std::sync::Mutex`, owner-checked) and `Wakeup` (a `Condvar`);
  without `std`, `Lock` is a `RefCell` and `Wakeup` does nothing.
- `vm/src/vm.rs`: `Vm::run` starts `schedulers - 1` helpers with `std::thread::scope`; each runs
  `schedule` = `lock().next()` (housekeeping, pop the run queue), the slice **with the system
  unlocked** (`interp::run` on the process taken out of the table), `lock().finish()`. A
  scheduler with nothing to run while others run sleeps on `Wakeup`; only one with nothing
  running anywhere (`running == 0`) calls `platform.idle`. `hold_back` keeps a spawned child off
  the queue until its parent's slice ends. `schedulers_online` parks helpers.
- `vm/src/sched.rs`: per-scheduler caches (atoms, limits, literal chunks, resolved calls) checked
  against `Generations` atomics, so most instructions take no lock.

So BEAM18 is mostly: give that mode a lock, a wakeup and threads on Redoubt, which has no `std`,
no futex and no thread-locals, then start it in a session and measure it.

## 1. Scheduler threads and N

- **Threads:** the first scheduler is beamlet's main thread, as now; N-1 helpers from
  `redoubt_rt::thread::spawn`, the way the hub's waiters, `resize` and `pool` threads are made,
  each with the session VM's first-thread stack size (18 pages, `SESSION_STACK_PAGES`) and its
  park endpoint. Waiters stay one per connection, unchanged.
- **N: question for you.** The kernel reports no hart count to userland (no call, nothing in the
  startup block, which parents write). Options:
  - **(a) recommended: an argument `schedulers=N`** (default 1), which the steward passes to a
    session's VM from its config; the image sets 2. No kernel change. At 1 hart the two threads
    share the hart; the helper is parked unless two processes are runnable, so a single
    process's loop pays only an uncontended atomic lock per slice. The 1-hart cost is gated
    (case 2 below).
  - (b) a kernel call (or argument-block field forwarded by init and the steward) giving the
    hart count, N = min(harts, a cap): a kernel/ABI change for kernel-red, plus the docs and
    model rows. Better long-term, not needed to measure throughput now.
  Either way N is capped (8) and fails closed to 1 on a malformed argument, as `budget_pages=`.

## 2. Run queue

Keep the **one shared queue under the system lock** that the VM already has and tests on the
host. A slice is 2,000 reductions (about 40 ms of guest time at the measured rate), and the lock
is taken twice per slice plus for sends, natives that touch the system, and timers, so for a
compute workload contention is small. Per-scheduler queues with stealing only if the case shows
the lock is the limit; I'd report the lock's hold share from the case rather than build it now.

## 3. What is shared and how it is guarded

- **Under the system lock (`Lock<System>`):** the run queue, the process table (a running
  process is out of it, owned by its scheduler), inboxes of running processes, exit signals
  (delivered at the target's slice end), the atom table, the module table and code loading,
  ETS and its word total, `persistent_term`, the registry, timers, ports, files table, stats.
- **Read without the lock:** loaded `Module`s (leaked, immutable), literal chunks (`Arc`,
  immutable), the `Generations` counters (atomics).
- **Per scheduler:** its caches (above) and the process it runs: heap, stack, mailbox, GC. The
  collector runs on the process's own scheduler and touches nothing shared but refcounted
  binaries and resources (`Arc`, so atomic counts in the threaded build; resources are
  `Send + Sync` there, as `assert_thread_safe` already checks under `std`).
- **The platform (`Arc<Lock<Box<dyn Platform>>>`), with the hub inside it:** always taken after
  the system lock, never before (the rule in `sched.rs`). Only a scheduler with nothing running
  anywhere enters `idle`, so one thread at a time receives on the VM's endpoint; requests go out
  from whichever scheduler holds the platform, as the page already says. The platform holds no
  `Rc`/`RefCell`; it must become `Send`, which the compiler checks.
- **Not per scheduler, said as residuals:** code loading reads the system volume holding the
  system lock (every scheduler waits for a module load, as today the one does); a typed call the
  platform makes in place (`size`, `rename`) holds the platform lock; the runtime heap is one spin
  lock (`libs/rt/src/heap.rs`), so two schedulers allocating contend there, and a holder preempted
  by the kernel makes the other spin. Measured by the case; left unless it shows.

## 4. The lock and wakeup on Redoubt (the new code)

The VM is `forbid(unsafe_code)`, so the primitives live in `redoubt-rt` (Tier A runtime, its
unsafe budget), and the VM gets a feature `redoubt` (beside `std`; internally `cfg(threads)` =
either) whose `sync.rs` backend uses them:

- `redoubt_rt::sync::Mutex<T>`: an atomic lock word and a list of waiters; spin briefly, then
  park. Parking is needed, not polling: one scheduler idles in `platform.idle` holding the
  system lock for as long as the prompt waits, and another woken at that moment must sleep, not
  poll the lock.
- `redoubt_rt::sync::Condvar` for `Wakeup`.
- **Parking without futex or thread-locals:** each thread rt starts gets a parker (an endpoint
  of its own and an atomic state: idle / notified / parked). `unpark` sets notified and, only if
  the thread said it is parked, `send`s one empty message, which the parked thread takes at once
  (the send blocks only for the moment between its saying so and its `receive`). A thread finds
  its own parker by the address of a local: the stack range it was given at `spawn` (rt records
  it), else the main thread's. Safe Rust, no asm.
- `redoubt_rt::thread::scope` (spawn borrowing, joined before return), so `Vm::run` keeps its
  one shape for `std` and `redoubt`. Stacks are never freed, as `spawn` says; `run` is called once.
- Host tests on the fake kernel: exclusion under contention, no lost wakeup in the park/unpark
  race, a condvar wait/notify storm, a thread finds its own parker.

## 5. Reductions and yields

Unchanged per process: a process yields after 2,000 reductions or 200,000 instructions, on
whichever scheduler runs it; the kernel's time slice preempts each scheduler thread whatever it
runs. Natives are still not preempted (`modpow` residual), but now block only their own
scheduler. `erlang:system_info(schedulers)` reports N; `schedulers_online` works as on the host.

## 6. Budgets and limits

Nothing new is carved. The threads are the session process's threads, in the session's one
budget: the kernel bills each hart's runner to that budget's one pass (scheduling.md, "One flat
stride queue"), so the session's share among budgets is by weight as before, and on an otherwise
idle machine it may use as many harts as it has runnable schedulers. Each helper costs its stack
(18 pages), its thread page and its park endpoint's page, from the session's budget; I report the
change to beamlet-footprint's peak and the session's process count (unchanged: threads, not
processes; 255 threads a process). The heap limit (a sixteenth of `budget_pages`), the ETS and
`persistent_term` limits and the session's page cap are unchanged.

## 7. The cases

Under `icount` QEMU runs the harts in turn on one host thread, and guest time counts every hart's
instructions, so two harts cannot run faster in guest time (testbench.md). The throughput case
therefore runs on the host's clock with multi-threaded TCG, as `sched-lock-contention-4-mttcg`
does, and gates a ratio of two times taken in one boot (load-proof except for a stalled vCPU; a
verdict only alone, per testbench.md's rule):

1. **`beamlet-schedulers-mttcg`**, both widths, `smp = [2]`, `keep_smp`: the console session of
   the image's boot runs `Task.async` over 2 tasks, each `Enum.reduce(1..65536, 0, &max/2)`
   (the reduction-rate loop), awaited, 10 times with `schedulers_online` 1 and 10 with 2,
   alternated, and prints `ratio` = 100 x fastest(1) / fastest(2), and each rate in reductions a
   second. **Floor:** the slower width's measured ratio less a tenth, rounded down to 5, and not
   below 150 (1.5x); if I measure under 160 I report before setting it.
2. **`beamlet-schedulers-one-hart-mttcg`**, `smp = [1]`: the same lines; two schedulers on one
   hart lose at most a tenth: ratio at least 90.
   Together they give "2 harts against 1": the page records each boot's rates side by side
   (2 schedulers at 2 harts against 1 hart), not gated across boots.
3. **`beamlet-reduction-rate`** unchanged (icount, 1 hart, floor 42,000), and run at 2 harts.

Gates as the brief lists: beamlet host tests (q; plus the VM's tests under `--features redoubt`
on the host and the rt sync tests), `./test-shell`, difftest, the two new cases +
beamlet-reduction-rate + userland-boot + beamlet-footprint both widths at 1 and 2 harts, docs,
size-budget, unsafe-budget (rt grows), formatting.

## Pages

beamlet.md (beamlet on Redoubt: N schedulers, `schedulers=`, the I/O report's thread count;
Threads; the hub "one hub per VM" unchanged; the case's numbers), native.md (rt's sync and
`scope`), sessions.md (the VM's threads), steward.md (the argument), m2-usable-shell.md
(several harts progress), testbench.md (the mttcg list).

## Tier and review

Tier A: rt (runtime library) and the steward's launch argument; the VM change is Tier B code
under the same review. steward-red; kernel-red only if option (b).

## Size

M: rt sync ~250 lines with tests, VM backend ~60, platform `Send` + argument ~40, steward ~10,
two cases, pages.
