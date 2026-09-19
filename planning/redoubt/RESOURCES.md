# Resources and scheduling: budgets, stride scheduling, donation

Status: agreed direction, 2026-09-18. Nothing here is built yet. Builds on CAPABILITIES.md.

## Where the kernel is today
- The memory manager records the owning process of every RAM page (`allocations` in `mem.rs`).
- Kernel objects live in fixed global tables: 128 server slots in total (`MAX_SERVER_COUNT`), a
  fixed process table, 32 threads per process. **One process can take every server slot**: a DoS.
- Scheduling is cooperative round-robin: no priorities, no time slices, a spinning process starves
  its turn forever.

## Budgets
Every process belongs to exactly one budget; every resource is charged to one. A budget is a kernel
object held by capability, with limits and usage for: RAM pages (including page tables), handles,
threads, processes, servers, and CPU.

- **A tree, cgroups-style.** A child's usage counts against every ancestor; an allocation fails if
  any ancestor is over its limit. Children may be promised more than the parent has (overcommit).
- **Except at the top: a hard split.** `root -> system [reserved, default 25% of RAM, set in the boot
  manifest] + users [the rest]`. Users can squeeze each other only inside the user share.
- **Limits fail the caller**, with `OutOfMemory` (or the matching error). The kernel never panics and
  never kills an innocent process to make room.
- **Donor pays.** Moved memory stays charged to the sender (the page table records the paying budget
  per page, not only the owner), so a client cannot fill a server's budget by sending it pages. A
  server allocating on a client's behalf pays itself and limits each client by badge; a shared
  library makes that the easy path.
- **Kernel objects are charged.** A process's handle table lives in pages charged to its budget (as
  seL4). Global tables are sized from RAM at boot; each budget holds at most its limit of entries.
- **Destroying a budget** kills everything in it and returns every resource. A lease is a budget with
  a deadline; revoking an agent or ending a session is destroying its budget.
- **Visible** as a small 9P tree to the holder (`/budget/.../usage`).

## Scheduling
Three problems to solve: starvation, wake-up latency (drivers, shells), priority inversion.

### A. Two classes, strictly ordered
- **System:** driver threads and system servers. Only system-signed code; never granted to users.
  Runs before everything else. System code is TCB; if it spins, that is our bug and the bench tests it.
- **Everyone else:** users, agents, applications, sharing by weight.
No numeric priorities. Real-time guarantees are a non-goal until something needs them.

### B. Hierarchical stride scheduling by budget
- Each budget has a weight. Each runnable budget has a *pass*; the scheduler runs the lowest pass,
  and after a time slice (10 ms to start) advances it by `STRIDE / weight`. Share is proportional
  to weight. About 50 lines.
- Hierarchical: choose a budget at each level of the tree, then a thread inside it. An agent competes
  for its sponsor's share, never for anyone else's.
- A thread waking from sleep starts at the current minimum pass: it cannot bank credit while asleep,
  but it runs promptly.
- The hart timer (IRQ 0) ends a slice. It is armed only when more than one thing is runnable.

### C. Blocking calls donate time
During a blocking call (blocking scalar, lend, lend_mut), the server thread runs on the **caller's
budget and class** until it replies. The server works at the caller's priority (no inversion), and
its CPU time is charged to the caller (an agent's "10 CPU-minutes" includes the fs time spent on its
reads). Non-blocking sends do not donate; the server pays. (seL4's scheduling-context donation.)

### CPU quotas
Counters on the budget. When exceeded, the budget's threads stop being scheduled and the holder is
notified; for a lease, the lease ends.

### SMP
One global run queue under the kernel lock first. Per-hart queues only if measurement demands it.

## Swap (later)
Not now; the upstream Precursor swap is slated for deletion with the rv32-only code. When it returns:
a userspace swapper server; a per-budget swap limit next to the RAM limit, so swapping never lets a
budget exceed its total; swapped pages encrypted and authenticated (the block layer's AEAD, or the
swapper's own), since swap holds process memory; the system budget never swapped.

## Attack tests the bench gains
- A spinning process cannot delay another budget beyond its fair share.
- A thread bomb or server bomb hits its own limit; other budgets keep creating.
- A memory hog gets `OutOfMemory`; the system budget is untouched.
- Moving memory to a server does not exhaust the server's budget.
- Donation: a low-weight caller does not get a high-weight caller's priority; server time is
  charged to the caller.
- Lease expiry reclaims everything; usage returns to zero.

## Kernel work order
1. Handles and budgets together (the same kind of kernel object); per-page payer; charged kernel tables.
2. Preemption with hierarchical stride scheduling; the two classes.
3. Donation on blocking calls; CPU quotas.
