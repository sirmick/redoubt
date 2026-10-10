# IRQ1: the 4-hart tail, instrumented (wp-IRQ1, probes since removed)

Probes (throwaway, sched-trace only): at each idle halt, `sip`, `sie` and the PLIC's pending bits
when the hart halts, and `sip` and pending/priority masks when it wakes; every mask, unmask and
idle-window claim with its time. Runs: `q run --cores 8 -- cargo testbench sched-lock-contention-4`
(both widths PASS), logs .worktrees/IRQ1/.tmp/lc2, lc3.

## Finding 1 (IRQ1's, fixed): `wfi` with an interrupt already pending
rv64, 30 of the idle halts over 15 ms began with SEIP pending and enabled (sip 0x200, sie 0x222,
the RTC's source 11 pending and unmasked) and still lasted 28.5 ms, waking with SEIP+STIP. The
architecture lets `wfi` return at once then; QEMU's helper halts the hart regardless and ends its
turn, and under `-icount` round-robin the hart runs again only after every other hart's turn, here
three 9.6 ms searches. Fix: `arch::halt_masking` skips `wfi` when `sip & sie` (less the mask) is
non-zero. Harmless on hardware.

## Finding 2 (not IRQ1's): a line that rises during a halt waits for the other harts' turns
After the fix (lc4), the long halts all began with nothing pending (rv64 45: 28 woke on SEIP, 9 on
the reschedule interrupt, 8 on the timer; rv32 25). The line rises on time in virtual time, but
QEMU's single-threaded round-robin (`-icount`) runs the woken hart only when the running hart's turn
ends, and a hart in a kernel section keeps its turn to the section's end; then it draws its ticket
behind up to three sections. On hardware the woken hart runs at once; the kernel cannot shorten
another hart's QEMU turn.

## Numbers after the fix (lc4), driver wake net p50/p99/max µs
- 2 harts rv64 8553/10622/10622, rv32 8521/10474/16949 (bimodal: 0-2 ms between searches, ~10 ms
  inside one; p50 is whichever mode holds 100 of 200: lc1 had 1.5/1.7 ms; both under 15 ms).
- 4 harts rv64 4430/49702/49741, rv32 5554/40634/41510 (were 3.5/58.6 and 5.4/58.6 before the fix;
  on main 4.3/59.3 and 7.1/86.9). rv64's p99 is 0.3 ms under 50: too close to gate.

## Proposal (option a)
Gate p50 and p99 at 2 harts, p50 at 4 harts; record the 4-hart p99 with a residual naming finding 2
and a follow-up (e.g. a multi-threaded TCG or hardware measurement of the 4-hart wake).

## Finding 3 (kernel red, for the follow-up): every hart traps for each device interrupt
An unmasked source (priority 1) raises SEIP on every hart's context, so every hart in user mode
or idle when the line rises traps; one claims, the others take the kernel lock for a claim that
finds nothing. Counts from the checked build's claim line (gate on 6ceb71fa1): 4 harts rv64 1,
rv32 149 empty claims a run; 2 harts 88 and 77. Each is a short section, but at four harts it is a
QEMU turn and a ticket ahead of the woken hart, so it feeds the 4-hart p99 beside the round-robin
turns. Remedy if it matters: route each source to one idle hart (raise a busy hart's threshold
above the sources' priority while some hart idles).
