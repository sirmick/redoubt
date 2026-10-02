# A destruction can feed a receiver it is about to kill

## What

Destroying a budget ends its processes one at a time, and each process's end pumps the
endpoints its threads served or waited through before the next process ends. A pump picks any
thread receiving on the endpoint, and nothing stops it from picking a thread of a process the same
destruction is about to end: the budget it runs in is already marked dying, but the pump does not
look. That thread takes what it is given and dies with it a moment later.

- **An exit notice.** Two processes in the dying budget receive on their creator's exit
  endpoint, which a budget outside it owns. The first to end has its notice delivered to the
  second, which ends holding it. The creator never receives that notice.
- **A queued call.** The same shape takes a queued call from a sender outside the budget, and
  that caller gets `Dead` rather than waiting for the restarted server. A doomed receiver can
  newly take a queued call only when its own process's open calls drop below `MAX_OPEN_CALLS`,
  and only that process's own end drops them. So across processes this shape is argued from the
  code, not yet reached by a case.

The model settles every pump once, after the whole destruction, by which time the doomed receivers
are gone. So the model and the kernel disagree here.

## Why it matters

[R4b (a server dies)](../kernel/ipc.md#r4b-a-server-dies) says queued senders keep waiting for
the restarted server. An exit notice is what a supervisor restarts a service on
([processes](../kernel/processes.md#exit-notices)), so a lost notice can leave a service down.

## Where

- `kernel/src/message.rs`: `pump`, where it chooses the receiving thread.
- `kernel/src/budget.rs`: `destroy_subtree`, which marks the subtree dying and then ends its
  processes one by one.

## Done when

- A case shows the bug first: two processes in one budget, both receiving on their creator's exit
  endpoint, and the creator destroys the budget. Afterwards the creator receives both notices.
  It runs on both widths, as a checked build, and fails on main.
- A thread of a process in a dying budget takes nothing from a pump: no message, no exit notice.
  What it would have taken stays for the next receiver outside the destruction. The rule is
  written beside R4b, and the exit-notice delivery in processes.md says the same.
- The residual risk on [IPC](../kernel/ipc.md#residual-risks) goes, and this page with it.
