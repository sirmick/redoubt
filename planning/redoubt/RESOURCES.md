# Resources: budgets, scheduling, the timer

Designed, not built (today: no budgets, cooperative scheduling, the timer as IRQ 0; BOOT.md).
Owns: budgets, scheduling, the timer. Revocation by budget: CAPABILITIES.md. Labels: CONTAINMENT.md.

## Budgets
Every process lives in exactly one budget, and every resource is charged to one. A budget is a
kernel object held by capability, with seven fields:

| Field | Meaning |
| --- | --- |
| parent | Budgets form a tree: root -> system / users -> alice -> session, agent -> sub-agent. |
| pages | Limit and usage. **Every kernel object is charged in pages** to its owning budget: memory, page tables, handle tables, thread contexts, endpoints, and budgets themselves. |
| processes | Limit and usage (PIDs double as ASIDs, which are scarce on rv32). |
| weight | CPU share, carved like memory. |
| class | `system` or user (Scheduling). |
| labels | Fixed at creation (CONTAINMENT.md). |
| deadline | Optional; when it passes the kernel destroys the budget (a lease). |

- **Hard limits, no overcommit.** A child's pages, processes and weight are carved from its parent:
  the children never add up to more than the parent. An allocation succeeds or fails on the caller's
  own budget alone.
- **Top level:** `root -> system [default 25% of RAM, set in the boot manifest] + users [the rest]`.
- **Bounded depth** (revocation checks walk ancestors).
- **Limits fail the caller** with `OutOfMemory` (or the matching error). The kernel never panics and
  never kills an innocent process to make room.
- **Every page is zeroed** before a process gets it; userspace never names physical RAM.
- **Destroying a budget** kills everything in it, revokes every handle stamped with it or a
  descendant, and returns every resource.
- **Usage is read with a syscall** by a holder whose labels allow it (CONTAINMENT.md).

## Scheduling
### Two classes, strictly ordered
- **System:** driver threads and system servers. Only system-signed code; never granted to users.
  Runs before everything else. System code is TCB; if it spins, that is our bug and the bench tests it.
- **Everyone else:** users, agents, applications, sharing by weight.
No numeric priorities. Real-time guarantees are a non-goal until something needs them.

### Stride scheduling over budgets
- Each budget that runs processes has a weight and a *pass*. The scheduler runs the runnable budget
  with the lowest pass, from one flat queue.
- **Charge actual runtime at every deschedule:** pass += runtime x `STRIDE / weight`. A thread that
  runs briefly and sleeps is still charged for what it ran.
- **On wake,** pass = max(own pass, current minimum): a sleeper cannot bank credit and still runs
  promptly.
- Because weight is carved like memory, every budget gets at least its carved share. Unused share
  goes to everyone by weight.
- The timer ends a slice (10 ms to start); it is always armed.

### Deferred
- **Time donation** (a server running on the caller's budget during a call) and **CPU quotas**.
  Donation is intricate (seL4's scheduling contexts), and a donated server thread stopped mid-call by
  the caller's quota or lease could hold server locks forever. Add only if measured priority
  inversion hurts. Until then servers pay for their own CPU, and leases are bounded by deadline and
  weight.

### SMP
One global run queue under the kernel lock first. All hardware threads of a core run one budget or
idle (PLATFORM-FPGA.md). The work list is in PLAN.md.

## The timer
- **The kernel owns the hart timer.** It keeps one deadline queue (slice ends, sleepers, lease
  deadlines) and programs SBI TIME, or Sstc by capability feature.
- **`receive` takes a timeout.** Sleeping is a receive with a deadline and nothing to receive. There
  is no timer server and no IRQ 0 timer.
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
- Transferring pages to a server that did not opt in fails; the server's budget is untouched.
- Lease expiry reclaims everything; usage returns to zero.
