# IPC3 layout: the Architect's ruling (architect-13, 2026-10-03)

**Verdict: OK to build from `.wash/local/IPC3-layout.md`, with the rulings below.** They amend the
brief where they differ; the successor builds from the layout plus this page.

## (1) The order: two lists, as proposed

The brief's "for a full receiver, the first group with a send" is wrong; the layout is right. The
model (`model/src/kernel.rs` `next_sender`, `served`) orders groups by the due of each group's
first *takeable* message, ties by key. A message's due is its seq, raised to the group's take
when the group is served while it waits, so `due = max(W_SEQ, W_G_TAKEN)` exactly (one counter
for seqs and takes, so a take before the message was queued is below its seq). A group's oldest
send can be due later than its oldest call, so a full receiver needs its own order: the
send-group list keyed `(max(oldest send seq, taken), Group::order)`. Agreed.

State in the code and the report: **a take moves the group to the tail of both lists** it is on
(the take raises both keys to the newest value), and a head leaving without a take moves the group
on whichever list's key it raised. Every walk named is bounded by the endpoint's own groups or
receivers, never every thread: that is the package's rule, and it holds.

## (2) Q1: per-budget stamp chains, as proposed

R10 step 4 (budgets.md:538-543) fails queued messages and taken waiting calls *sent through a
handle stamped with a dying budget*, on any endpoint, live ones included. No endpoint list reaches
them. Take the two chains headed in the stamp budget's frame: two consts in `kernel/src/budget.rs`
beside `STAMPED_WORD` (104: queued messages, 105: waiting taken calls), with the same comment
style (above every object's own words, zero in a new frame). This is IPC3's one line in
`budget.rs`, allowed. Order as the layout says: the queued chain first, then the open chain.

## (3) Q2: the audit runs at the outermost exit only

Not at each `message.rs` entry point. Its pumps nest (an expiry pumps, a destruction pumps), and
the scheduler oracle refuses an audit inside a destruction or inside another audit
(`tools/testbench/src/sched_oracle.rs`, `'U'` inside `X`). So: every list change sets the flag,
and **one call runs the audit when the flag is set, at the end of each outermost kernel entry that
can change a list**: the trap handler's common return if there is one; otherwise the end of
`redoubt::handle` (every system call, destructions included, after their own audit), the end of
`time::expire_due` (the timer, deadline destructions included), and the device interrupt's
delivery to an IRQ waiter. Its id is one const beside the others in `sched.rs`:
`AUDIT_IPC_LISTS = 3`. **K22 takes 4** for its marks audit (told). That is IPC3's one line in
`sched.rs`. The audit scans no frame: it enumerates from the budget tree, the stamp chains, the
owner chains' endpoints and devices, then one walk of every thread and its open calls for the
totals. That is the checked build's only walk of every thread, which brief item 8 allows.

## (4) Q3: the one walk is billed in equal shares

Not to the first item alone. A merge sort of R due waits is R log R work, and R is set by every
budget whose waits fall due together. Billing it to the first item would let a neighbour's many
waits charge a budget with one, which R12 forbids ("never depends on what other processes hold").
So: measure the collect walk and the sort, and bill them in equal shares to the items the expiry
handles (each expired wait, and each wait found already gone), the remainder to the first, each
item adding its own pop and ending. `sched::bill` takes a measured amount, and every share goes
to a budget whose item the expiry handled, so the oracle's charging rules hold unchanged. With no
item, billing is as today.

## (5) Q4: move the EXPIRY bracket, in IPC3

Nested walks are not recorded (`sched::trace::walk`: "A walk inside another is the outer one's"),
so the bench cannot subtract the pumps from `EXPIRY`. Instead `time::expire_due` (IPC3's) opens
the `EXPIRY` walk around the collect walk and sort only. Each ending and its pump run after it
closes, and record as their own `PUMP` walks. No `sched.rs` or trace change. The worst-walk case
(IPC3's toml) reports `EXPIRY` (the timer's own walk) and the interrupt's `PUMP`s apart. Any
reading change in the bench's walk report is small and IPC3's.

## Frame words: checked

- Endpoint 4-11, device 12-13, open-call `CALL_WORDS`.. (35..), thread page from `THREAD_WORDS`
  (296) all lie below `DEFER_WORD` (100) and the chain heads 101-103. Thread words stay inside
  `CONTEXT_OFFSET` on both widths: about 312 words is 2,496 bytes, against 3,840 and 3,968.
- Budget frames have nothing at 104-105.
- objects.md and memory-layout.md give no word maps, so neither page changes. Thread refs
  (`pid << 8 | tid`) fit: PIDs to 511, TIDs to 255. Removing `W_DUE` is fine: renumber or leave
  the hole, either is acceptable.

## Behaviour that moves: page lines

The pages do not fix the receiver order today, so the layout's move to the model's order needs
stating:
- **ipc.md**: "Receivers waiting on one endpoint are served in the order they began to wait; a
  receiver at `MAX_OPEN_CALLS` is passed over while no group has a send. An abandoned-call notice
  goes to the holder that began waiting first." And the brief's residual and `WAIT_CAP` lines.
- **devices.md**: "Threads waiting on one interrupt are woken in the order they began to wait."
- **timer.md:244-246**: "So is the walk that found the item, and a budget with many timeouts due
  at once pays one walk for each." becomes "The one walk that finds what is due, and its ordering
  by deadline, is billed in equal shares to the items it handles, each with its own ending, so a
  budget pays for its own share of a shared instant, not a neighbour's." The residual at :292-299
  says what the code then does. :154's equal-instant order is unchanged (the sort is stable over
  (pid, tid)).
- **scheduling.md:195-197**: "each with the walk that found it" becomes "each with its share of
  the walk that found it".
- **budgets.md**: R10's implementation note (:682-689, "Two thread walks for the endpoints'
  teardown") becomes the dying endpoints' lists and each dying budget's two stamp chains.
  Destruction fails in list order; R10 states no order, so no rule changes.

## (3b) The audit's cost: ruled again (architect-14, 2026-10-03)

Measured: the object enumeration costs ~430 us per audited exit, mostly in the budget tree's
decoding. 1,300 of them in `sched-timer-flood` stretch the attacker's calls until its stale timer
interrupts land in its own slice, and the case fails on both widths. Audits are stamped and move
no slice (K18), but a hardware deadline still fires by real time.

1. **At each outermost exit that changed a list (the flag, as before): option (a).** One walk of
   every thread and its open calls. Each list it meets is audited whole from the member that heads
   it, and the totals are matched by kind. There is no budget walk.
2. **The full enumeration** (today's, from the budget tree, stamp chains and owner chains) closes
   (a)'s gap: a list whose every member is stale. It runs at the checked build's existing full-audit
   points, after each destruction and at each process-object free, and at a pick that finds
   nothing to run, before the hart idles.
   - Not once a slice: that would put a 430 us stretch back into every slice of the flood case.
   - A list left with only stale members comes from a teardown that failed to unlink, which is
     exactly where these points run.
   - (b)'s raw accessors are allowed inside it if they are simple, but are not needed.
3. **The scale check (K22's lesson).** Run (a) at the gate's full fill and in `worst-walk`, by name.
   If (a) per audited exit drops trace records or moves the gate, fall back to (a) once a slice
   (`SLICE_US`, at the first audited exit after it) and before idle, and report which form ships.
4. **endpoint-destroy-full's fix: no objection.** The teardown runs in the existing owner-chain
   walk, skipping endpoints whose eight list words are 0. That is exact only if an empty list's
   words are 0 and a non-empty list's never are. The full enumeration must assert that: an endpoint
   with any non-zero list word is a member of its owner's chain, and one that is skipped has all
   eight words at 0.
