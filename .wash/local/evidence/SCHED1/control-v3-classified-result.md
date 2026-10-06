# SCHED1 v3: the slice-10ms controls under the control ruling. Result: rv64 PASS, rv32 PASS

This followed the Architect's ruling `.wash/local/SCHED1-control-ruling.md` (option (a)) and the
orchestrator's message `2d7ba3ed`.

## Commits (wp-SCHED1)

- `0f9c1b895` sched-cluster: forbid only a halt, so a failed run still dumps its trace
  (`tests/sched-cluster.toml`).
- `39586d915` testbench: classify a control that cannot reach its slot as the demonstrated miss
  (`tools/testbench/src/sched_oracle.rs`, `tests/sched-cluster-old-control.toml`,
  `docs/kernel/scheduling.md`).
  - The oracle gains `cluster_control_classify`. In `run()`, a `cluster_old_control` log with a
    `CLUSTER-FAIL` line is judged by this classification alone; the full cluster check and the
    partial cluster samples are skipped.
  - The control's toml drops the TEST PASSED expect and keeps the plan line and
    `SCHED-TRACE-END ... dropped 0`. Without that change, a guest-reported failure would never
    reach the post-check. Its forbid is now only PROGRAM HALT, and its description states the
    classified miss.
  - scheduling.md states the classified miss. Its Responsiveness status line gains the new test
    (13 tests). testbench.md does not describe the control's verdict, so it has no edit.

The classification's rules:
- Every `CLUSTER-FAIL` must be branch 2, result 0, on zero-intent attempt `i >= 1`, with
  `target_us == i*80000 + phase(i)` and `current_us > target_us`.
- The stand-in's attempts `0..i` each joined the trace after its go: a D, its W, and a pick of
  the stand-in before its next D.
- For at least one such stand-in, `N = B - target` less the certified audit credit inside
  `[target, B)` (the envelope's `cluster_metric` credit) exceeds that stand-in's
  `<measure>_p99_us`.
- The trace is required; `parse` refuses a log without one.
- Each stand-in gets one line, in the ruling's form, and a stand-in that did not fail is
  recorded as such.

Host test: `a_control_that_misses_its_slot_is_classified_net_of_audits`. It covers:
- positive: net 94189 > 50000, with 9999 µs of audit credit;
- net under the target (heavy audit) → Err;
- the wrong kind: branch 3, positive intent, attempt 0, target not yet passed, a target that is
  not the attempt's, an unknown role;
- attempt 0 not joined → Err;
- no trace → `run` refuses the log with "no SCHED-TRACE-END".

Through the jobserver: `cargo +nightly fmt --all` (formatting applied), then
`jobserver share cargo test -p testbench --test sched_oracle_only`: exit 0, 30 passed.

## Offline re-judgment of the first run B console (no trace)

The replay harness was run with the control's arguments on
`run-v3-run-2543441-1791238310923470939/sched-cluster-old-control-rv64-smp1.log`. Result:
`ORACLE FAIL` / `no SCHED-TRACE-END line: the trace is incomplete`. No trace, no control, as
expected. The patch was reverted and the tree is clean.

## Run B again: rv64 control, seed 3

HEAD `39586d9154d539458a2c0e64a6be1f8562b359fc`, with a clean tree before and after.
`make -f .wash/local/jobs.mk -C <SCHED1> rv64/sched-cluster-old-control` ran from 22:18:36 to
22:21:40 UTC and exited 0.

```
PASS  sched-cluster-old-control [rv64, smp=1]   6.1s
sched_oracle: 17852 records, 1666 picks in rank order, every wake at or above the floor, ...; 1859 audits, 391158 µs; 1543 timer interrupts billed by the rule (..., 0 nobody's)
control: stand-in driver_wake could not take attempt 1 (zero intent, offset 300): B exceeded its target by 104188 µs, 1512 µs of audits inside, net 102676 µs > 50000
control: stand-in timer_wake could not take attempt 1 (zero intent, offset 300): B exceeded its target by 109853 µs, 3381 µs of audits inside, net 106472 µs > 50000
```

(The bench prints the oracle's report entries after the label "walks, net of audits", as for
every case; the control lines are among them.)

The console shows:
- `CLUSTER-WINDOW 825495 875495 16875495`;
- `CLUSTER-FAIL role=17 index=1 ... target_us=80300 current_us=184488` and `role=18 index=1 ...
  target_us=80300 current_us=190153`;
- `SCHED-TRACE-END 17852 dropped 0`.

Preserved:
- `run-v3-run-2564671-1791238894703504573/` (`diff -qr` 0), with its 27-file manifest and bench
  stdout;
- console SHA256 `e174e01d9e222a2c438d3bc8764a68311ed2f0fd685c063f8923c7716f06f726`, 17,992
  lines, 647,513 bytes.

## Run C: rv32 control, seed 3

Same HEAD, clean tree. It ran from 22:21:51 to 22:23:33 UTC and exited 0.

```
PASS  sched-cluster-old-control [rv32, smp=1]   4.7s
sched_oracle: 17606 records, 1646 picks in rank order, ...; 1844 audits, 478822 µs; 1516 timer interrupts billed by the rule (..., 0 nobody's)
control: stand-in driver_wake could not take attempt 2 (zero intent, offset 600): B exceeded its target by 30416 µs, 573 µs of audits inside, net 29843 µs <= 50000
control: stand-in timer_wake could not take attempt 1 (zero intent, offset 300): B exceeded its target by 71624 µs, 2109 µs of audits inside, net 69515 µs > 50000
```

On rv32 the timer's classified miss qualifies, which the ruling says suffices. The driver's
miss, net 29,843 µs, is under the target and recorded as its state.

The console shows:
- `CLUSTER-WINDOW 835575 885575 16885575`;
- `CLUSTER-FAIL role=17 index=2 ... target_us=160600 current_us=191016` and `role=18 index=1 ...
  target_us=80300 current_us=151924`;
- `SCHED-TRACE-END 17606 dropped 0`.

Preserved:
- `run-v3-run-2567956-1791239008964486276/` (`diff -qr` 0), with its 27-file manifest and bench
  stdout;
- console SHA256 `4a067818fff5f11e212cf372d27f7f85100e1d64aac6cc56242127270997543e`, 17,748
  lines, 634,427 bytes.

## Standing

- The candidate passes on rv64 (re-judged) and on rv32.
- The slice-10ms control is the demonstrated negative on both widths, by the ruling's classified
  miss. On rv64 both stand-ins miss by about 100 ms net; on rv32 the timer misses by 69.5 ms net.
- The remaining gates are the orchestrator's to schedule: the whole bench on both widths on the
  folded head, docs, unsafe and size, and the sweep the residual-risks text needs.

MACHINE RELEASED.
