# SCHED1: the positive-lead category under a 1 ms slice (architect-15, 2026-10-05)

QA thread `IPC3-wake-latency`. Evidence: the one v3 candidate run, rv64, seed 3, HEAD
`18818cb14`, `.wash/local/evidence/SCHED1/candidate-v3-seed3-rv64-result.md`. Failure:
`cluster driver_wake positive-lead coverage [0, 0, 0, 0], total 0`. Everything before that gate
passed, including zero-lead coverage.

## (c) The oracle is not wrong

`tools/testbench/src/sched_oracle.rs` at `18818cb14`, lines 1131-1141: `lead = W.pass - floor`
with the floor the independent replay carries (`floor = floor.max(min)`, line 1198), and `ahead`
the spinners whose replayed `(pass, rank, id)` is strictly below the W's. That is the layout's
definition, applied as written. The category is empty because no positive-intent wake had eight
spinners ranked ahead, not because the oracle miscounted.

## Why it is empty by construction, in stride units

`libs/stride`: `pass += runtime * STRIDE / weight`, `STRIDE = 2^20`; the run calibrated 10
ticks/µs. One full slice moves a pass by:

| Budget | 1 ms slice | 10 ms slice |
| --- | --- | --- |
| spinner, weight 100 | 100 · 2^20 | 1000 · 2^20 |
| stand-in, weight 1000 | 10 · 2^20 | 100 · 2^20 |

The ≤100 µs release spread the oracle proves is 10 · 2^20 at weight 100.

A budget is picked only when it ranks lowest. So when the stand-in blocks after its preparation
spin, its pass is at most the lowest spinner's pass plus one own slice: lowest + 10 · 2^20 under
1 ms. It then waits at most 850 µs, during which the floor can only rise. For eight spinners to
rank ahead of it at W, the eight lowest spinner passes must all lie within 10 · 2^20 of the
lowest, that is within 100 µs-equivalent of each other. The spinners are that tight only at the
release instant; from then on each runs 1 ms slices that space their passes by about
100 · 2^20, and the first positive-intent attempt is 320 ms after release. So `ahead` is about
one or two at every positive wake, and ≥8 is unreachable for a weight-1000 waker at 1 ms,
whatever it owes.

Under 10 ms the same reasoning gives lowest + 100 · 2^20, one whole spinner slice, so the waker
sits behind nearly the whole cluster: that is the attack IPC3 exposed, and why the old control is
expected to show `ahead` near 16. The ≥8 count is the 10 ms attack's signature. Its absence under
1 ms is the mechanism working, which the layout ruling's "positive lead has ≥8 spinners ahead"
did not foresee: it measured the slice, not the debt.

## Ruling

Option (a), by steps, with (c) first as a check and nothing tuned.

1. **Host replay only.** Re-run the oracle on the saved console log (the evidence directory,
   byte-identical to the run) with a diagnostic that prints, for every positive-intent sample
   of both stand-ins, `(sample, offset, floor, lead, ahead)`. No machine run, no fixture, kernel
   or model edit; the diagnostic is not committed. Report the distribution of `lead` and
   `ahead` per stand-in and per offset.
2. **If `lead > 0` holds for ≥25 positive-intent wakes and ≥5 per offset, per stand-in:** the
   category rule becomes *positive lead: `W.pass` above the replayed floor*. `ahead` stays a
   recorded witness and is reported as a distribution (min, median, max) for each category, in
   the candidate and the control alike, by the same oracle with no new parameter. The control's
   non-vacuity is judged from that report: at least 25 of its positive wakes per stand-in must
   show `ahead ≥ 8`, else it is not the required negative control. Unchanged: the zero-lead
   rule, 25 per category and 5 per offset, latency over all 200 samples, every numeric gate,
   64 MiB and zero drops, the fences and joins, and the proposal's stop rules. Write the change
   and this reason into `SCHED1-layout-ruling.md`'s category paragraph and the report; the
   `sched-cluster.toml` description and the testbench page follow the new words.
3. **Re-judging, not retrying.** The oracle is a host post-check; the fixture and kernel are
   unchanged, so the saved rv64 log re-judged by the amended oracle is the candidate's rv64
   verdict, with the source identity recorded as the result file does. rv32 and the controls
   are machine runs under their grants, as before.
4. **If `lead > 0` is also short:** stop and return here with the distribution. Changing the
   stand-ins' weights, the slots or the offsets to obtain coverage is not authorized; that would
   be a redesign question.

Not an owner choice: the mechanism, the latency targets and the sample population are untouched.
This amends the Architect's own proof gate on evidence that it was measuring the old slice. It is
disclosed in the report.
