# IPC3 layout (design checkpoint; no code yet)

Written against main 53bcd9704. Every list is doubly linked unless marked. A **thread ref** is one
word, `pid << 8 | tid` (tid 1..=255, pid 2..=511; 0 = none); a **call ref** is open-call frame + 1.
New frames are zeroed (`alloc_object_frame`), so every head starts empty. No new allocation.

## Where the words live

**Endpoint frame** (uses words 0-3 today; `store_endpoint` writes only those):
- 4, 5: receivers head, tail (thread refs). FIFO in `receive` arrival order.
- 6, 7: group list head, tail (thread refs of group nodes): every R2 group with a message queued.
- 8, 9: send-group list head, tail: the groups with a *send* queued.
- 10: owed-notice list head (call refs): open calls with `F_NOTICE` on this endpoint.
- 11: open list head (call refs): calls taken here whose caller still waits (`F_WAITING`).

**Thread IPC page**, new words after `W_CALLS`'s block (296..; page has room to `CONTEXT_OFFSET`):
- `W_PREV`, `W_NEXT`: the one list its wait puts it on: receivers (`Receive`), its device's IRQ
  waiters (`Irq`), or its group's sends or calls chain (`Send`). A thread waits on one thing, so
  one pair serves all three. `Sleep` and `Reply` are on none.
- `W_GROUP` (`Send`): the thread holding its group's node.
- `W_SPREV`, `W_SNEXT` (`Send`): its stamp budget's queued chain.
- `W_XPREV`, `W_XNEXT`: the expiry's due list (only inside `expire_due`).
- `W_SEQ`: a sender's arrival (as today); a receiver's arrival too (`next_seq` at `mark`).
- Group node, read only on the thread holding it: `W_G_PREV/NEXT` (group list),
  `W_G_SPREV/SNEXT` (send-group list), `W_G_SENDS` head/tail, `W_G_CALLS` head/tail, `W_G_COUNT`,
  `W_G_TAKEN` (the group's last take while it had anything queued; 0 for none).
- `W_DUE` goes: a message's due is `max(W_SEQ, its node's W_G_TAKEN)`, which is exactly what
  today's per-message restamp computes. **The restamp becomes one write per group**
  (`W_G_TAKEN`). The new reason for ipc.md's `WAIT_CAP` row goes in the report.

**Open-call frame**, after `CALL_WORDS` (35..; `store_open_call` writes only below):
- `C_EPREV`, `C_ENEXT`: its endpoint's open list while `F_WAITING`, its notice list while
  `F_NOTICE`. The two never hold at once, so one pair.
- `C_SPREV`, `C_SNEXT`: its stamp budget's open chain, while `F_WAITING`.

**Device frame** (words 0-11 today): 12, 13: IRQ waiters head, tail (thread refs), FIFO.

**Budget frame**, 104 and 105, beside `HELD_WORD`/`STAMPED_WORD` (see Q1): heads of the queued
messages (thread refs) and the waiting taken calls (call refs) **stamped with this budget**.

## R2's groups, and how a node moves

A group's node lives in its **oldest queued message's** thread. Two orders, because a group's
oldest send can be due later than its oldest call (call at seq 1, send at 5; another group's send
at 3: a full receiver must take the seq-3 send, as `next_sender(calls = false)` does today):
- group list key: `(max(oldest.seq, taken), Group::order)`; the pick for a receiver below
  `MAX_OPEN_CALLS` is the head group's older chain head;
- send-group list key: `(max(oldest send.seq, taken), Group::order)`; a full receiver's pick is
  the head group's sends head.

Keys are values of the one counter, so a new group, a group's first send, and every take (a
delivery or a refusal: R2's turn) give a key above every other: **append at the tail**, O(1). Only
a head leaving without a take (timeout, `Dead`, kill) raises a key without making it the largest;
the group then moves forward past the groups with lower keys, a walk of at most the endpoint's
groups. When the node's thread leaves, the node's words are copied to the new oldest (the older of
the two chain heads), its list neighbours and the endpoint's head/tail are relinked, and each
member's `W_GROUP` is rewritten: at most `WAIT_CAP - 1` writes, today's restamp bound. `send`
finds its group by walking the group list comparing `Group`s (at most one group per sending
budget), and `WAIT_CAP` is the node's count.

## What each path walks afterwards

- **pump**: notices: e's notice list, picking the holder receiving on e that arrived first, its
  lowest rid (the model's: first receiver, its serving order). Exit notice: `pending_notice`
  (≤ 510 process objects, unchanged) to the first non-doomed receiver. Message: receivers from the
  head until one can take (skips doomed, and full ones only while no group has a send); then two
  list heads. Delivery: O(1) list moves plus a node move.
- **next_sender**: gone; two heads. **send**: e's group list once. **receive**: O(1) link.
- **irq_ready**: the device list's head. **destroy_device**: pops its list until empty.
- **R10 step 4** (`budgets_dying`): the owner-chain walk it already makes visits each dying
  endpoint and pops, until empty: receivers (`Dead`), every group's chains (`Dead`), the notice
  list (dropped), the open list (callers `Dead`, calls abandoned with no notice). Then each dying
  budget's queued-stamp chain, then its open-stamp chain (these may pump live endpoints, so queued
  first: a pump never takes a message whose stamp is dying). `fail_all` and both thread walks go.
  `endpoints_dying`'s ≤ 510 process objects stay. There is no other endpoint destruction path.
- **expire_due**: one walk of the due processes' threads, as today's first walk, linking every
  due wait into the due list in (pid, tid) order, then a stable bottom-up merge sort by deadline
  on the links: (deadline, pid, tid), today's order, in R log R steps. The loop pops the head
  (skipping a thread no longer waiting), still comparing with the due budget (timeout first at an
  equal instant). `end_thread` unlinks a listed thread, since a deadline destruction mid-expiry
  frees killed threads' pages. The walk also gives `next`, `stale`, and each walked process's
  cache (its earliest deadline still to come). Billing: Q3.

## The checked build's audit

Every link change sets a flag; at the end of each `message.rs` entry point and `expire_due` that
set it, `sched::audit` runs one audit (Q2). It enumerates without a frame scan: the budget tree
from the root, each budget's two stamp chains and its owner chain's endpoints (four lists) and
devices (one); per list: symmetric links, head/tail, each member's own words say it belongs there
(wait kind, object, flags, stamp), order (receivers by `W_SEQ`, chains by seq, both group lists by
key), node on the oldest member, count and `W_GROUP` right. Then one walk of every thread and its
open calls counts what must be listed; the totals must match. The due list is empty outside
`expire_due`. Cost per audit: budgets + objects + threads + open calls.

## Behaviour that moves to the model's

Receivers in arrival order, not (pid, tid); IRQ waiters FIFO; a notice to the earliest-arrived
holder. A destruction fails in list order, not (pid, tid). Cases that relied on (pid, tid) order:
named in the report once run.

## Questions (each blocks only its part)

- **Q1, a gap in the brief.** R10 step 4 also fails queued messages and waiting taken calls whose
  *stamp* is dying, on live endpoints too; the dying endpoints' lists do not reach them, and today
  the second thread walk does. Proposed: the two stamp chains above, headed in the stamp budget's
  frame (words 104, 105: two consts in `budget.rs` beside `STAMPED_WORD`, which is not my file).
  Alternative: keep one thread walk for stamps only, which breaks "nothing walks every thread".
- **Q2.** The audit needs an id beside `AUDIT_PROCESS_INDEX` in `sched.rs` (K22's file): one const,
  `AUDIT_IPC_LISTS = 3`. Or should it run at the dispatcher's exit (`redoubt.rs`) instead?
- **Q3.** Billing: the one walk and sort are billed with the first item the expiry handles (as
  today's first walk is); each later wait pays only its pop and its ending, no longer a walk.
- **Q4.** Splitting the expiry's time from its pumps in `worst-walk`'s numbers needs the bench to
  subtract nested `PUMP` brackets from `EXPIRY` (`tools/testbench`, not mine), or a new bracket
  around the collect walk in `sched::trace` (K22's). Which?
