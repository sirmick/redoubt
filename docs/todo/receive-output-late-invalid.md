# A notice delivered to an unwritable `receive` record is lost

## What

A thread blocked in `receive` gets its result written into its record when something arrives.
If the record has become unwritable while the thread waited (the process's own thread unmapped
it or changed its flags), the kernel wakes the thread with `InvalidArgument`. For a message or an
exit notice the kernel checks the record before it takes the item, so the item stays pending for
the next `receive`. For an interrupt or an abandoned-call notice the item is consumed, and the
thread learns nothing of it.

A lost abandoned-call notice leaves the thread holding a call whose id it never learned, until
the process ends: I15 (abandoned calls reported once) makes the report, but it is never
received.

## Why it matters

Only the process's own threads can make its record unwritable, so the loss is self-inflicted. But
R13 (one outcome per call) and I15 promise one report per abandoned call, and a driver that
loses an interrupt can stall.

## Where

- [`kernel/src/message.rs`](../../kernel/src/message.rs): `answer_record` and
  `check_receive_record`, and the interrupt and abandoned-call delivery paths.
- The pages: [IPC](../kernel/ipc.md#residual-risks) and
  [invariants](../kernel/invariants.md).

## Done when

The owner decides the rule: either interrupts and abandoned-call notices stay pending like
messages when the record is bad, or the loss is the stated rule. The code follows it, and a case
makes a waiting thread's record unwritable and then delivers each kind.
