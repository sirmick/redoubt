# SHELLN1: the listener split does not move the burst; the serialization is not the shell's print

Branch wp-SHELLN1 at 0d80aac5e (on origin/main c01146946, STEWARD12 in). The shell's notices
listener now only receives, decodes and queues each send to a printer process (host-tested on
beamlet and BEAM: eight sends taken before a stuck printer printed one). The burst cases were
rerun at the same seed with the prebuilt remade; logs beside this file in `after/`
(`steward-deadline-burst-<width>-smp<N>.log`, the session's `-alice.ssh.log`).

## Measured (gross µs from the deadline; "before" is STEWARD12's table, same seed)

| boot | before: first / last | after: first / last | after: gaps between send returns, ms | `[lease ended]` lines |
| --- | --- | --- | --- | --- |
| D=6 rv64 1 hart | 595,093 / 1,424,085 | 526,249 / 1,556,347 | 73 223 138 123 148 | 6 of 6 |
| D=6 rv64 2 harts | 488,810 / 1,450,583 | 471,402 / 1,227,597 | 376 215 219 193 234 | 6 of 6 |
| D=6 rv32 1 hart | 640,248 / 1,543,325 | 642,787 / 1,718,608 | 86 234 150 144 163 | 6 of 6 |
| D=16 rv64 1 hart | 673,343 / 1,842,096 | 872,296 / 2,174,528 | 73 147 73 59 183 63 65 115 67 61 116 67 60 112 66 | **15 of 16** (agent-8 lost) |
| D=16 rv64 2 harts | 971,861 / 1,924,564 | 889,698 / 2,601,708 | 122 114 170 107 101 138 108 86 120 99 89 125 90 104 133 | 16 of 16 |

Every case passed as STEWARD12 landed it (no bound, no line count). The shape is unchanged:
each `lease_ended` send still returns 60 to 380 ms after the previous, and one send of sixteen
was dropped at the steward's bound on rv64 at one hart (STEWARD12 saw one of six there). The
house-rule bound from these figures would be 4 s for six and 6 s for sixteen (twice the worst
last notice, rounded up), not the ms the ruling expected, and the all-N assertion fails on
rv64 at one hart as the ruling framed it.

## What this rules out, and what it does not

- Ruled out: the listener process's receive as the gate. It no longer prints or re-binds; the
  printer's work is a separate process, and the gaps did not move.
- Ruled out by reading: a page cap on the VM's take (`COMPLETION_PAGES = MAX_LEND_PAGES`; the
  forward is one page of 256-byte header plus the steward's page).
- The path a send takes, from `userland/otp/redoubt/src/serve.rs` and `lib.rs`: the steward's
  `send(..., NOTICE_US)` rendezvous with the VM's serve thread (`endpoint.receive`); the serve
  thread forwards it to the VM's own wake endpoint with `wake.send(..., FOREVER)`, another
  rendezvous, and only after that returns does it receive the steward's next send. The VM
  takes wake-ups with a zero-wait receive in `take_completed`, called from `Platform::idle`
  and from `console_read`, which the scheduler's `poll_console` calls on every `next()` while a
  console reader exists (the shell's driver, always). So by the code the forward should be
  taken within one scheduler turn whatever any Erlang process does, and the 60 to 380 ms per
  send is either the VM not reaching `next()` / `idle` for that long (a long native, a long
  slice at the icount rate: 2,000 reductions is about 48 ms at 42,000 reductions a second), or
  the kernel not running the VM's thread for that long after the serve thread's send, or the
  cost of the steward's send itself (the first send of each burst is 10 to 14 ms when nothing
  queues behind it). The D=16 one-hart gaps alternate 60-70 ms and 110-180 ms, a regular shape
  that looks like a slice or a timer, not like a print's variable cost.

## The one cheap measurement that would split it

Stamp the guest clock (`System.monotonic_time`, the kernel's `time_now`) in the shell at the
listener's receive of each notice and at the printer's print, write them to a file in the
session's home at the case's end, and set them beside the probe's send-return times. If the
listener receives each notice at about the previous send's return, the gate is between the
steward's send and the VM's take (the serve thread's forward, the VM's turn, or the kernel);
if the listener receives them in a bunch long after, the VM is not running its scheduler loop.
One rv64 one-hart burst boot (~2 min of case) with a test-only stamp the branch does not keep.
Not run: the orchestrator ruled a lesser-than-expected result a finding for the Architect, not
a tuning, and the instrumentation is the Architect's call.

## State of the package

Built and gated on main before the rebase: the listener and its two tests (test-shell, 8 passed
on both VMs), docs and formatting cases, the narrowed shell set (9 cases × 2 widths, all rc=0,
`gates-before-rebase.md`). Rebased onto c01146946 as 0d80aac5e, conflict in steward.md's Leases
bullet resolved keeping both sides. Not done, pending direction: the burst tomls' bound and
line-count assertions, STEWARD12's residual text on the pages (steward.md 1584-1593 and
1691-1692, scheduling.md 818-825, steward-latency.toml's comment 150-153), which this result
does not let me replace with a ms bound.

## The diagnostic boot (scratch commit 0c631b195, dropped after; logs and timelines in `diag/`)

The shell stamped the guest clock (`System.monotonic_time`, the kernel's `time_now`, the probe's
clock) at the listener's receive of each notice and at the printer's print, printed in the line.
rv64, same seed, both hart counts; `diag/timeline-smp1.txt`, `timeline-smp2.txt` hold the full
alignment and the kernel's records across the two longest gaps.

ms after the deadline (send return = the probe's `deadline_notice` sample, the steward's send
complete; recv = the listener received it; print = the printer printed it):

| rv64 one hart | send return | recv | recv − send | print | print − recv |
| --- | --- | --- | --- | --- | --- |
| agent-1 | 519 | 563 | 43 | 752 | 190 |
| agent-2 | 610 | 761 | 150 | 952 | 192 |
| agent-3 | 815 | 950 | 135 | 1241 | 291 |
| agent-4 | 956 | 1008 | 52 | 1389 | 382 |
| agent-5 | 1044 | 1237 | 193 | 1829 | 592 |
| agent-6 | 1182 | 1256 | 74 | 2222 | 966 |

| rv64 two harts | send return | recv | recv − send | print | print − recv |
| --- | --- | --- | --- | --- | --- |
| agent-2 | 752 | 808 | 56 | 959 | 151 |
| agent-3 | 886 | 972 | 86 | 1249 | 277 |
| agent-4 | 1089 | 1253 | 164 | 1446 | 193 |
| agent-5 | 1305 | 1445 | 140 | 1655 | 211 |
| agent-6 | 1515 | 1621 | 106 | 2071 | 450 |
| agent-1 | 1734 | 1919 | 185 | 2374 | 455 |

What it says:
- **The lag is between the steward's send returning and the listener receiving**: 43 to 193 ms
  at one hart, 56 to 185 at two. The steward's send is complete (the serve thread took it) and
  the next send is already decided within a millisecond, yet the VM's Erlang side sees the
  notice 50 to 190 ms later — and the steward's next send returns about when that receive
  happens (send N+1's return ≈ recv N: 610 vs 563, 815 vs 761, 956 vs 950, 1044 vs 1008, 1182 vs
  1237), which is the serve thread blocked in its FOREVER forward to the VM's wake endpoint
  until the VM takes it, then receiving the next. So the gate is the VM's take of the forward.
- **The print is slow and backlogs, and never gated anything**: each print follows its receive
  by 150 to 966 ms, the backlog growing through the burst (the printer runs a line through
  Printer.text, group, the driver's draw and the relay). The split made the listener's receive
  independent of it, as designed, and that moved nothing, as measured.
- **The hart is idle through the gaps.** In the one-hart gaps the runner recorded most often is
  none (`H0`: 62 of the runner records in gap 2, 28 in gap 4), with eight budgets each picked
  for short turns between (ids 45, 60, 66, 69, 72, 86, 98, 111; `K`/`H` tallies in the
  timelines). So the VM's thread is not starved of the hart; it is waiting.
- **No wake record for the receiving VM's budget inside the lag.** The budgets that wake (`W`)
  inside each lag are the periodic ones (86 every ~8-10 ms, 45, 66, 69, 72, 98); the budget
  running when the listener stamps its receive differs from notice to notice at the trace's
  resolution (86, 66, 98/111, 98/111, 69/60/111, 51), and budget 111, the one with the most
  picks and slice-end requeues (`I111`, `R111`), never has a `W`. I cannot name the shell VM's
  budget from the trace alone (the steward's console prints no budget ids), so which budget is
  the session's is inferred, not read; a kernel-side reader who can map the session's budget id
  would settle it from `diag/steward-deadline-burst-rv64-smp1.log` directly.
- Consistent with: the VM's scheduler thread asleep in the platform's `idle` (a `receive` with
  the next timer's deadline on its wake endpoint) and the serve thread's send to that endpoint
  not ending the sleep until a timer or another wake-up does; or the serve thread itself not
  running between its receive and its forward. Not the listener process, not the print, not the
  steward's send. The owner is beamlet's wake/take path or the kernel's receive-with-timeout, per
  the Architect's outcomes 2 and 3; the stamps cannot tell those two apart from inside the VM.
- A re-bind could not be provoked in this case (no shared server restarts in it); the claim
  that a notice no longer waits on a re-bind is unmeasured.
