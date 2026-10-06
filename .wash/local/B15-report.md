# B15 report (b15-implementer), round 2: the gate passes on both widths

## Merge head 0a062f7c1 (on main 992d447ce), red's two notes folded

- kernel-containment rv64 via jobs.mk: PASS rc 0, 724.8 s, 3,905,919 records (dropped 0);
  charged share 833, expected 833, target 783..883 met. Stdout:
  evidence-B15/large/bench-stdout-rv64-0a062f7c1.log.
- docs: PASS rc 0. size-budget: PASS rc 0 (kernel 9190 of 9190).
- testbench host tests: 136 pass (on 8bb4654c2; the rebase onto 992d447ce touched none of B15's
  files).
- rv32 kernel-containment last ran on the same code before the rebases: PASS, 4,610,867 records,
  dropped 0.


## Final head 0a062f7c1: every gate rerun on it

| Check | Result |
| --- | --- |
| jobs.mk `rv64/kernel-containment` | PASS rc 0, 495.5 s; 3,907,238 records, dropped 0; charged share 833, expected 833 |
| jobs.mk `rv32/kernel-containment` | PASS rc 0, 548.4 s; 4,610,867 records, dropped 0; charged share 833, expected 833 |
| rv64 `bench-attack-forgery`, `bench-qemu-early-exit` | rc 0 |
| rv64 + rv32 `bench-cbo-self-unrefused` | rc 0 |
| `cargo testbench --arch rv64` docs, formatting, no-cruft, size-budget | all PASS, rc 0 |
| Kernel `cargo check --profile checked` (qemu-virt, with/without `sched-trace-large`, rv64 + rv32) | rc 0 |

`dropped 0` is an expect line and the oracle refuses any drop, so a PASS is a 0. The consoles were
pruned by later runs; the record counts are the oracle's.

Pages:
- `docs/kernel/README.md`, "The run": the 192 MiB ring, the feature, 512 MiB of RAM, and why.
- `docs/testbench.md`, "Checked builds": the feature beside `sched-trace`. That page has no
  separate kernel-containment paragraph.


Branch `wp-B15`, worktree /home/mcloonan/redoubt/.worktrees/B15, base main 051a2f86c. Three
commits, never pushed:

1. `81df53d54 kernel: the containment gate's trace ring is 192 MiB`
2. `873a35fd4 testbench: the containment gate judges the bystander on the kernel's charges`
3. `0a062f7c1 testbench: a wait cut short reports the failure the guest printed`

## kernel-containment at the tip: PASS on both widths

| | rv64 | rv32 |
| --- | --- | --- |
| Result | PASS, 511.6 s, rc 0 | PASS, 550.5 s, rc 0 |
| `SCHED-TRACE-END` | 3905919 dropped 0 | 4606233 dropped 0 |
| Ring fill (of 6,291,456 records) | 62.1 % | 73.2 % |
| Charged share | 833 | 833 |
| Expected | 833 | 833 |
| Target | 783..883, met | 783..883, met |
| Window (µs) | [38321403, 48321403] | [47274802, 57274802] |
| Deadline notice net p99 | 28411 µs | 29213 µs |
| R10 p99 | 23410 µs | 24125 µs |
| Count (gross useful work) | 708 | 690 |

- Command: `make -f .wash/local/jobs.mk -C <wt> rv64/kernel-containment rv32/kernel-containment`.
- Competitors by trace weight, both widths: {44 (bystander): 100, 58: 9, 60: 1, 4159: 9,
  4161: 1}. Those are both slots' leases (free weight 9) and their sub-agents (1).
- Charged outside the marks: {1, 41}, the program and the steward.
- The counts (708, 690) come from the previous runs with the same window code. This run's lines
  are in the consoles.
- Consoles: /home/mcloonan/redoubt/.wash/local/evidence-B15/large/.

## 1. Trace volume

- Ruling step (1) is dropped, on my measurement and the Architect's answer. The checked build's
  marks audit runs after every reconcile, so every slice has a U/V pair, and that is real time
  the latency targets subtract.
- The ring is 192 MiB (`trace::PAGES` 49152) under a new feature, `sched-trace-large`, which
  implies `sched-trace` (`kernel/Cargo.toml`, `kernel/src/sched.rs`, cfg on the constant only).
- `tests/kernel-containment.toml` builds it, at `memory_mib = 512`. That is the only case that
  moved; every other trace case keeps 64 MiB at its RAM.
- Records per slice (per timer interrupt), before and after (unchanged by design):

  | | Total | P | U | V | B | K | R | I | O |
  | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
  | rv64 | 10.90 | 3.68 | 1.06 | 1.06 | 1.04 | 1.01 | 0.99 | 1.00 | 1.00 |
  | rv32 | 10.93 | 3.71 | 1.06 | 1.06 | 1.04 | 1.01 | 0.99 | 1.00 | 1.00 |

  P is a third of all records; it comes from the scheduler's billing and is not this package's.
- Main's run (64 MiB) wrote 4.25M records and dropped half.
- `docs/testbench.md`, "Checked builds", now names the feature beside `sched-trace`, with why
  only the gate needs it.

## 2. The gate's clause and window

- **Marks:** the program marks the bystander (empty child of weight 2) and sessions (weight 3).
- **Window:** the steward stand-in opens it once round 0's two leases and their sub-agents run,
  right after D is armed. It is 10 s, closing before D's deadline less a margin; fails bit 64
  and a new check line cover it. The steward sends it to the bystander's counting thread (new
  endpoint, slots S9/B6) and reports it (`CT_STEADY`); the program prints `CHARGED-SHARE
  bystander <start> <end> 50 2 3`.
- **Oracle:**
  - charged runtime is pass rises times the trace weight;
  - the expected share is the bystander's weight over the weights of the users budgets charged
    in the window, within 50 either side;
  - a window with a reweigh or lift of a budget under the marks is refused;
  - its line names the competitors, weights and expected share, the wakes in the window and the
    budgets charged outside the marks.
- **Page:** row at `docs/kernel/README.md:177` as ruled, plus "under both slots' leases".
  SECURITY is unchanged.

## 3. The bench's FAIL report

- Unchanged from the first report.
- The gate's forbid pattern was a TOML literal string that never matched; fixed.
- A wait that ends now leads with the first `[name] FAIL` line.
- `bench-cbo-self-unrefused`'s `must_fail` is updated.

## Gates

- **testbench host tests:** 124 pass (`jobserver bounded cargo test --manifest-path
  tools/testbench/Cargo.toml`). New tests: `a_charged_share_is_the_kernels_not_the_count`
  (steady window, count 694 < old floor, charged share 833 = expected; out-of-queue misses;
  expected follows the weights; mark and format errors),
  `a_charged_share_counts_charges_never_the_floors_lift` (floor lift, mid-window reweigh refused,
  forged mark), `a_wait_cut_short_reports_the_failure_the_guest_printed`.
- **rv64:** `bench-attack-forgery` and `bench-qemu-early-exit` PASS. **rv64 + rv32:**
  `bench-cbo-self-unrefused` PASS. These ran before the round-2 changes; the round-2 changes
  don't touch their code path.
- **Gate cases, rv64:** `docs`, `formatting`, `no-cruft`, `unsafe-budget` and `size-budget` all
  PASS. `size-budget` first failed: the kernel was at 9193 lines against a ceiling of 9190.
  `PAGES` became a single `cfg!` const and is now at 9190 of 9190 lines, with the same values;
  folded into commit 1.
- **Kernel `cargo check --profile checked`:** rv64 and rv32, each with `qemu-virt` plus
  `sched-trace-large`, plus `sched-trace`, and with neither, all rc 0. The gate runs above used
  the equivalent cfg-attribute form (identical constants).
- **Not run:** the whole bench; other trace cases (unchanged: the feature is not theirs and the
  64 MiB build is the same).

## Summaries checked

- `README.md`, `GETTING-STARTED.md`, `docs/plan/m1-separation.md` (lines 74 and 133): no claim
  about how the gate judges, or about the ring.
- `docs/kernel/README.md`: the Containment section is unchanged; its Status, "tested:
  bench:kernel-containment", is now true again.
- `docs/kernel/scheduling.md` lines 218-223: unchanged.
- `docs/SECURITY.md` R12: unchanged.

## Residuals

- Charges a budget gets while out of the queue are not counted; wakes are reported (1 in the
  window).
- The whole relies on every lease being destroyed while traced, so its lift places it. The
  budgets outside the marks are listed.
- rv32 fills 73 % of the ring.
