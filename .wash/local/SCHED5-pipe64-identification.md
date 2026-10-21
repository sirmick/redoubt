# SCHED5 / train 91: rv32 pipe-sixty-four, identified (sched5-implementer-3, 2026-10-21)

## Result

Not a slower range call. A waiter thread of the session VM issues a completion `call` on
`piped`'s endpoint after `piped` has died. That endpoint is owned by the **session's** budget,
not `piped`'s, so nothing ever fails the call. The waiter blocks for ever, its place
(`MAX_WAITERS` 6, userland/otp/redoubt/src/io.rs) is never given back, and every later pipeline
waits `FILE_WAIT_US` (10 s) for a place and fails `{:mkdir, :emfile}`. The 100-400 s is that
cascade, about 10 s for each pipeline after the stuck one.

This is a pre-existing race in the platform's server lifecycle. SCHED5's timing makes it much
likelier; no SCHED5 code is on the path.

## Rates (rv32, alone, quiet cores, host clock)

| tree | pass / runs | notes |
| --- | --- | --- |
| base 820d5e5ff | 3/3 | 16.9-17.1 s |
| ea9666366 (head) | 3/6 | 19.3-19.8 s passing; failing at pipe 50, 56, 33 (109-337 s) |
| ee2122829 (SCHED5 merge, same code) | 1/4 | the pass had kernel probe prints on |
| 6a44d41a3 (train head) | 0/4 | orchestrator's 3 plus mine |

Scratch boot-class copies with kernel counters passed (base 13.8 s, head 15.5 s). The range-call
counters for the VM over the whole run: map_anon 701 ms at base, against 446 ms map_anon plus
146 ms map_fixed at head. Nothing pathological.

## Evidence (kernel probes; scripts .tmp/SCHED5/instr{2,3,4,5}.py, logs .tmp/SCHED5/q64/)

- **Thread create/exit.** The VM starts one waiter a pipeline, and each exits after its pipeline
  ends. One waiter never exits, and no waiter starts after it.
- **Calls of threads 13 and up.** A normal waiter's end is `call` (it fails Dead), then `send`
  (the last hand-over), then `handle_close` and `thread_exit`. The stuck waiter's last entry is a
  `call`. The hart goes idle right after, so it is blocked in the kernel, not spinning.
- **Probe 5** (`.tmp/SCHED5/q64/rv32-pipe-sixty-four.smp1-p5-2-1.log`): every waiter call in the
  run, across all pipelines, goes to endpoints owned by one budget, 5438, which never dies.
  - A normal ending: piped's thread holds the parked call, `end_thread` finds it and wakes the
    caller with Dead ("served caller (168, n) waiting true" for each earlier waiter).
  - The stuck waiter (tid 42): piped (pid 200, budget 31075) ends with `ncalls 0`, before the
    waiter's next `call`. No Dead wake ever names (168, 42). The call is queued on a live endpoint
    that no process can receive from again.

## Why the endpoint outlives piped

`userland/otp/redoubt/src/jobs.rs` `served()` creates the child's receive endpoint in the VM, so
it is charged to and owned by the session's budget, and hands the receive right to the child.

When the pipeline ends, `budget_destroy` of piped's budget fails what that budget's own endpoints
hold (`endpoint_dying`) and the dying threads' open calls (`end_thread`). It does neither for a
session-owned endpoint. The handle the waiter calls through (the connection's, aio.rs
`spawn_waiter`) names that endpoint, so it is not revoked.

`Io::forget`'s doc assumes the opposite: "The server is dead, so its waiter's completion call fails
at once." That holds when the waiter's call was parked in piped at the death. It fails when the
waiter is between hand-overs (answer, `send`, `map_anon`) and calls again after the death. The
probe shows a 4 ms call/send/map_anon cycle while output streams. The window is the time from
piped's last receive, through its death, to the waiter's next call.

## Options (a design decision: beamlet platform, kernel IPC; not SCHED5's files)

1. **The child's endpoint owned by the child's budget.** Created in it, or by the child itself and
   its send right returned. Then destruction fails queued calls (`endpoint_dying`) and revokes the
   waiter's handle, so a later call fails at once. This closes the race. It needs a way to create
   an endpoint in a child budget, or a start protocol change.
2. **The VM keeps the receive right.** In `forget`, after `budget_destroy`: stop new calls, then
   drain and refuse what is queued. This is racy unless new calls are stopped first, and closing
   the shared handle index has the reuse hazard `forget`'s doc names.
3. **Kernel: a call on an endpoint whose every receive right is gone fails Dead.** The kernel does
   not track receive rights today. This is the largest change.
4. **Waiter call with a timeout instead of `FOREVER`.** This does not fix the cascade: `COLLECT_WAIT`
   is 10 s, so any bound is past the next pipeline's `FILE_WAIT_US`.

Recommendation: (1), as its own node (Tier A: platform plus the start protocol, with an attack or
host case). The case's test for it: pipe-sixty-four on rv32, and a host test on the fake kernel
where the server dies between the waiter's answer and its next call.
