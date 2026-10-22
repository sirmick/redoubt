# K37 design note: a server's turn is one kernel entry (k37-implementer, 2026-10-22)

Base main b6d28a6b6, branch wp-K37, nothing built. For the go, and for the Architect on the ABI
questions marked **Q**.

## What the hops are, and why only a combined entry removes them

K35's naming (K35-report.md "Naming"): about 5 of the 10-11 lock hand-offs a probe follow an entry
that readies a thread and leaves its runner running: netd's serving thread's `reply` to ipd's
transmit call (1.0-1.7), netd's rx thread's `send` to a waiting ipd (1.0-2.7), and ipd's `receive`
taking a queued send, which readies netd's blocked sender (1.0-2.8). Rule (a) (scheduling.md,
preemption points) keeps a readied thread on this hart only when the runner blocks in the same
entry. Each of those runners blocks in its next entry anyway (netd's `receive`, netd's
`wait_irq`), so fusing the two entries makes rule (a) reach them. The third (ipd's receive readying
netd) goes away because netd, once its send is taken, goes straight to its interrupt wait inside
ipd's entry and is never readied at all.

## The calls

There are two new calls, numbered after `console_hold`: `reply_receive` (0x11e) and `send_receive`
(0x11f). Each one is "the first half, then `receive`" in one entry.

Registers. `a1`-`a5` are `receive`'s, unchanged: the endpoint or IRQ handle or 0, the timeout (2),
`max_transfer` and `received_rec`. `a6` is `first_rec`, the first half's record. `a7` is 0.
Seven registers cannot hold both halves' arguments: a reply needs 3 and a send needs 6. So the
first half goes in one new record, the **first-half record** (13 slots), whose unused slots are 0:

| slot | `reply_receive` | `send_receive` |
| --- | --- | --- |
| 0 | message id (non-zero) | endpoint handle |
| 1 | 0 | the send's timeout (µs, relative; FOREVER allowed) |
| 2-3 | 0 | transfer address, pages ((0,0) none) |
| 4-12 | the reply body (the 9-slot body record's layout) | the message body |

Why two calls and not a flag on `receive`: each call's error row is then the sum of two rows that
already exist, `Number::can_return` stays one set per number, and `receive`'s row and the
legacy-gone sweep (a non-zero `a6` is `InvalidArgument`) do not change.

Results. `a0` is 0 or the error code. `a1` is the **first-half disposition** and is set on every
return, errors included, as `call`'s dispositions are. `a2` is the reply's installed-handle mask
(`reply_receive` only). `a3`-`a7` are 0. On success the receive record is written exactly as for
`receive`.
- `reply_receive`, `a1`: 0 *not replied* (the call is still open and nothing changed), 1
  *discarded*, 2 *delivered*.
- `send_receive`, `a1`: 0 *not sent* (the transfer is the sender's again, or was never given up),
  1 *taken*.

`decode_result` refuses any other combination: success with `a1` = 0; a mask bit unless `a1` = 2,
or one past bit 3; `a2` ≠ 0 on `send_receive`; and with `a1` ≠ 0, an error outside the second
half's delivery stage (Timeout, Dead, InvalidArgument).

## R13: one outcome, whole

The order of checks is chosen so that every error the first half can earn comes before anything
is done. Stage 1 decodes `receive`'s registers, then `first_rec` (the record check, read), then its
slots, in slot order. Stages 2-4 then check `receive`'s handle (BadHandle, WrongObject,
NotPermitted for a badge) and the first half's own stages 2-4 exactly as `reply`'s or `send`'s
row (for a send: LabelDenied, Busy, ...). Any of these returns `a1` = 0 and leaves nothing done:
the current call is not cleared (R21), and no transfer is given up.

Then the first half acts.
- **Reply.** Nothing after `reply`'s checks fails (its row says so). Its delivered/discarded and
  mask are fixed here.
- **Send.** If the receiver is waiting, the message is taken in this entry. If not, the thread
  waits as a sender (Wait::Send, the send's deadline). Refused, Timeout or Dead end the call with
  `a1` = 0 and the transfer given back, exactly as `send` does today.

Then the receive half runs as `receive` does. It clears the current call (R21; after a reply to
the current call that clear already happened), runs the record check at delivery, and blocks or
takes. Its errors (Timeout, Dead, InvalidArgument for a record gone bad) return with `a1` ≠ 0,
which names what the first half did: "a failed receive after a delivered reply" is one outcome,
`(Timeout, delivered, mask)`.

**The send's continuation.** When a blocked `send_receive`'s message is taken (`deliver`'s Send
arm, in the receiver's entry, maybe on another hart), the sender is not woken. Its receive half
starts there:
- an IRQ that has already fired answers it (it is readied, with its record written, as any
  delivery);
- otherwise it is marked waiting (Receive or Irq, its own timeout counted from now). It joins the
  endpoint's receivers, and the endpoint is put on the pumps list (`pump_listed`) rather than
  pumped inside the current pump, so one entry never nests pumps and R12's bound stays one
  pump's.

The receive half's object is resolved when the call is decoded and kept in the thread's IPC page:
six new words (kind, object frame and id, timeout, max_transfer, rec). There is room below
`W_LISTS`, which I will check against CONTEXT_OFFSET. An endpoint destroyed during the send is
`Dead` with `a1` = 1, as it is for a blocked `receive`.

Unchanged: R4b (a server dying with open calls), R3 (lends and abandoned calls, since a reply
half is `reply`), R4 (Refused, transfer limits), R2 (a `send_receive` counts in its group's
WAIT_CAP like a send), R14, I13 (every wait has its deadline).

## The model, the fake and libs/sys

The contract is one table, with the kernel, the model and the fake identical.
- **libs/sys.** `Call::ReplyReceive { msg_id, body, from, timeout, max_transfer, received_rec }`
  and `Call::SendReceive { endpoint, body, transfer, send_timeout, from, timeout, max_transfer,
  received_rec }`; `first_rec` is built by the encoder from `body`. `Return::Turn(TurnOutcome {
  status, first, mask })`. FirstRecord encode/decode with the malformed-record refusals.
  `can_return` rows. Round-trip, malformed and random tests extended; the fuzz decode target
  covers the record.
- **The model (model/src).** Both calls are modelled with the same order of checks and the
  continuation. Its R13 check covers the combined result (the first-half disposition agrees with
  what the ghost state did). The generator fuses a drawn `reply`/`send` with the same thread's
  next drawn `receive`, decided by a bit of a value already drawn: **no new RNG draws**, seeds and
  model-mutations' deadlines unchanged. I will name a bit and show that fixed seeds give the same
  traces apart from the fused pairs. New mutations: `TurnReplyLost` (the reply half skipped when
  the receive blocks) and `TurnSendDispositionLies` (`a1` = 1 after a refused send).
- **The fake (libs/rt/fake).** Both calls, same results, so rt host tests run against them.
- **The kernel.** redoubt.rs dispatch; message.rs: `reply_receive` = `reply`'s body + `receive`;
  `send_receive` = `send` + the continuation in `deliver`/`fail_wait`. Checked builds assert the
  disposition against the row.

## The rt: the one consumer

`Request::reply` is the one place an rt server replies, and `receive_raw` the one place it
receives. Nobody outside the rt reads a handle-free reply's `ReplyOutcome`. Only
`new_connection` and keyd's `grant` read an outcome, and both send a handle, because the outcome
decides a rollback (serving.md "Replies and rollback").

Shape I propose: a serving loop holds at most one **held reply**, a handle-free answer, and makes
it with its next receive as `reply_receive`.
- `typed::finish` hands back a handle-free answer as held when its caller is a turn-aware loop,
  and makes every reply that carries handles at once, as today (rollback unchanged).
- A held reply whose half is refused (`a1` = 0) is completed with finish's Malformed fallback and
  then the plain receive, so serving.md's rule holds.
- Loops: rt `server::serve`, ninep's run loop, netd's serving loop, ipd's loop, and the test
  log-server (`tests/programs/src/logsrv.rs`, raw rd, so `ipc-client` measures it).
- netd's rx thread forwards its last drained frame with `send_receive` on its IRQ handle (K35's
  (b), moved here). Earlier frames go by plain `send`.

**Q-rt** (implementer's choice, stated): explicit in the loops (the handler's answer is returned
to the loop) and not hidden per-thread state. The rt has no thread-local slot, and a hidden
deferral would delay a reply behind whatever user work its handler does next. The cost is a small
signature change in each loop's handler.

## Acceptance as I read it, and one question

- **Q-red.** launch-idle-probes asserts interrupts and SBI calls per frame at twice the measured
  worst. Removing ~5 hand-offs (about 10 interrupts plus SBI calls a frame, of 37-42 / 48-53)
  cannot bring a 2x ceiling below main's figure, so those ceilings, re-derived, cannot be red on
  main. What can be red on main is **user ecalls per frame** (worst + 1, today 33.1): the fused
  entries remove 2 a probe (netd's reply + receive and netd's send + wait_irq become one each), so
  ~30-31 against main's 32.0-32.1. I propose:
  - the user-ecall bound is the red-on-main witness;
  - the re-derived interrupt and SBI ceilings are guards;
  - the hand-offs per probe by entry are reported by the instrumented naming (K35's method) and
    stated on ipd.md, not judged.

  The alternative is a judge counting lock hand-offs (m_software at acquire_ticket) per frame at
  worst + spread instead of 2x. I need the Architect's ruling on which.
- **ipc-client.** 1000 lend round trips, two harts against one, within 1 % (2.07 against 2.02 s
  today). scheduling.md's residual "A server's turn crosses harts" is rewritten.
- **Counts kept:** sched-latency, sched-irq-wake, sched-wake-no-preempt(-harts); the idle ceilings
  do not move.
- Load runs and quiet slots are asked for first.

## Coordination

K39 owns sched.rs slice accounting. K37 should need no sched.rs code, since rule (a) already
covers it. On scheduling.md K37 touches only the residual "A server's turn crosses harts" and the
preemption-points sentence naming two-entry turns. Hotspots K37 writes: message.rs, redoubt.rs,
libs/sys, model, the rt loops.

Pages: ipc.md (The calls, How a call completes, R13 text), abi.md (table, registers, records,
order of checks, unknown numbers 0x101-0x11f), model.md, scheduling.md residual, ipd.md probe
cost, serving.md (held reply), netd's page (forwarding last).

Size M. Tier A: kernel-red, simplifier and editor.
