# B51 ruling: a completion call has no clock on the client

Thread `B51-completion-hold`. Evidence: `.wash/local/B51-report.md` "Phase 2b".

## The fault, named

The serving page promises (serving.md:253-262, native.md:609-614, `libs/client/src/aio.rs:20`)
that the server answers a parked completion call within `hold`, and the client enforces it: a
kernel timeout of `hold + COLLECT_MARGIN_US` (aio.rs:327, :538), after which the call is
abandoned (R3), the server ends the session, and every request answers `Ended` (files.rs maps it
to `eio`). The promise cannot be kept: the mux runs only between dispatches (`run_around`,
ninep_mux.rs:484-511), and a dispatch is the file server's time, which includes every call it
makes to another server (walfsd's `with()` runs walfs on blkd: a flush is the host's fdatasync,
unbounded). So the margin is a bound on something that has none, and one client's slow write
kills every other client's connection, and under icount its own too (the console session's
second 64 KiB write answered `eio` in base-1 is the writer's own session lost). The loss is
permanent while the cause is transient: that is the design error.

## Options weighed

- (a) answer parked calls "when the hold is nearly out" before a dispatch: the same bound in
  another place; a dispatch longer than the threshold still loses the session. Answering *every*
  other session's call before every dispatch is correct but defeats the long poll (N idle
  sessions wake per request) and cannot cover the dispatching session's own call without a round
  trip per request.
- (b) a bigger margin: a bigger bound on the unbounded.
- (c) a walfsd I/O thread with `Write::Wait`/`Read::Wait` and a wake: every op of one
  `Filesystem` still serialises behind the sync, so other clients' *requests* wait anyway and
  take their request deadline; it saves only the idle completion call, at the price of a thread
  and a wait path for every op, repeated in littlefsd and every server that calls out.

## Ruling

The clock comes off the client. The server's `hold` is **how long the server keeps the call
while it has nothing to say**, not when the call returns; a call outlives its hold while the
server is in a dispatch, and comes back when the mux next runs (`expire` answers it empty,
`pump` answers it full). The client's completion call has **no kernel timeout** (`FOREVER`):
a late answer means a busy server; a gone server frees the call by its death
([ipc.md:194, :373]: the caller gets `Dead`). Nothing else changes: the server's session bound,
R28, R77, and the server-side host test `a_completion_call_is_held_at_most_its_hold_and_the_servers_bound`
stand (the server still honours the hold when idle).

What is lost: a caller that waits in its own completion call (`Hub::wait`) may overshoot its
own next deadline by a stall's length; a server wedged alive in a dispatch holds its clients'
calls until it dies. Both are delays where today's design makes a permanent loss on the same
event, and today's client can do nothing with the dead connection either.

Tier: `libs/client` (the hub), not `libs/rt`; walfsd unchanged. The dead-code Busy retry from
the earlier reading comes out (never mention unused things).

## Page sentences

serving.md "The completion call" bullet, replacing from "The client's own timeout on the call" to
"broke its promise.":

> The hold is how long the server keeps the call with nothing to say, not when it returns: a
> server in a dispatch (its file server's time, including the calls that makes to other servers)
> answers when the mux next runs. So the client puts no clock on the call: a late answer is a
> busy server, and a dead one frees the call with `Dead`
> ([R3](../kernel/ipc.md#r3-lends-and-abandoned-calls)).

native.md "One connection needs no other thread" bullet, replacing "and timing it out only a
margin (1 s) after that, so a timeout means the server broke its promise." with:

> and gets it back by then only from a server with nothing to do: a server in a dispatch answers
> when it next runs, so the caller's deadline slips by at most the dispatch, never its session
> ([serving](../servers/serving.md#multiplexed-connections)).

aio.rs module doc line 20-21 says the same; `COLLECT_MARGIN_US` goes (no constant for a clock
that no longer exists). Add to serving.md "Residual risks":

> - **A server wedged in a dispatch holds its clients.** Their completion calls come back only
>   when it answers or dies; the bench, not the client, finds a server that never does.

## The cases

1. `libs/client/tests/aio.rs`: `a_server_that_breaks_its_hold_loses_the_session_at_the_margin`
   is replaced (its claim is now false) by `a_server_late_past_its_hold_keeps_the_session`: the
   fake server takes the completion call and pauses past `hold + COLLECT_MARGIN_US`'s old value
   (same 50 ms hold, pause > 1.05 s), then answers; `hub.wait` returns after the pause with the
   read's answer, `hub.read` on `NOW_FID` then succeeds (no `Disconnected`), and no
   abandoned-call notice reached the server.
2. `libs/rt` host test, the mux end: `a_dispatch_longer_than_the_hold_answers_the_parked_call_after_it`:
   two sessions; B's write dispatch blocks (fake file server sleeps) past A's hold; A's call is
   answered empty when the dispatch returns, A's session is still open, A's next request is served
   in A's next call. This is the server half of the implementer's "dispatch forced longer than the
   hold while another client's call is parked".
3. `steward-home-quota` itself under load is the boot evidence; no new boot case.

## Not ruled here

The hub's connection is dead for good after any `Ended`; whether a client library should reconnect
is a separate question (not B51's).
