# K16 commit 8: the pump at full occupancy (architect-11, ahead of the numbers)

## Which walk

`pump` (kernel/src/message.rs, wp-k16) walks every live thread of every live process, up to
four times per delivery:
- the abandoned-call notice: `find_thread` over all threads, and for each receiver on e its
  open calls;
- the exit notice: `find_thread` for a receiver on e;
- the pick: `find_thread` for a receiver on e, and for each such receiver `next_sender`, which
  walks every thread again to find R2's oldest message.
So one delivery is O(T) at least and O(R_e x T) at worst, with T = 130,051 at full occupancy and
R_e the receivers waiting on e; the loop repeats it per message delivered. It is not a walk over
the receiving process's threads (<= 255): it is over everyone's. ipc.md's residual "Delivery
walks every thread, twice over" names it, and justifies it by the constants clause.

## Does ruling 2 / K16-walks-ahead.md cover converting it?

No. Ruling 2 made walks skip empty slots: cost follows live threads. The pump already does; its
problem is that it follows *other processes'* live threads. Converting it is a redesign of the
IPC core's data structures, not a walk change:
- per endpoint, an intrusive list of threads waiting in `receive` there (links in the IPC-page
  slot, as K10 linked an endpoint's open calls for its destruction);
- per endpoint, per R2 group, FIFOs of queued senders (sends and calls apart, for R4a's skip),
  and the groups ordered by (due, group): the pick is the head of the least group;
- per endpoint, a list of owed abandoned-call notices.
Then a delivery costs a constant plus the groups queued on e (or log of them), touching only
objects tied to the endpoint the call names. The model, the oracle and R2's mutations must agree
unchanged. That is its own package, not one more K16 commit.

## What R12 says

"A system call's kernel time is bounded by a constant plus a term linear in the pages it maps or
the objects it names. It never depends on ... what other processes hold." A term linear in a
fixed constant counts as a constant. A pick over the receiving process's own threads (<= 255) is
fine under that. A walk over every thread is constant only by the letter (512 x 255, squared at
worst); it depends on what other processes hold, which the first sentence forbids, and every wake
waits for it. At 66x the old slot count, the residual's justification no longer holds in fact.

## If R10 > 30 ms net (the brief's stop-and-report)

An owner decision_request, per K16-walks-ahead.md "Was 512 right?":
- (recommended) K16 merges with the worst-walk numbers stated as residuals (they are not
  targets), ipc.md's residual rewritten with the numbers and the R12 conflict named, and a new
  package redesigns delivery as above (cut after K16; R12's "partly tested" gains the pump);
- `MAX_THREADS` lower (e.g. 63), which divides T by 4 but keeps the conflict;
- `MAX_PROCESS_COUNT` lower (256 or 128), the same.
Not free: lowering a value to pass without asking. If R10 <= 30 ms net, no owner question; the
numbers go in as the brief says, and I still recommend the redesign package to the orchestrator.

Also wanted from the rerun: the pump's time alone (one delivery, its walks counted), and which of
R10's destruction steps pump (the abandoned-call notices) and how many times.
