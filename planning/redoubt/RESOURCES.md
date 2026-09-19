# Resources: budgets, scheduling, clocks

Designed, not built. Owns: budgets, scheduling, clocks. Revocation by budget: CAPABILITIES.md.
Labels on budgets: CONTAINMENT.md.

## Where the kernel is today
- The memory manager records the owning process of every RAM page (`allocations` in `mem.rs`).
- Kernel objects live in fixed global tables: 128 server slots in total (`MAX_SERVER_COUNT`), a
  fixed process table, 32 threads per process. **One process can take every server slot**: a DoS.
- Scheduling is cooperative round-robin: no priorities, no time slices; a spinning process starves
  its turn forever.
- The hart timer is delivered to userspace as IRQ 0 and user mode reads `time` directly (TIMER.md).

## Budgets
Every process belongs to exactly one budget; every resource is charged to one. A budget is a kernel
object held by capability, with limits and usage for RAM pages (including page tables), handles,
threads, processes and endpoints, plus a CPU weight, a label set (CONTAINMENT.md) and an optional
deadline (a lease).

- **A tree with hard limits.** A child's limits are carved from its parent: the children's limits
  never add up to more than the parent's. An allocation succeeds or fails on the caller's own budget
  alone, never on a sibling's use. No overcommit.
- **Top level:** `root -> system [default 25% of RAM, set in the boot manifest] + users [the rest]`.
- **Bounded depth.** The tree has a maximum depth (scheduling walks it under the kernel lock).
- **Limits fail the caller**, with `OutOfMemory` (or the matching error). The kernel never panics and
  never kills an innocent process to make room.
- **Donor pays.** Moved memory stays charged to the sender (the page table records the paying budget
  per page, not only the owner), so a client cannot fill a server's budget by sending it pages.
  Servers limit per-client state by the caller's budget id (CONTAINMENT.md).
- **Kernel objects are charged.** A process's handle table lives in pages charged to its budget (as
  seL4). Global tables are sized from RAM at boot; each budget holds at most its limit of entries.
- **Every page is zeroed** before it is handed to a process; userspace never names physical RAM.
- **Destroying a budget** kills everything in it, revokes every handle stamped with it, and returns
  every resource. A lease is a budget whose deadline makes the kernel destroy it.
- **Usage is read with a syscall** by the budget's holder.

## Scheduling
Three problems: starvation, wake-up latency (drivers, shells), priority inversion.

### Two classes, strictly ordered
- **System:** driver threads and system servers. Only system-signed code; never granted to users.
  Runs before everything else. System code is TCB; if it spins, that is our bug and the bench tests it.
- **Everyone else:** users, agents, applications, sharing by weight.
No numeric priorities. Real-time guarantees are a non-goal until something needs them.

### Hierarchical stride scheduling by budget
- Each budget has a weight and a *pass*. The scheduler runs the runnable budget with the lowest pass.
- **Charge actual runtime at every deschedule:** pass += runtime x `STRIDE / weight`. A thread that
  runs briefly and sleeps is still charged for what it ran.
- **On wake,** pass = max(own pass, current minimum). A sleeper cannot bank credit, and still runs
  promptly.
- Hierarchical: choose a budget at each level of the tree, then a thread inside it. An agent competes
  for its sponsor's share, never for anyone else's.
- The kernel's timer ends a slice (10 ms to start); it is armed only when more than one thing is
  runnable.

### Deferred
- **Time donation** (a server running on the caller's budget during a blocking call) and **CPU
  quotas**. Donation is intricate (seL4's scheduling contexts), and a donated server thread stopped
  mid-call by the caller's quota or lease could hold server locks forever. Add only if measured
  priority inversion hurts. Until then servers pay for their own CPU, and leases are bounded by
  deadline and weight.

### SMP
One global run queue under the kernel lock first. On hardware threads that share a core, all threads
of a core run one budget or idle (PLATFORM-FPGA.md).

## Clocks
- **The kernel owns the hart timer.** It keeps one deadline queue (slice ends and sleepers) and
  programs SBI TIME or Sstc.
- **Blocking receive takes a timeout.** Sleeping is a receive with a deadline and nothing to receive.
  There is no timer server and no IRQ 0 timer.
- **User mode cannot read the `time` CSR** (`scounteren.TM` clear). User processes get monotonic
  time from the kernel at 1 ms resolution; the system class gets full resolution. This slows timing
  attacks; it does not stop them (a process can count in a loop), which is why cores are not shared
  between budgets and the rest is RTL.
- **Wall-clock time** (dates, time zones, NTP) is a userspace offset over monotonic time.

## Swap (later)
A userspace swapper server; a per-budget swap limit next to the RAM limit, so swapping never lets a
budget exceed its total; swapped pages encrypted and authenticated; the system budget never swapped.

## Attack tests the bench gains
- A spinning process cannot delay another budget beyond its fair share, including by sleeping
  briefly between bursts.
- A thread, endpoint or handle bomb hits its own limit; other budgets keep creating.
- A memory hog gets `OutOfMemory`; the system budget is untouched.
- Moving memory to a server does not exhaust the server's budget.
- A user process reading `time` traps; the kernel clock is 1 ms.
- Lease expiry reclaims everything; usage returns to zero.

## Kernel work order
1. Handles, endpoints and budgets together (the same kind of kernel object); per-page payer; charged
   kernel tables; label sets.
2. Kernel-owned timer, receive timeout, preemption with hierarchical stride; the two classes.
