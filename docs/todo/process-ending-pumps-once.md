# An ending process pumps each endpoint once, after its threads

## What

`process_ending` (`kernel/src/message.rs`) ends a process's threads one at a time, and
`thread_ending` pumps the endpoint each one waited for a reply through as soon as that thread
ends. Two things follow:
- A later thread of the same process that is still receiving on that endpoint is a receiver
  `pump` can pick. It can take a queued call and then end holding it, so the caller gets `Dead`
  ([R4b (a server dies)](../kernel/ipc.md#r4b-a-server-dies)). R4b says queued senders keep
  waiting for the restarted server.
- Every pump walks every thread, about 1.2 ms. With four parked calls, `process_ending` is 8 ms,
  and that is its whole line in the budget of
  [R10 (destruction)](../kernel/budgets.md#r10-destruction) ([budgets](../kernel/budgets.md#residual-risks)).

## Why it matters

R4b is broken for any process that both calls and receives on one endpoint. The time is margin
under R10's 30 ms: about 3 ms at the containment gate's full fill.

## Where

- `kernel/src/message.rs`: `process_ending`, `thread_ending` (its `served` pump), `pump`.

## Done when

- A case shows the bug first. One process has a thread waiting for a reply through endpoint E
  and a later thread receiving on E, and another process's call is queued on E. When the first
  process is killed, the queued caller must still be waiting, and it is served by the next
  receiver.
- `process_ending` ends every thread of the process, gathering the endpoints their waits served
  (at most `MAX_THREADS`, each once). It then pumps each endpoint once. A lone thread's exit
  still pumps at once.
- Notices stay exactly once, to the thread holding the call, in no promised order
  ([R3 (lends and abandoned calls)](../kernel/ipc.md#r3-lends-and-abandoned-calls),
  [I15 (abandoned calls reported once)](../kernel/invariants.md#i15-abandoned-calls-reported-once)).
- `process_ending` is remeasured with the bisect's T records at the full fill and the measured
  numbers go on budgets.md. The residual in ipc.md goes, and this page is deleted.
