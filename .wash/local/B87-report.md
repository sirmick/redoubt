# B87 report: the two-hart "device interrupt returned without its claim" rejections

Branch `wp-B87`, worktree `/home/mcloonan/redoubt/.worktrees/B87`, on main 75a2c8b62.
Head: 242692350 `testbench: a device interrupt a deadline preempts before its claim is billed as
one that claimed nothing`. One commit; `git status --short | wc -l` = 0. Kernel untouched.

## Reproduction (kept, under `$REDOUBT_TMP/B87`, i.e. `/home/mcloonan/redoubt/.tmp/B87`)

`TESTBENCH_QEMU_SEED=<s> cargo testbench steward-latency --arch rv64 --smp 2 --keep-failed <dir>`
under `q run --cores 4 --ports 1`:

- seed 1, smp 2: accepted this time (the FAIL is the known timer-wake target miss) — the
  rejection is not a pure function of the seed; host-side timing reaches the guest.
- seed 2, smp 2: rejected, `record 2427851` — `seed2-smp2/rv64-steward-latency.smp2.log`.
- seed 4, smp 2: rejected, `record 1869759` — `seed4-smp2/rv64-steward-latency.smp2.log`.
- seed 1, smp 1 (contrast): accepted; 4686 device interrupts, all claimed, none with a
  destruction inside (`seed1-smp1/`).

`scan-x.awk` (scratch) lists every `x`..`O` span without a `c`: seed 2 has 1 of 1876 device
interrupts, seed 4 has 2 of 1972 (records 1869759.. on hart 0 and 2092926's entry on hart 1);
every one has a destruction inside and returns `O 0 2`. The accepted traces (seed 1 at one and
two harts) have zero spans with a destruction inside.

## Cause (the kernel's rule, recorded faithfully; the oracle's pairing was too strict)

Seed 2, hart 0, kernel entry 647064: `Q`/`y` (a lock wait of 55337 audit ticks), `x 254`
(2427851), a reweigh group, `X 389` (2427858) .. `Y 389` (2427893), `E 93 1`, `B 93` charges,
`R 254`, `H 0`, `O 0 2` (2427910). No `c`. In `kernel/src/arch/riscv/irq.rs:236-253`
`expire_at_entry()` runs before the external-interrupt arm; the lease's deadline had passed
while hart 1 held the lock, the expiry destroyed it (`expired.destroyed`) and `preempt()` left
for `kmain` before `intc::pending()` and `trace::claimed()` (irq.rs:328-330). timer.md:191 states
the rule (a deadline is a preemption point; the trap is taken again when the thread next runs);
the PLIC keeps the source pending, so no interrupt is lost or double-handled. The lock is held
across the whole entry (released only at irq.rs:28, the syscall return and kmain's idle), so the
"claim recorded on the other hart's stream" shape is ruled out. One hart: the deadline's own
timer interrupt answers it first, so the coincidence needs the other hart's lock hold.

## Fix (tools/testbench/src/sched_oracle.rs)

`check_external_entry` takes the return's pass: a missing claim is accepted only with a
destruction inside the entry and `O` 2, billed as "claimed nothing" (charges after the expiry's
end are the budget billed last), counted as `external_preempted` in the summary line
`({} claiming nothing, {} a deadline preempted before their claim)`. Either mark absent still
fails, with a message naming the missing preemption. Host tests:
`a_device_interrupt_a_deadline_preempts_before_its_claim_is_billed_as_claiming_nothing`
(positive + 4 negatives) and the ignored `a_kept_log_judged_again`
(`SCHED_ORACLE_LOG`, `SCHED_ORACLE_ARGS`), which re-judges a kept console log: both kept rejected
traces now pass the device-interrupt check (seed 2: 1 preempted, seed 4: 2) and fail only on the
timer-wake target the two-hart rows already miss.

## Pages

- docs/kernel/scheduling.md "Charging": the rule, and the status list (tested 20 → 21).
- docs/testbench.md: the oracle paragraph; `--keep-failed` paragraph names the re-judge test.
- Summaries checked, no change needed: README.md, GETTING-STARTED.md, docs/README.md,
  tools/testbench/README.md, docs/plan/m3-agents.md (none names the oracle's device-interrupt
  rule or the two-hart steward rows); devices.md R5 (its oracle claim about claimed interrupts
  charging `system` still holds).

## Gates

- host: `cargo test -p testbench sched_oracle`: 65 passed, 1 ignored.
- jobs.mk set: docs PASS (after the status count), formatting PASS (nightly fmt applied to
  sched_oracle.rs), size-budget PASS, unsafe-budget PASS; sched-latency PASS rv64/rv32 smp 1 and
  2; sched-latency-tcg PASS rv64/rv32 smp 2; kernel-containment PASS rv64/rv32 smp 1 (its own
  count) and at `--smp 2`: rv64 PASS, rv32 see below.
- steward-latency at two harts, seeds 1-4, both widths (`gate-<arch>-seed<s>.out`): see below.

- kernel-containment `--smp 2`: rv64 PASS (503 s), rv32 PASS (718 s).
- steward-latency at two harts (every run FAILs on the timer-wake target the page already
  records as missed; "accepted" = the oracle's summary printed, no record rejected):
  - rv64 seed 1: accepted, 2062 device interrupts (686 claiming nothing, 2 a deadline preempted
    before their claim); seed 2: accepted (662, 1); seed 3: accepted (177, 0); seed 4: accepted
    (324, 1).
  - rv32 seed 1: accepted (198 device interrupts, 62 claiming nothing, 0 preempted).
  - rv32 seed 2: did not reach the oracle: alice's session hung in round 2 (`step 9: timed out
    waiting for {:round, 2, :done}`, 5400 s; the ssh log ends at `{:error, :timeout}` after
    `lease alice/agent-33 ended`), the handoff's known rv32 two-round hang, upstream of this
    package; its partial trace has no unclaimed device interrupt.
  - rv32 seeds 3, 4: RV32-34-PENDING

## What I read

The diff in full; `sched_oracle.rs` by its parse, interrupt state machine, external/timer
judges, summary printing and the device-interrupt tests; the kernel's irq.rs trap path, the
trace module and `leave`, cell.rs's lock, time.rs's `Expired`; the pages by the sections named
above. Not the whole of the three committed files.

## Open

- The orchestrator's gate asked for a kernel case if the kernel moved; it did not, so the fix is
  the oracle's with host tests, as the node's second branch allows. No question was opened on
  B87-irq-claim (architect-2) since the kernel's rule stands as timer.md states it.
- `sweep` still cannot keep a failing log; `a_kept_log_judged_again` covers re-judging one once
  kept.
