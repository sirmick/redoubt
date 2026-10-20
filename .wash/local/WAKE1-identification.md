# WAKE1 phase 1: identification (wake1-implementer, 2026-10-20)

Branch `wp-WAKE1` at a04108b06 (WIP, on origin/main ddafcad7d): test-only instrumentation, none
of it to keep as is. One diagnostic boot, rv64 one hart, `tests/deadline-burst-diag.toml` (the
burst case with `block-trace`, the shell's receive stamps and three timed file reads), PASS in
182 s, oracle green. Evidence: `.worktrees/WAKE1/.tmp/WAKE1/diag4/deadline-burst-diag-rv64-smp1.log`
(console, 1.32 M trace records) and `...-alice.ssh.log` (session); the readers
`.worktrees/WAKE1/.tmp/WAKE1/analyze.py` and `chain.py` (run them on those two files; `111` as
the third argument of analyze.py). Two earlier runs of the same boot idled to their timeout on a
session step of mine (a kill not yet processed, a wrapped result line); their session logs
(`diag1-stuck-alice.ssh.log`, `diag3-stuck/`) repeat the read timings below. SHELLN1's `diag/`
logs went with its worktree; its finding file is `.wash/local/SHELLN1-burst-finding.md`.

## (a) The session VM's budget id is read, not inferred

Under `block-trace` (kernel feature, implies `sched-trace`, no judged build sets it) the kernel
now records three more kinds, admitted by the oracle's parser and ignored by its checks:
`#` a budget's creation (`budget_create`: id, parent id, creating PID), `~` a wait's wake
(`message::wake`: budget, wait kind, timed out, answered without blocking, endpoint frame),
beside the existing `@` block. From the `#` records: PID 434 carves alice's tree (21 budgets) and
is the steward; `98 <- 93` at boot 3.6 s is the console session (Login audit at 4.0 s);
**`111 <- 93` at boot 15.5 s, weight 100 (`H` record), is alice's ssh session** (Login audit at
16.3 s; sshd's `login alice` line after it); `127..147 <- 93`, weight 50, the six leases, each
with one child. 86 is the steward's own budget (weight 1000, carved by init). So SHELLN1's
inference (111) was right. The id is in the trace only; no probe or console line names it, and no
syscall exposes a kernel budget id to user space (`budget_usage` returns limits and usage).

## (b) The three arms, from the trace

The forward's target and the idle receive are one endpoint: budget 111 blocks in `receive`
(wait 3) on frame **31395** 103 times and is woken there by a send 102 times; its zero-wait
polls (`take_waiting`'s `receive(0)`, answered `Timeout` in the caller's own call) are 1354 on
the same frame; its blocked sends (wait 1) are 136 on the same frame, each later taken by a poll
(136 polls that found a message). No other budget touches 31395. **(i) excluded**: the idle
sleep is a receive with the next timer's deadline on the frame the serve thread sends to, not
another endpoint, and not a timer wait.

A receiver with a deadline is woken by a send in the sender's kernel entry: `send` queues the
message and `pump` delivers it to the waiting receiver, `wake` readying its thread, before the
sender's own `settle` (message.rs); the trace shows each pair in one entry (frame 36401: 868
receives woken, 868 sends answered without blocking, same entries). What follows the wake is a
pick: over the boot, from the VM's idle receive woken by a send to the next pick of budget 111,
**2.9 ms median, 10.6 ms worst, n = 101** (one slice: a wake never preempts, scheduling.md
"Preemption points"; `sched-irq-wake` is that rule's case). Existing case for the quantity:
`sched-latency`'s `deadline_notice`, the steward's `receive` with a deadline woken by the
kernel's exit notice, p99 <= 40 ms under sixteen spinners (scheduling.md "Responsiveness");
and each burst's first notice with the shell idle, 10 to 14 ms (STEWARD12). **(ii) excluded**;
no new ipc case is needed for it.

The serve thread is runnable between its receive and its forward, and waits only for the hart:
the first pick of 111 after each `lease_ended` send return is 35, 11, 38, 1, 1, 10 ms later
(`chain.py`, "K111 picks"), the budget's turn at weight 100 behind the steward (1000) and the
weight-1000 servers, not a lock. **(iii) does not hold as stated**; its cost is the share below.

## What holds: the VM is not idle, and a busy VM takes a forward once per Erlang slice

In the burst window (+0.35 to +1.35 s after the deadline) budget 111 is queued and runnable
throughout: requeued (`R`) at every slice end, never dequeued (`D`), picked for 1 ms turns; the
hart is never idle (no runner-less timer entry in the whole boot; `H0` is the transient return
to kmain before each pick, which SHELLN1's "hart idle" read as idling). The hart in that
window: **61 % inside the checked kernel's audits** (`U`/`V`, ids 3 and 4: the IPC lists and the
scheduler's marks, about 1.4 ms per audited entry), 111 running 26 to 28 % of the rest's time
(gaps 2, 3, 5: 72, 77, 52 ms of 229, 179, 161), the steward 86 and the periodic servers the rest
(`analyze.py`, "occupancy per gap").

With the VM's scheduler thread running the shell's listener (which on main prints each notice
inline), the serve thread's forward finds no receiver and blocks on 31395 (`@` wait 1); the VM
takes it at its next `next()`, where `poll_console` -> `console_read` -> `take_completed` polls
the endpoint, which comes once per 2000-reduction slice of the running Erlang process. In the
burst the ten forwards that blocked waited **107 ms median, 126 ms p90** (n = 10; the rest of
the boot 7.5 ms median); SHELLN1's split-listener `recv - send` of 43 to 193 ms is this wait.
At full speed a 2000-reduction slice is about 48 ms on this VM (SHELLN1's figure); at a quarter
of a hart it is the 100 to 140 ms measured. On main's unsplit listener the Erlang-side stamps
are larger and growing, `recv - send` 76, 344, 331, 440, 715, 866 ms (about 300 ms more per
notice: the previous notice's inline print through the relay, the cause the Architect first
named and SHELLN1 removed), while the steward's sends still return every 85 to 230 ms, the VM
thread's take cadence, not the listener's. So the burst's two terms, in the tree as it is: the
shell's inline print (listener-side, the larger), and the VM's slice-granular take of a
forward at its hart share (platform-side, 100 to 140 ms per notice here).

## (c) A call's reply to an idle shell

`File.read("/home/alice/project/in")` timed on the guest clock from the shell, eight each:
VM idle before the burst **0.96 to 1.31 s** (first 1.31, then 0.96 to 0.99); after the burst
0.99 to 1.16 s; beside a spinning process (`Enum.reduce` over 100 000, alive throughout) **1.37
to 1.53 s**. One read is one wake-up at the VM (9 idle wakes over the 8 idle reads); idle, the
wake is 2.8 ms of the second (the 1 s is the servers' side on the checked, traced kernel under
icount); busy, the forwards that blocked in that window waited 3.4 ms median (n = 7 to 9, worst
102 and 298 ms), so the +0.4 s is mostly the read's own VM work at half the VM's reductions, not
the take. A reply to an *idle* VM does not carry the burst's wait; a reply to a *busy* one waits
up to one Erlang slice, as the forward does.

## For the Architect

The premise "the VM's idle is not ended by the forward" is wrong: the idle is ended at once by
the kernel, and the VM is not idle in the burst. The platform-side residual is the poll-per-slice
take of a forward by a busy VM at a weight-100 budget's share of a hart that the checked build's
audits take 60 % of; candidates I see, not ruled: a shorter poll interval when wake-ups are
pending (the platform knows of the blocked send only by polling; the kernel offers no "a send
waits" signal short of a receive), the VM's weight, or leaving it as the measured cost of a
weight-100 session beside weight-1000 servers. What to keep of the instrumentation is also the
Architect's call: `~` and `#` under `block-trace` are two records and an oracle allowlist line;
the shell stamps and the scratch case are not for the tree.
