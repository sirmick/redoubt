# B102: shape (for the Architect, thread B102-shape)

Base 820d5e5ff. Read only; nothing built, nothing run.

## Mechanism

- The step (tests/steward-context-login.toml:63-64) types
  `{:ended, :redoubt.send(<dev/cons>, {[18,0,0,0], nil, []})}` and expects `{:ended, :ok}`.
- `:redoubt.send/2` is beamlet's one-way native: `to.send(..., SEND_TIMEOUT_US)` with
  SEND_TIMEOUT_US = 1,000 us (userland/otp/redoubt/src/system.rs:43, 451-463). The kernel's `send`
  queues and waits up to its timeout for a receiver to take it (docs/kernel/ipc.md "The calls").
  beamlet.md (~984, ~1047, table ~1059) states this bound: a send waits at most 1 ms, because it
  waits on the VM's own thread.
- On a context the VM's `/dev/cons` is consrelay's serving thread (not sshd's slot, as B95's
  report read it). Its loop (consrelay.rs:551-575) takes a send only in `endpoint.receive`, between
  `expire` + `turn` and each call it serves (the VM's writes, its three helper threads' calls). It
  drops the `ended` (`close_delivery`, consrelay.rs:473).
- SEND_TIMEOUT_US equals the kernel's SLICE_US (1,000 us, kernel/src/sched.rs:72). If consrelay is
  not already in `receive` when the VM sends (here it had just served the echo of the typed line),
  one slice given to any other budget, or one host deschedule of QEMU (the guest clock is the
  host's), times the send out. On one rv32 hart under a whole bench that is ordinary; alone it
  never showed (5/5 both widths).
- Production: no Elixir code sends one-way through `send/2` today. The generated `Consol.ended`,
  `Ipd.frame`, `Consrelay.hello` (libs/wire/elixir/client/) have no caller in userland or libs; the
  only callers are this case and two host tests (natives.erl, beamlet_natives_attack.erl). The
  1 ms bound hurts only this case.

## Why it hung instead of failing

1. The `expect '\{:ended, :ok\}'` step has no `within`, so the session waits for the case's
   1,500 s deadline; `{:ended, {:error, :timeout}}` matches nothing the session forbids.
2. The FAIL names the wrong session. At the deadline the mark-waiting session (bob.work+x, polling
   every 50 ms in `Shared::wait`, tools/testbench/src/ssh.rs:407-420) fails first and calls
   `shared.fail()`; the stuck session (bob.work-again) then stops as `Aborted`, which the report
   skips (ssh.rs:566-578). A mark-wait timeout is always the consequence of another session's
   failure, yet it is the one reported.

## Proposed shape (recommended: A + B; C held for your ruling)

A. The case, not the contract. Leave SEND_TIMEOUT_US and beamlet.md's send/2 rule as they are: a
   timeout is the documented answer to a receiver not in `receive` within 1 ms, and the caller
   decides. The step retries until the relay takes the send, stopping on any other answer:
   a bounded loop (e.g. up to 200 tries, `Process.sleep(10)` between, so ~2 s plus load) that
   prints `{:ended, :ok}` once taken and the last error otherwise. The property tested is
   unchanged (the relay took `ended` and the session answers after it); only a send never taken
   no longer passes as a hang.
B. Fail fast in the case: the session forbids `\{:ended, \{:error` (checked per line, ssh.rs:1019),
   and a `{ within = 60 }` before the `{:ended, :ok}` expect, so a send never taken fails in
   seconds and names bob.work-again.
C. (bench, Tier A; optional) A session that times out waiting for a mark does not abort the others
   and ranks after their failures, so the report names the session that was actually stuck. Host
   test in tools/testbench. I would leave this out unless you want it in B102.

Rejected: raising SEND_TIMEOUT_US (Tier A, blocks the VM's scheduler thread longer for every send,
and only narrows the race; no production caller needs it); a call in place of the send (`ended` is
a one-way message in the consol protocol).

Tier: A+B is a case change only (Tier B, one reviewer); C makes it Tier A (the bench).
Gates for A+B: the case alone both widths (5 each), docs, formatting; no load run unless you want
one (it would need the orchestrator's go).
Docs: the case cites steward.md/sshd.md/sessions.md; none states the 1 ms send or the step, so
no page change for A+B; beamlet.md's send/2 bound stays as written.
