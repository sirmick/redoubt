# Resources: budgets, scheduling, the timer

Designed, not built (today: no budgets, cooperative scheduling, the timer as IRQ 0; BOOT.md).
Owns: why budgets look the way they do, scheduling policy, the timer. The precise fields, rules and
constants: KERNEL-SPEC.md. Revocation by budget: CAPABILITIES.md. Labels: CONTAINMENT.md.

## Budgets
Every process lives in exactly one budget, and every resource is charged to one. One object does five
jobs, instead of five mechanisms (cgroups, namespaces, revocation lists, security labels, schedulers):
1. **Accounting:** every kernel object costs pages from its budget; running out is an error for the
   caller, never for anyone else.
2. **CPU:** its weight is its share.
3. **Revocation:** destroying it revokes everything stamped with it (CAPABILITIES.md).
4. **Information flow:** its labels (CONTAINMENT.md).
5. **Identity for servers:** its account travels with every message.

Seven fields: parent, pages, processes, weight, class, labels, deadline, plus the account. Why each
rule:
- **Everything costs pages**, including threads, handles, endpoints and budgets themselves, so one
  number bounds every kind of exhaustion. Processes are counted separately only because PIDs are
  address-space tags, which are scarce on rv32.
- **Carved, never overcommitted.** Children's limits add up to at most the parent's, so an allocation
  succeeds or fails on the caller's own budget alone and reveals nothing about anyone else. A
  parent's usage counts its children's limits, not their live usage, for the same reason.
- **Top level:** `root -> system [default 25% of RAM, set in the boot manifest] + users [the rest]`.
- **Bounded depth**, because revocation and "is this a descendant" walk ancestors.
- **The kernel never panics and never kills an innocent process to make room.**
- **Destroying a budget returns everything** (the exit slots its creator paid for, once their
  notices are received); a lease is a budget with a deadline, at most `MAX_LEASE` (CAPABILITIES.md).

## Scheduling
### Two classes, strictly ordered
- **System:** driver threads and system servers. Only system-signed code; never granted to users.
  Runs before everything else. System code is TCB; if it spins, that is our bug and the bench tests it.
- **Everyone else:** users, agents, applications, sharing by weight.
No numeric priorities. Real-time guarantees are a non-goal until something needs them.

### Stride over budgets
One flat queue of budgets with runnable threads (KERNEL-SPEC.md, R12). Actual runtime is charged at
every deschedule, so a thread that runs briefly and sleeps is still charged; a waking budget cannot
bank credit while asleep, and still runs promptly. Because weight is carved like pages, a budget
always gets at least its share; idle share is redistributed by weight. Within a budget, threads run
round-robin.

### Deferred
**Time donation** (a server running on the caller's budget during a call) and **CPU quotas**.
Donation is intricate (seL4's scheduling contexts), and a donated server thread stopped mid-call by
the caller's quota or lease could hold server locks forever. Add only if measured priority inversion
hurts. Until then servers pay for their own CPU, and leases are bounded by deadline and weight.

### SMP
One global run queue under the kernel lock first; one budget per core (PLATFORM-FPGA.md). The work
list: PLAN.md.

## The timer
- **The kernel owns the hart timer.** One deadline queue (slice ends, timeouts, lease deadlines),
  programmed through SBI TIME, or Sstc by capability feature. It is always armed.
- **Every blocking call takes a timeout.** Sleeping is a `receive` with a timeout and nothing to
  receive. There is no timer server and no IRQ 0 timer.
- **User mode reads the high-resolution counter** (`rdtime`) directly. Hiding time protects nothing
  (TENETS.md, timing).
- **Wall-clock time** (dates, time zones, NTP) is a userspace offset over monotonic time.

## Swap (later)
A userspace swapper server; a per-budget swap limit next to the page limit, so swapping never lets a
budget exceed its total; swapped pages encrypted and authenticated; the system budget never swapped.

## Attack tests the bench gains
- A spinning process cannot delay another budget beyond its share, including by sleeping briefly
  between bursts.
- A thread, endpoint, handle or budget bomb hits its own page limit; other budgets keep creating.
- A memory hog gets `OutOfMemory`; the system budget is untouched.
- A transfer to a server that did not opt in fails; the server's budget is untouched.
- A lender that dies mid-call leaves the server running; the pages are freed at its reply.
- Lease expiry reclaims everything; the parent's usage returns to what it was once the exit notices
  are received (I10).
