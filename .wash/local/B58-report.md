# B58 report (k33-implementer, 2026-10-12)

Base db6575a3f (main). Head 2cb94d714 (amended from cde8c6691, numbers only) on wp-B58 (/home/mcloonan/redoubt/.worktrees/B58), one commit:
"kernel, testbench, tests, docs: the oracle decides whether a timer interrupt ended the slice".
Trials before the ruling: .wash/local/B58-trials.md.

## What changed

- kernel/src/sched.rs (sched-trace only; a release build is unchanged): `slice_over` records its
  inputs (`trace::slice_decided(end, now)`: record `=`, the slice's end in µs in the id, now in µs
  in the pass field) inside an interrupt from user mode; the ring's `slice_over` comes from that
  decision, not sampled at `timer_entry`. `O` therefore says 0 only when the decision found the
  slice over.
- tools/testbench/src/sched_oracle.rs: parses `=`; each timer interval keeps its hart and its
  decision; at `O` the oracle computes over = end <= now and refuses a return that says otherwise
  (hard error), and refuses a decision outside a timer interrupt on its hart, in a device
  interrupt, or a second one. New bound `wake_mid_slice=N` in check_wake_no_preempt_harts. Host
  test the_oracle_decides_whether_a_timer_interrupt_ended_the_slice (4 accepted, 4 refused
  returns, 3 malformed decisions, other hart); mid-slice bound met/missed added to the harts wake
  test; five fixtures write the kernel's `=` before `O 0` (two record ranges moved: 25..=34 ->
  25..=35, 27..=222 -> 27..=261). Module doc names the decision.
- tests/programs/src/sched.rs: WakeDelay's k-th nap is base + (k x step) mod 1000 (step param 2;
  0 keeps the one-hart case unchanged).
- tests/programs/src/bin/sched-wake-no-preempt-harts.rs: 130 µs base, step 17, 120 naps, calls 37 µs.
- tests/sched-wake-no-preempt-harts.toml: wake_no_preempt=120 wake_witnesses=20 (kernel-red);
  mid-slice recorded; comment says why rv32 reads 0.
- tests/sched-wake-no-preempt-harts-mttcg.toml (new, quiet in scripts/jobs.mk): no icount,
  wake_witnesses=5 wake_mid_slice=5; five-run ranges and the negative quoted.
- tests/size-budget.toml: kernel 10642 -> 10652 (`Size budget: kernel:` line in the commit).
- docs/kernel/scheduling.md: Preemption points (paragraph; status tested 8 -> 10: the twin and
  the oracle test), sched-trace case list, 'QEMU's turns' residual (the wake case), keep_smp
  sentence. docs/testbench.md: the oracle's O and `=` paragraph, wake_mid_slice, status 26 -> 28.

## Numbers: ten runs a width (five before the commit, five at its head), quoted alike in the tomls, scheduling.md and the commit message

| ten runs | rv64 mid-slice / at a call | rv32 mid-slice / at a call |
| --- | --- | --- |
| sched-wake-no-preempt-harts (icount, gates 20 witnesses) | 22-30 / 51-69 | 0-1 / 40-43 |
| sched-wake-no-preempt-harts-mttcg (quiet, gates 5 mid-slice) | 52-78 / 30-60 | 57-71 / 30-44 |

The two halves: before the commit icount rv64 22-30 / 55-65, rv32 0 / 40-43, mttcg rv64 52-72 /
35-60, rv32 57-71 / 30-44; at cde8c6691 icount rv64 26-29 / 51-69, rv32 0-1 / 41-43, mttcg rv64
67-78 / 30-41, rv32 63-70 / 32-40. The same code (the amend after cde8c6691 changed numbers in
comments, docs and the message only). Negative (temporary toml, wake-preempts kernel, mttcg, deleted):
rv64 'took hart 0 from budget 34', rv32 'took hart 0 from budget 37': FAIL both, as required.
sched-wake-no-preempt (one hart) five runs a width: PASS, 20 proofs each.

## Gates

- On 49a188d74 (same code; the amend changed the witness bound, a description, docs wording and
  the ceiling): `make prebuilt` rc 0 (rv64 290, rv32 276 cases); `make -k set CASES="<31 sched-*>
  irq-boot-hart-only kernel-containment docs formatting size-budget unsafe-budget"`: 70 targets,
  69 rc 0, size-budget rc 1 (10,652 > 10,642), fixed in the amend.
- On cde8c6691: prebuilt rc 0; size-budget, docs, formatting rc 0; the two wake cases x5 a width
  above, all PASS; `--smp 2` sweep, one case per invocation, both widths: every sched-* but the
  mttcg twins (28) and kernel-containment PASS (keep_smp cases at their own counts;
  kernel-containment 388.5 s rv64, 521.9 s rv32).
- `cargo test -p testbench`: 188 passed.
- Not run: the whole bench (the train's).

## Summaries checked

docs/kernel/scheduling.md and docs/testbench.md updated (above). docs/SECURITY.md R12 row: lists
no wake-no-preempt case; Preemption points carries them; no change. README.md, GETTING-STARTED.md:
no claim about the trace or these cases; no change. docs/kernel/timer.md: says nothing of the
trace's slice decision; no change.
