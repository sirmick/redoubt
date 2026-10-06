# SCHED1: the 10 ms control that cannot take its samples (architect-15, 2026-10-05)

QA thread `IPC3-wake-latency`. Evidence: run B, rv64 control, seed 3, HEAD `14fc61bd3`,
`.wash/local/evidence/SCHED1/control-v3-seed3-rv64-result.md`. The driver took attempt 0, then at
attempt 1 (zero intent, offset 300) read B = R + 184,488 µs against a target of R + 80,300 µs:
`CLUSTER-FAIL role=17 index=1 branch=2 result=0 target_us=80300 current_us=184488`. The proposal
makes a passed target a construction failure, so no samples, no verdict, and, because the bench
stopped at the forbidden `[cluster] FAIL` line before the parent's `finish()` reached
`system_reset` (where `kernel/src/sched.rs` dumps the ring), no trace either.

## Ruling: (a), as a demonstrated negative of the construction's own kind

The proposal's three conditions on the control (construction and coverage; a missed envelope
target; a lower witness above its target) exist to exclude a control that "fails" by arming or
observation uncertainty. A stand-in that could not get to its next slot for 104 ms past the slot
is not uncertainty of that kind: B is the kernel's own clock read, taken when the budget next ran;
between its previous service and that read it did only its fixed bookkeeping, and the slot it
missed is 80 ms wide. That is a latency failure larger than any percentile the envelope could
have shown, and it is the attack IPC3 exposed: whole 10 ms slices owed to the server and sixteen
spinners. TENETS asks the harness to be checked against a known-bad run; this is the most
known-bad run the fixture can produce. (b) and (c) tune the control to fit the measurement and
lose the identical construction the proposal insists on; neither is authorized.

The negative counts only under these conditions, all checked by the oracle from the console and
the trace, never by eye:

1. **The kind.** The failing line is `CLUSTER-FAIL` with `branch=2 result=0`: a zero-intent
   attempt whose target had passed at B, with `index >= 1`, so the stand-in had already taken a
   real, trace-joined wait in the measurement (the window and plan lines must be present and the
   stand-in's attempt 0 must join as the oracle joins any sample). The timer's or the driver's
   suffices; the other stand-in's state is recorded.
2. **The miss, net of audits.** `N = current_us - target_us` (here 104,188 µs), less the
   certified audit time inside `[target, B]` taken from the trace's `U`/`V` stamps as the
   envelope already credits them, must exceed the p99 target (50,000 µs). The trace is required:
   a control without it is not the demonstrated negative.
3. **Same fixture.** The candidate took all 200 attempts per stand-in on the same fixture,
   seed and phase list (both widths), which it has. Candidate results stand; nothing reruns.
4. **The record.** The oracle prints one classified line, which the report quotes: `control:
   stand-in <role> could not take attempt <i> (zero intent, offset <p>): B exceeded its target
   by <N> µs, <A> µs of audits inside, net <N-A> µs > 50,000`. A construction failure of any
   other kind, or a net miss under the target, stays what it is: not a control.

## The one change to make it checkable

The trace is lost to the toml's `forbid = ['\[cluster\] FAIL', ...]`: the bench stops at the first
forbidden line (`tools/testbench/src/qemu.rs:404`). That pattern is redundant: the parent's
failure already denies the required `^SCHED-CLUSTER TEST PASSED$` expect, and the oracle fails
any stand-in without 200 joined samples. Remove `\[cluster\] FAIL` from `tests/sched-cluster.toml`'s
`forbid` (keep `PROGRAM HALT`), so the guest runs to `finish()` and the kernel dumps the ring. This
is not a fixture change: it alters no guest code, no measurement and no verdict (a run that
passed had no such line, so the candidate's results are unaffected; a run that fails still fails
on the expect and the oracle). The oracle gains the classification above, run on the saved log
(host replay, as ruled for the coverage question) and in the bench alike.

Then rerun the control on both widths (run B again on rv64, then rv32), one run each, stopping
at the first failure that is not the classified kind. Run C follows under its grant.

Not an owner choice: no target, mechanism or sample rule changes; the control's bar rises (a
trace-backed miss above the p99 target, net of audits) rather than falls. Disclosed in the report.
