# Resources: budgets, scheduling, the timer

Budgets, accounting and scoped revocation are implemented on the new kernel path. CPU weights
and deadlines are recorded; stride scheduling and timer-driven enforcement remain planned.
The current scheduler is cooperative and exposes the hart timer as IRQ 0 (BOOT.md).
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

Seven fields: parent, pages, processes, weight, class, labels, deadline, plus the account.
Why each rule:
- **Everything costs pages**, including threads, handles, endpoints and budgets themselves, so one
  number bounds every kind of exhaustion. A budget's own page is its parent's, and a process's
  object is its creator's (it holds the exit notice, which must outlive the process's budget).
- **A lend is charged to both sides while its call is open**, so a server's budget covers its open
  lends up front (about 4 MiB for 64 open 9P calls), a server that cannot pay does not take the
  call, and no budget is ever over its limit. Processes are counted separately only because PIDs are
  address-space tags, which are scarce on rv32.
- **Carved, never overcommitted.** Children's limits add up to at most the parent's, so an allocation
  succeeds or fails on the caller's own budget alone and reveals nothing about anyone else. A
  parent's usage counts its children's limits, not their live usage, for the same reason.
- **Top level:** `root -> system [default 25% of RAM, set in the boot manifest] + users [the rest]`.
- **Bounded depth**, because revocation and "is this a descendant" walk ancestors.
- **The kernel never panics and never kills an innocent process to make room.**
- **Destroying a budget returns everything** (the process objects its creators paid for, once their
  notices are received); a lease is a budget with a deadline, at most `MAX_LEASE` (CAPABILITIES.md).

## Scheduling
The policy below is the accepted target, not the current scheduler. **Question 166 remains open**:
answer 103's one-slice wakeup promise does not follow from R12's retained-pass rule and unspecified
ties. The promise is not verified, and neither the proposed weaker claim nor a replacement
scheduler has been accepted. WP-K5 must resolve that acceptance issue before claiming the bound.

### Everyone by weight, in one queue
- **One stride queue for every budget** (KERNEL-SPEC.md, R12). There is no priority, no second
  queue and no flag that jumps one: `init`, the steward and the drivers get **large weights in the
  boot manifest** (INIT.md) instead of running first.
- **Wake rule.** A budget that wakes uses `max(own pass, current minimum)` (R12); it can retain a
  larger pass. Answer 103's claim of about one `SLICE` is qualified by open question 166 above.
  Strict priority would only matter for a
  driver that spins while others are runnable, and that is a bug for the bench to find, not a mode
  to support.
- **The steward's weight is large too**, which is what keeps logout and ending a lease responsive.
  It also works for users, so it bounds the work any one request can cause and relies on its
  per-(account, label set) caps.
- **Servers that work for users** (`fsd`, `keyd`, `ipd`, `sshd`, ...) get ordinary manifest weights
  and bound the work of one request. Were they ahead of everyone, Bob could make `fsd` or `keyd` do
  expensive work and no user budget would run meanwhile.
- **Unresolved latency claim (166):** answer 103 states up to one `SLICE` for a driver or steward
  under load. The specified mechanism does not establish that bound; it is not a measured result.
- **Stated residual:** work a server does for a user is paid by the server's weight, not the
  requester's (and the steward's by the steward); CONTAINMENT.md.
Class (`system` or `user`) means trust, not order: it decides R1's exemption, who may read
`budget_usage` across labels, who may add labels, and (being inherited) who may create further
system budgets — and nothing about scheduling (KERNEL-SPEC.md, Objects). No numeric priorities.
Real-time guarantees are a non-goal until something needs them.

### How stride works
One flat queue of budgets with runnable threads, and only that one (KERNEL-SPEC.md, R12). Actual
runtime is charged at every deschedule, so a thread that runs briefly and sleeps is still charged;
a waking budget cannot bank credit while asleep (its pass is raised to the queue's minimum), and
for the same reason still runs promptly. Because weight is carved like pages, a budget
always gets at least its share; idle share is redistributed by weight. Within a budget, threads run
round-robin.

### Deferred
**Time donation** (a server running on the caller's budget during a call) and **CPU quotas**.
Donation is intricate (seL4's scheduling contexts), and a donated server thread stopped mid-call by
the caller's quota or lease could hold server locks forever. Add only if measured priority inversion
hurts. Until then servers pay for their own CPU, and leases are bounded by deadline and weight.

### SMP
One global run queue under the kernel lock to begin with; one budget per core (PLATFORM-FPGA.md). The work
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
- A driver woken by an interrupt runs within about one `SLICE` while user budgets spin (the stated
  cost in answer 103, still disputed by open question 166), and a large-weight server keeps its
  share under that load. This is a pending acceptance claim, not existing evidence of a bound.
- A thread, endpoint, handle or budget bomb hits its own page limit; other budgets keep creating.
- A memory hog gets `OutOfMemory`; the system budget is untouched.
- A transfer to a server that did not opt in fails; the server's budget is untouched.
- A lender that dies mid-call leaves the server running; the server gets an abandoned-call notice,
  and the pages are freed at its reply.
- A user flooding `fsd` with expensive requests delays other users only by `fsd`'s weight, never
  every user budget.
- Lease expiry reclaims everything; the parent's usage returns to what it was once the exit notices
  are received (I10).
