# A server is not told which calls an endpoint's destruction abandoned

## What

When an endpoint is destroyed (its owner budget is destroyed, R10 (destruction)), the kernel
first fails every thread blocked sending to it or receiving on it with `Dead`, then fails every
caller whose call a server took through it with `Dead`, abandoning those calls
(R3 (lends and abandoned calls)). Receivers go first, so no abandoned-call notice is ever offered
for these calls: there is no endpoint left to receive one on. A server that took calls through the
endpoint, and runs outside the dying budget, learns only that its `receive` returned `Dead`.

The rule to state on the IPC page: "`Dead` from `receive` on an endpoint is the server's cue that
every call it took through that endpoint is abandoned. No notice follows, since the endpoint is
gone."

## Why it matters

R13 (one outcome per call) and I15 (abandoned calls reported once) promise a server one report
per abandoned call. Here the report is implied by `Dead`, not delivered, and the server must
know that to release the lends and state it holds for those calls. It is an open design question
until the rule is written.

## Where

- [`kernel/src/message.rs`](../../kernel/src/message.rs): `destroy_endpoint` (receivers first,
  then the callers of taken calls).
- The page: [IPC](../kernel/ipc.md#r3-lends-and-abandoned-calls) and its failure section.

## Done when

The owner or the architect settles the rule, the IPC page states it under R3, and a case
destroys an endpoint's owner budget while a server in another budget waits in `receive` holding
taken calls: the `receive` returns `Dead`, each caller gets `Dead`, and a reply to a taken call
gets the result the rule states.
