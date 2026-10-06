# R78 (fair kernel entry), as built: the text for SMP1's docs commit (architect-15)

Three pieces, each to be pasted as is. At the rebase onto main, main's planned section and row
(commit 6d0ce2090) are replaced by these same words.

## 1. docs/kernel/scheduling.md: after R12's section, before `### R23 (no test channels)`

```markdown
### R78 (fair kernel entry)

Status: built · tested: bench:smp-boot

On several harts, kernel entry is fair across them: the one kernel lock is a FIFO ticket lock,
so a hart that arrives at the kernel waits behind at most `MAX_HARTS` - 1 kernel sections, never
for ever. The rule exists for R12. A test-and-set lock is unfair: a hart can lose the lock to
later arrivals indefinitely, and the budget running on that hart loses its share with it, so one
budget's harts can starve another's of kernel entry, which no pattern of calls may do. FIFO
bounds the wait by the hart count, and the bound is part of how R12's shares are judged across
harts ([several harts](../plan/m2-usable-shell.md#several-harts)).

The lock (`kernel/src/cell.rs`, `TicketLock`) draws a ticket with a relaxed `fetch_add`, spins
until `serving` reads it (acquire), and releases by `serving + 1` (release); each turn of the
spin runs the pause hint through one named place, `wait_for_change`, where a Zawrs wait goes
later (no `wrs.nto` is emitted today). It is taken once at trap entry and released before the
return to user mode or an idle wait. A checked build asserts, at every acquisition, that the
hart waited behind fewer sections than harts were started (the draw, the read and the release
are sequentially consistent there, so the count can only under-count); `bench:smp-boot` prints
the most any hart waited (one section at two harts, three at four, on both widths). It is
attacked by a kernel built with the ticket lock replaced by test-and-set
(`sched-test-and-set-entry`, a debug-only kernel feature like the tie fault,
[below](#failure-and-restart); not a model mutation, since the model has one hart): with it,
`smp-boot` fails at the FIFO assertion at two and at four harts on both widths, in a recorded
negative run. The one-hart kernel is the uncontended case of the same lock.
```

## 2. docs/kernel/scheduling.md, "Failure and restart", the diagnostic-features sentence

After `sched-inject-tie-fault`'s clause, in the same sentence:

```markdown
`sched-test-and-set-entry`, which replaces the kernel lock by test-and-set for
[R78](#r78-fair-kernel-entry)'s recorded negative run;
```

(On main the clause reads "planned with [R78]…, a debug-only replacement of the kernel lock by
test-and-set for its recorded negative run"; this is its built form.)

## 3. docs/SECURITY.md: the row, after R12's in the kernel group

```markdown
| On several harts, a hart waits for kernel entry behind at most `MAX_HARTS` - 1 kernel sections: the kernel lock is FIFO, so no budget's harts can starve another's of the kernel | [R78 (fair kernel entry)](kernel/scheduling.md#r78-fair-kernel-entry) | `kernel/src/cell.rs` | bench:smp-boot | built, tested | The bound is by hart count, not time; a long kernel section delays every waiting hart by its length ([more](kernel/scheduling.md#residual-risks)) |
```

The residual the row links, in scheduling.md's "Residual risks" (main has it from 6d0ce2090; on
this base add it before "The kernel is not preemptible"):

```markdown
- **Fair kernel entry is bounded by count, not time.** On several harts, R78 bounds the wait
  for the kernel lock by `MAX_HARTS` - 1 kernel sections, each as long as the call or
  destruction holding it: a long section delays every waiting hart by its length, which is one
  more reason R12 bounds a call's kernel time.
```
