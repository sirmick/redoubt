# SMP6 report: a freed frame is zeroed outside the lock

smp6-implementer-2, 2026-10-09. Branch wp-SMP6 in .worktrees/SMP6.

- **Base:** main 050357e8a. Main has since moved to 98b866854, with nothing in the kernel.
- **Head:** dace73d35 (kernel, testbench) on 6c3e096b9 (model).
- **Where the gates ran:** on 01f5cf410, whose tree is identical to dace73d35. dace73d35 changes only the commit message's attack-case figures.

## What the branch is now

R81 as built and ruled today, three rulings beyond SMP6-design.md (all on thread SMP6-zeroing-billed-to-nobody, notes in architect-questions.md):

1. **Billing debt is lifted.** A destruction lifts any hart's zeroing still owed by a budget it ends to its own R10 payer. The trace records this as `p`, and the checked kernel asserts at the bill.
2. **(A) No zeroing at an exit.** A hart never zeroes on its way back to user mode. Frames are zeroed on demand, one per allocation under the lock, when the bitmap is empty, taken from any hart's pending list.
3. **A1 + A3 for idle zeroing.** An idle hart zeroes one frame per pass, so an interrupt waits at most one frame (~30 µs), and only while every started hart is idle.
   - I did not build A1 as "a 64-frame batch stopped at an interrupt". That leaves frames held by a hart waiting for the lock while the lock holder's allocation waits for them, a deadlock.
   - Pending lists change only under the lock. The checked kernel counts every frame in flight, on its list, against the ownership table at each destruction's audit.

Also folded in:

- **kernel-red P2-1..P3:**
  - self-lend guard;
  - the empty-bitmap path, now exercised by the attack case;
  - the interrupts-off residuals in budgets.md and timer.md;
  - the model page's note on how `R81InFlightAllocatable` is caught;
  - `install_table(already_zero)`.
- **The R10 regression fix.** `destroy_marked` freed ~4,270 endpoint frames one full retire at a time (+6.8 ms per lease destruction). A destruction's object frames now go in flight as one chain: one owner change and one link write each.
- **`smp-inflight-race` under memory pressure.** The judge holds root's free pages; fillers hold users' and system's. So the race empties the bitmap and reuses frames in flight, any hart's.
- **The zeroing trace record is `0`.** Main took `z` for the lift's floor and no letter is free.

## Gates, on 01f5cf410 (tree = dace73d35), exit codes

| Gate | Result |
| --- | --- |
| docs | rc 0 |
| formatting, size-budget, unsafe-budget | rc 0 |
| model-host-tests, model-mutations (release, through q) | rc 0 |
| build-rv64, build-rv32, prebuilt (263 / 249 cases) | rc 0 |
| host-tests, memory-host-tests | rc 1: one test, `case::tests::the_image_recipe_packs_init_the_servers_and_the_manifest`. It fails identically on main 050357e8a (checked there) and is fixed on main by a7efce041; not SMP6's. |
| own counts: 42 cases (every sched-*, mem*/lend*/map* boot case, smp-inflight-race, kernel-containment, the smoke set), both widths | 90 PASS, rc 0 |
| `--smp 1`, both widths | 82 rc 0 |
| `--smp 2`, both widths | 83 rc 0 |
| lend-untouched-page at 4 harts, both widths | rc 0 |

Failures in the sweeps:

- **smp-inflight-race at `--smp 1`** (declared `smp = [2]`): a filler's 5 s handshake times out at one hart. Outside its declared count; the gate asks for 2 harts, which pass.
- **sched-lock-contention-4-mttcg rv32 in the `--smp 2` sweep:** real time, under sweep load. Rerun alone it passed twice, and it passes at its own count.

The two regressed cases pass again:

- **map-anon-search-bound:** 1 and 2 harts, both widths.
- **sched-cluster-old-control:** both widths (`keep_smp`, so 1 hart).

Size and unsafe ratchets:

| Ceiling | Change |
| --- | --- |
| kernel lines | 10,076 → 10,595 |
| model lines | 11,092 → 11,125 |
| libs/paging lines | 257 → 263 (a rustfmt reflow) |
| Sv39 backend unsafe | 13 → 11 (two `zero_frame` calls gone) |
| core unsafe | reclaim.rs added, no unsafe |

The commit messages carry the Size budget lines.

## The attack case and the recorded negatives

**smp-inflight-race at 2 harts.** No frame taken in flight; the witness saw 0 non-zero words and 0 changed markers.

| | Frames retired | At most in flight | Taken again |
| --- | --- | --- | --- |
| rv64 | 3,316 | 624 | 2,692 |
| rv32 | 3,297 | 627 | 2,670 |

The verdict comes from the system: the witness's own pages, the checked kernel's checks, and the trace order.

**Recorded negatives on dace73d35's tree.** Each fails where intended, on both widths:

| Negative | Fails at |
| --- | --- |
| `inflight-early-commit` | the checked kernel's zero sample at allocation (mem.rs:445) |
| `inflight-zero-unshot` | the trace: "frame 584 pending before PID 337 … was shot down" |
| `zeroing-unlifted`, sched-exit-churn and sched-budget-churn at 2 harts | the R10 bill assertion (sched.rs:545) |

**Positive lift runs** at 2 harts pass, lifting 6–9 entries per run.

**Model mutations.** Run alone on the first base: `R81InFlightAllocatable` is caught in 0.06 s and `R81CommitUnzeroed` in 0.44 s, against a 25 s deadline. model-mutations passes rc 0 on the head.

## Measurements

### Containment gate, hold-trace, 2 harts, rv64: main 050357e8a vs head

Measurement only: both sides ran with a 288 MiB ring and 640 MiB RAM, because the 256 MiB ring overflowed by 0.8%.

| | main | head |
| --- | --- | --- |
| Verdict | PASS | PASS |
| R10 p50 / p99 | 22.9 / 23.6 ms | 24.6 / 25.4 ms |
| Kernel sections, net ticks p50 / p99 | 4,528 / 8,939 | 4,543 / 8,861 |
| budget_destroy longest section | 239,354 | 254,746 |
| Lock waits | 279 / 1000 | 280 / 1000 |
| Nobody | 34 | 34 |
| driver_wake p99 | 4.76 ms | 3.98 ms |
| timer_wake p99 | 6.51 ms | 6.31 ms |
| deadline_notice p99 | 32.3 ms | 33.8 ms |
| Frames zeroed outside the lock | n/a | 52 |

- **No saving is visible in this gate.** Under A3 its harts are never all idle and 576 MiB never runs short, so allocations take never-used frames and zero them under the lock at first take, as main does. The commit's 15 s figure is the motivation, measured before; it is not claimed as a saving here.
- **rv32 hold-trace** overflowed even the bigger ring on both main and head (measurement config only). The gate's own config passes on rv32 at 1 and 2 harts.

### Pending set under load: containment with inflight-trace, 2 harts

| | High-water | Given back during the run | Longest in flight before given back |
| --- | --- | --- | --- |
| rv64 | 79,546 | 1,218 | 113 s |
| rv32 | ~79,544 | 993 | — |

- The high-water frames are about 310 MiB of 576, almost all still in flight at the end.
- R10 fails in this config only: two trace records per freed frame.

### Nobody share: deadline-flood-billed-traced, 2 harts, `keep_smp` lifted

| | main | head |
| --- | --- | --- |
| rv64 | 20 / 1000 | 24 / 1000 |
| rv32 | 54 / 1000 | 64 / 1000 |

Traced in absolute ticks (kernel − audits − charged):

| | main | head | Zeroing outside the lock, head (all billed) |
| --- | --- | --- | --- |
| rv64 | 153,123 | 164,798 (+11.7k) | 6.6k ticks (31 frames) |
| rv32 | 466,434 | 438,005 (lower) | 7.2k ticks (28 frames) |

- The share rose because the runs did less charged work: rv64 charged 7.20 M → 6.70 M ticks; rv32 ran 214 → 134 destructions.
- Zeroing can account for at most 6.6k of rv64's +11.7k, and that zeroing was billed.
- The case keeps 1 hart because its counts do not stand at 2; this comparison is that noisy.

## Pages and summaries checked

**Updated:**

- docs/kernel/memory.md:
  - Frames in flight: the three drains and why A3; one hart at a time; the checked count;
  - R81: rule bullets and status;
  - the state diagram;
  - Why, and the new residual "freed frames are not zeroed beside the work".
- docs/kernel/model.md: 170 variants; the R81 row and how it is caught.
- docs/kernel/budgets.md: R10 zeroing payer, lift and root exception; destruction residual; the stale "departs" passage removed.
- docs/kernel/timer.md: the interrupts-off residual.
- docs/kernel/scheduling.md: billing of zeroing.
- docs/kernel/invariants.md: I9's kept-in list.
- docs/SECURITY.md: the R81 row.
- docs/testbench.md: the `0` and `p` records.
- docs/plan/m2-usable-shell.md: step 5 and the progress line.
- docs/todo/kernel-attack-gaps.md.
- tests/smp-inflight-race.toml: RAM and why.

**Checked, no change:**

- README.md, GETTING-STARTED.md, kernel/README.md: no zeroing or R81 claims.
- model/README.md: points to model.md.

**Finding:** docs/kernel/README.md:292's TCB paragraph gives the kernel's unsafe split as 13/12/19 = 44. Main's ratchet is already 13/10/16, and SMP6 makes the Sv39 backend 11. It was stale before SMP6; it needs a docs pass on the real counts, not a one-number patch.

## Open risks

- **Pending frames are bounded only by RAM.** Containment holds ~79.5k in flight.
- **The on-demand path costs main's price under load.**
- **Parallel zeroing is forgone while any hart works.** This is a residual on the page, to revisit when an idle hart's work can be judged without icount's shared clock.

Next: kernel-red renews on dace73d35; the orchestrator rebases onto main at merge.

## Fix round (kernel-red OK with notes on dace73d35), 2026-10-09

New head **8fb11dc37** (kernel, testbench) on **938a92b17** (model), base main **98b866854**.

The rebase was `git rebase --onto 98b866854 050357e8a wp-SMP6`. Both commits replayed cleanly and keep the two-commit shape. Ceilings are unchanged: kernel 10,595, model 11,125, paging 263.

### Folded in

- **P2-1, docs/kernel/memory.md:**
  - The residual "freed frames are not zeroed beside the work" now states:
    - the containment gate saves nothing (allocations zero never-used frames at first take);
    - ~79,500 frames (~310 MiB) in flight, nearly all held to the run's end, the longest given back after 113 s;
    - a process keeping one hart busy keeps every allocation at main's cost, never worse;
    - freed contents stay in RAM though no call reaches them.
  - The Why entry now says an allocation writes nothing only when idle time has zeroed ahead, and under load zeroes its own frame as before. Its garbled "Measured at two harts" sentence is rewritten.
- **P2-2, docs/plan/m2-usable-shell.md:**
  - Step 5: zeroing only while every started hart is idle. Allocation writes nothing unless the frame was never used since boot. The gate saves none of it.
  - Step 6: frames never used since boot still need zeroing when a magazine is filled.
- **P3, `in_flight_linked`:** it now goes through `set_owner` (owned to owned, no bitmap change), so "every write of the table goes through here" holds.
- **P3, the R10 +1.8 ms and notice +1.5 ms:** measured with phase timestamps through one lease destruction, rv64 at 2 harts, main 050357e8a against the head.

  | | main | head | Delta |
  | --- | --- | --- | --- |
  | Whole destruction | 22.9 ms | 25.7 ms | +2.8 ms |
  | Endpoint frees (~4,270 frames) | 4.86 ms | 7.25 ms | +2.39 ms (≈0.56 µs a frame) |
  | Start to the first thread's ending (the first victim's frames going pending) | 0.14 ms | 0.44 ms | +0.30 ms |
  | lift_dying | | | +0.04 ms |
  | Every other step | | | within ±0.05 ms |

  The cause is the red's suspicion: each freed frame gets a link write into the frame plus the in-flight owner change, where main flipped one bitmap bit. The notice carries the destruction. Both stay within their bounds (30 ms, 40 ms).
- **P3, README TCB unsafe split:** left for a follow-up node. It was stale before SMP6 (13/12/19 against main's 13/10/16; SMP6 makes Sv39 11), and the fix is more than one line.

### Gates on 8fb11dc37, exit codes

| Gate | Result |
| --- | --- |
| docs | rc 0 |
| formatting, size-budget, unsafe-budget | rc 0 |
| host-tests, memory-host-tests | rc 0 (the image-recipe failure is gone on this base) |
| model-host-tests, model-mutations | rc 0 |
| build-rv64, build-rv32, prebuilt | rc 0 |
| smoke set, both widths: userland-boot, init-boot, bench-net-peer, ipc-outcomes, sum-clear, lend-untouched-page (also at 4 harts) | PASS |
| smp-inflight-race at 2 harts, both widths | PASS |
| kernel-containment at 2 harts | PASS, rv64 532.5 s, rv32 698.3 s |

### Scratch worktrees

All removed. Their logs are kept under `.tmp/SMP6/logs`, outside any worktree.

Every snapshot was a commit plus toml edits, so a rerun rebuilds it from the commit and the edits:

| Purpose | Edit |
| --- | --- |
| hold-trace | add `"hold-trace"` to kernel-containment's `kernel_features` |
| in-flight | add `"inflight-trace"` to kernel-containment's `kernel_features` |
| the 2-hart rv64 measurement | trace ring `PAGES` 65536 → 73728 and `memory_mib` 640 |
| negatives | `inflight-early-commit` / `inflight-zero-unshot` on smp-inflight-race; `zeroing-unlifted` on sched-exit-churn and sched-budget-churn |
| deadline-flood at 2 harts | drop `keep_smp` |

None was kept.

## B56: the idle regression (smp6-implementer-3, 2026-10-09)

New head **833331cdb** (kernel, testbench) on **938a92b17** (model), base main **98b866854**. The model commit is unchanged. The fix is folded into the kernel commit by amend; the message now says what idle drain does and what it costs.

### Measurement on 8fb11dc37 (before the fix)

launch-idle at 4 harts, alone on the quiet cores:

| | rv64 | rv32 |
| --- | --- | --- |
| Verdict | FAIL, 166.6/s busiest hart, 0.012 host cores | FAIL, 88.3/s, 0.010 host cores |
| m_software | 165.5 / 163.7 / 89.3 / 137.6 per hart | 86.0 / 87.3 / 86.7 / 24.5 |
| user_ecall | 3.9/s (IDLE1's) | 4.8/s |

Where they come from (symbolised against the run's own kernel, nm/objdump):

- m_software epc `0x…d02586` is the instruction after `wfi` in `TicketLock::acquire_ticket`: a hart waiting for the kernel lock.
- supervisor_ecall `0x…d024ce` is in `hart::wake_halted`: the release's IPI to that waiter.
- They pair 1:1 on every hart: lock hand-offs, IDLE1's cause, but ~50x as many.
- They are a burst, not a rate. Using s_timer interrupts (one per hart per ~1 s, in clusters of four) as a clock:
  - rv64: hart 0 took 9,802 of its 9,920 IPIs before its first s_timer, and ~32,900 of all 33,160 fell between the timer clusters ~2 s and ~3 s into the window.
  - rv32: ~16,640 of 17,080 fell in one interval.
  - After the burst: 0-9 IPIs per hart per timer period, ~2-3/s, IDLE1's level.
- The burst is R81's idle drain. The login freed ~33k frames (~130 MiB). They wait pending until every hart idles, which is when the window opens. Then all four harts drain their own lists at once. A1 makes each frame a lock release plus a lock take. With four drainers, nearly every take found the lock held, so each take halted and each release sent an IPI.

Ruled out:

- A hart with an empty list halts as before: it is never in this loop.
- No SIE-window timer storm: s_timer stays ~0.8/s per hart.
- The WANT/done wake never fires at rest.
- The user ecalls are IDLE1's.

### Fix

`reclaim::DRAINER`: one hart drains at a time, under the lock (`reclaim::take_one(all_idle)`).

- A hart that idles while another drains halts, exactly as before.
- The drainer finds the lock free on every take, so no halt and no IPI.
- When its list is empty, the drainer clears the claim and wakes the next idle hart with frames pending: one `wake_halted`, so one IPI per hart per drain.
- When a hart wakes to work, the drainer drops its claim at its next idle pass (`!all_idle`). The drain restarts when the system is next all idle.

Unchanged: A1 (one frame a pass, back through kmain's SIE window), A3 (only while every started hart is idle), the on-demand path and billing.

### Gates

Formatting, size-budget, unsafe-budget, model-mutations, the smoke set and smp-inflight-race also ran on 833331cdb. launch-idle ran on f160bc4a8, whose code is identical; only docs/testbench.md and the message changed since.

| Gate | Command | Result |
| --- | --- | --- |
| launch-idle rv64 | `q run --cores 4 --quiet -- target/prebuilt/testbench --prebuilt target/prebuilt --exact --arch rv64 launch-idle`, alone | rc 0, PASS |
| launch-idle rv32 | same, `--arch rv32`, alone, after rv64 | rc 0, PASS |
| formatting, size-budget, unsafe-budget, model-mutations | `make -f scripts/jobs.mk set CASES=…` | rc 0 each |
| docs | `make -f scripts/jobs.mk set CASES=docs` | rc 0 |
| host-tests, memory-host-tests, model-host-tests | `make -f scripts/jobs.mk set CASES=…` | rc 0 each |
| Smoke set, both widths: userland-boot, init-boot, bench-net-peer, ipc-outcomes, sum-clear, lend-untouched-page | `make -f scripts/jobs.mk set` | rc 0 each |
| smp-inflight-race, both widths | its declared 2 harts | rc 0 each |
| kernel-containment rv64 | `q run --cores 2 -- … --smp 2 kernel-containment` | rc 0, PASS 635.5 s |
| kernel-containment rv32 | same | rc 0, PASS 899.3 s |

launch-idle per-hart detail on f160bc4a8:

| | rv64 | rv32 | Ceiling |
| --- | --- | --- | --- |
| Busiest hart | 3.4/s (m_software 2.4, s_timer 0.8, s_software 0.1) | 2.9/s (m_software 2.0, s_timer 0.8) | 9 |
| Host cores | 0.006 | 0.007 | 0.01 |
| user_ecall | 5.3/s | 3.7/s | |
| supervisor_ecall | 15.4/s | 13.7/s | |

An earlier window on the pre-compaction fix (same logic): rv64 3.1/s at 0.005 host cores, rv32 2.7/s at 0.007.

kernel-containment at 2 harts, notice and wakes (the case asserts R10 within its bound and passed):

| | deadline_notice net p50 / p99 | driver_wake p99 | timer_wake p99 |
| --- | --- | --- | --- |
| rv64 | 28.0 / 35.7 ms (≤ 40) | 4.1 ms | 6.1 ms |
| rv32 | 28.9 / 37.1 ms | 5.6 ms | 5.8 ms |

Size: the kernel ceiling goes 10595 → 10608 (13 lines, `DRAINER` and the hand-on). The commit's Size budget line is updated. Unsafe is unchanged.

### Pages and summaries checked

- **docs/kernel/memory.md, "Frames in flight", Idle bullet:** now says one hart drains at a time, why (the ~33k interrupts), that the drainer wakes the next, and that a hart with nothing pending halts as before, so the system at rest takes IDLE1's interrupts. The R81 bullet's stale "the batch it cut off its own" is now "the frame it took off its own".
- **docs/testbench.md, "The machine at rest":** restates IDLE1's numbers against the new ones (3.1/2.8 at 0.002 cores before R81; 3.1-3.4 / 2.7-2.9 at 0.005-0.007 with it), and that the window opens on the drain.
- **Checked, no change:**
  - docs/plan/m2-usable-shell.md step 5 ("only while every started hart is idle"): still true.
  - docs/kernel/timer.md and scheduling.md: nothing about idle drain order.
  - README.md, GETTING-STARTED.md: no idle-zeroing claims.
  - The memory.md residual "Freed frames are not zeroed beside the work": still true.

### Open risks

- **Host cores at rest:** the drain's zeroing now falls inside launch-idle's window (0.005-0.007 against 0.002 before R81; ceiling 0.01). A login that frees much more could approach the ceiling. It is CPU spent zeroing, not wake-ups.
- **The drain is serial:** ~33k frames by one hart, about a second or two under MTTCG. Pending frames on other harts wait their turn; an allocation still takes them on demand.

### B56 follow-up: the drain's length and cost, and per-cause tables (orchestrator's ask)

**The drain, measured.** A temporary print in `take_one` (not committed; the patch is .tmp/SMP6/b56/measure-drain.patch) recorded the frames and kernel time of each drainer's run. It ran on 833331cdb's code in one launch-idle run per width, 4 harts, alone on the quiet cores. Both runs passed.

| | rv64 | rv32 |
| --- | --- | --- |
| Frames drained after the login | 34,691 (hart 0 12,486, hart 1 20,387, hart 2 783, hart 3 1,035) | 35,440 (13,726 / 18,369 / 2,678 / 667) |
| Drain, end to end | 4.431 s → 4.682 s guest time: **0.25 s** | 4.366 s → 4.685 s: **0.32 s** |
| Per frame | ~7.2 µs | ~9.0 µs |
| When | Begins right after `log int` (console log line 69), so inside the window | Same |

The four runs are back to back. Each finisher hands to the next hart with frames pending, as designed.

**Through the rest of the window:** small runs of 1–3 frames every few seconds (something at rest frees a page or two). Then ~3,800 frames right after `log none`, freed by the session's next step.

**Host cost:** host cores read 0.006 (rv64) and 0.007 (rv32) against IDLE1's 0.002. The difference is 0.004–0.005 cores, or 0.24–0.30 s of host CPU over 60 s. That matches one host thread zeroing for 0.25–0.32 s. The drain fits under the 0.01 ceiling as is, so the case is unchanged: no settle wait, no ceiling change. The work is visible and counted, not hidden.

**Per cause, interrupts/s per hart, launch-idle at 4 harts, alone (60 s windows):**

rv64:

| Hart | 8fb11dc37 (before) | 833331cdb code (after) |
| --- | --- | --- |
| 0 | 166.6: m_software 165.5, s_software 0.1, s_timer 0.9 | 3.4: m_software 2.4, s_software 0.1, s_timer 0.8 |
| 1 | 164.7: m_software 163.7, s_software 0.2, s_timer 0.8 | 3.4: m_software 2.5, s_software 0.1, s_timer 0.8 |
| 2 | 90.6: m_software 89.3, s_software 0.4, s_timer 0.8 | 2.7: m_software 1.9, s_timer 0.8 |
| 3 | 139.1: m_software 137.6, s_software 0.6, s_timer 0.8 | 1.9: m_software 1.2, s_timer 0.7 |
| Host cores | 0.012 | 0.006 |
| supervisor_ecall / user_ecall | 569.3 / 3.9 | 15.4 / 5.3 |

rv32:

| Hart | 8fb11dc37 (before) | 833331cdb code (after) |
| --- | --- | --- |
| 0 | 87.0: m_software 86.0, s_software 0.1, s_timer 0.8 | 2.9: m_software 2.0, s_software 0.1, s_timer 0.8 |
| 1 | 88.3: m_software 87.3, s_software 0.2, s_timer 0.8 | 2.9: m_software 2.1, s_timer 0.7 |
| 2 | 87.9: m_software 86.7, s_software 0.5, s_timer 0.7 | 2.3: m_software 1.6, s_timer 0.7 |
| 3 | 25.5: m_software 24.5, s_software 0.2, s_timer 0.8 | 2.4: m_software 1.6, s_timer 0.8 |
| Host cores | 0.010 | 0.007 |
| supervisor_ecall / user_ecall | 293.4 / 4.8 | 13.7 / 3.7 |

s_external is under 0.05/s on every hart in every run.

IDLE1, before SMP6 (0e8b68421), busiest hart: rv64 3.1 (m_software 2.1, s_timer 0.9, s_software 0.1), rv32 2.8 (m_software 1.9, s_timer 0.8, s_software 0.1), 0.002 host cores.

The branch is unchanged at 833331cdb after this measurement; the worktree is clean.

### B56 round 2: kernel-red's checklist for the one-drainer fix → head 4c1dd6a91

New head **4c1dd6a91** (kernel, testbench; amended) on **938a92b17** (model, unchanged), base main **98b866854**.

What changed since 833331cdb:

- `take_one` returns the hart to wake, and `arch::idle` wakes it after the release.
- A hart that idles last with no frames of its own also wakes a hart that has frames pending.
- The wording in docs and comments follows.

The checklist, item by item:

1. **The drainer role, and every way a drainer stops.** The role is `reclaim::DRAINER`, read and written only under the lock in `take_one`.
   - **List empty:** the drainer clears the role and wakes the next hart with frames pending.
   - **Woken with work mid-drain:** the drainer leaves `idle` for `kmain` and keeps the role while it works. No one else can drain meanwhile, since not every hart is idle. At its next idle pass:
     - if every hart is idle, it goes on with its own list;
     - if its list was emptied on demand, it clears the role and hands on;
     - if another hart is busy, it clears the role.
   - **A3 fails because another hart woke:** the same as above, at the drainer's next pass.
   - **No stranded role:** the role is set only by a hart that just took a frame and has more pending. It stays set only while that hart is working or coming back for its next frame.
   - **Stranded frames, closed in this round:** a hart that halted with frames pending while another worked was previously left until its own next interrupt. Now the hart that idles last with no frames of its own wakes it. Wakes happen only when every hart is idle and no hart is draining, so they cannot loop.
2. **The wake is not lost.** The role is set and the target chosen under the lock; the IPI is sent after the release (`arch::idle`: release, then `hart::wake_halted(wake)`), so the woken hart finds the lock free.
   - Every hart is idle, so the target is somewhere between `set_idle(true)` and `set_idle(false)` in `idle`.
   - **If it has not halted yet:** `halt()` skips the `wfi` when a reschedule interrupt is already pending (`sip & sie`, mask 0, with SSIE enabled on every hart).
   - **If it is waiting for the lock:** the wait ends early, and the hart re-checks at its next idle pass.
   - **Either way:** it reaches `take_one` again under the lock and claims the role.
3. **A1 kept, and the count asked for.** Still one frame a pass, back through kmain's SIE window between frames.
   - The rv64 window on the new code drained ~35k frames. Over the whole window, `acquire_ticket` lock waits totalled **520 across all harts** (IDLE1 before SMP6: 559).
   - So the drain added none measurable: under 1.5% even if every one were the drain's.
   - The matching `wake_halted` ecalls: 558. 61 m_software hit the `wfi` in `idle`; IDLE1 had 59 there.
4. **Whose lists are drained.** A drainer drains only its own list. The wake goes to the next hart, which drains its own. Billing is unchanged: idle zeroing bills the hart's own payers, and only `pop_pending` (on demand) takes another hart's frames, with its existing by-count decrement.
5. **launch-idle.** The case is unchanged: no settle wait, and the ceiling stays. The drain's length and cost were measured separately (0.25 s / 0.32 s; 0.004–0.005 host cores; previous section). testbench.md now states the ~35,000 frames and their 0.25 / 0.32 s, beside IDLE1's numbers.
6. **R81's audit.** `counted()` already counts a drainer's frame: the `zeroing` slot is counted and the done push is counted first. The drainer role does not move frames, so nothing changes there. The cases were rerun (gates below).

Gates on 4c1dd6a91. Every row ran through `make -f scripts/jobs.mk set` unless noted, both widths for boot cases, all rc 0:

| Gate | Result |
| --- | --- |
| launch-idle, 4 harts, alone, quiet cores (`q run --cores 4 --quiet` prebuilt) | rv64 PASS: busiest 4.2/s (m_software 3.1, s_timer 1.0, s_software 0.1), 0.006 cores, user_ecall 5.6/s |
| | rv32 PASS: busiest 2.9/s (m_software 2.0, s_timer 0.8, s_software 0.1), 0.007 cores, user_ecall 4.5/s |
| smoke set: userland-boot, init-boot, bench-net-peer, ipc-outcomes, sum-clear, lend-untouched-page | rc 0 |
| smp-inflight-race at its declared 2 harts | rc 0 |
| map-anon-search-bound, sched-cluster-old-control | rc 0 |
| kernel-containment at `--smp 2`, run directly with `q run --cores 2` | rv64 PASS 614.4 s, notice net p99 35.0 ms; rv32 PASS 894.5 s, notice net p99 36.2 ms (≤ 40) |
| docs, formatting, size-budget, unsafe-budget, host-tests, memory-host-tests, model-host-tests, model-mutations | rc 0 |

- launch-idle ran on f4acd2644. Its code is identical to 4c1dd6a91; only testbench.md's numbers and their wrapping changed after it.
- The kernel ceiling stays at 10608.
- rv64's 4.2 on hart 0 is within IDLE1's own spread (it measured 3.9 in its first window). testbench.md and the commit now state three windows each: 3.1–4.2 (rv64), 2.7–2.9 (rv32), 0.005–0.007 cores.

Summaries updated:

- memory.md, Idle bullet: the hand-on after the release, the wake for a hart left with frames, and how a drainer woken to work gives the drain up.
- testbench.md, "The machine at rest": drain length and window ranges.
- hart.rs: `wake_halted`'s doc names its new caller.

The plan step, README.md and GETTING-STARTED.md were checked; nothing to change.

### Rebase onto main 04b7fa4d5 → head c3eaec9d7

- **Command:** `git rebase --onto 04b7fa4d5 98b866854 wp-SMP6`, starting from 4c1dd6a91 (round 2).
- **New head:** c3eaec9d7 (kernel, testbench) on 2bc216b77 (model). The split stays: two commits, no fix-ups.

**Conflict:** one, in docs/plan/m2-usable-shell.md's SMP paragraph.
- Main (BEAM18) added the sentence about a session's VM running two schedulers; SMP6 added the in-flight sentence after `smp-fence`.
- Resolution keeps both, BEAM18's first, then SMP6's sentence, rewrapped.

**docs/kernel/README.md:** auto-merged, since the old branch did not touch it.
- The kernel commit now sets the C13 cells, matching tests/unsafe-budget.toml (11 + 10 + 16):
  - kernel row: 39 → 37;
  - "kernel: Sv39, SBI and PLIC backends" row: 13 → 11.
- Prose is unchanged: K32's Sv39 description already has no frame zeroing.
- tests/size-budget.toml (kernel 10608) and unsafe-budget.toml auto-merged; main's kernel ceiling was still 10076.

**After the rebase**, one text amend (message and testbench.md only, no code). A fourth launch-idle window on the new base read rv32 3.7, above the 2.7–2.9 the page stated. testbench.md and the message now state four windows each: rv64 3.1–4.2, rv32 2.7–3.7, 0.005–0.007 cores. The message's one overlong line is rewrapped.

**Range-diff** `98b866854..833331cdb` vs `04b7fa4d5..c3eaec9d7`:

1. model: `=` (938a92b17 = 2bc216b77).
2. kernel: `!`. Every hunk explained:

| Hunk | What changed | Why |
| --- | --- | --- |
| Commit message | the hand-on sentence ("once it gives the lock up, and so does a hart that idles last with none of its own…"); the launch-idle figures, now four windows each; one line rewrapped | round 2, plus the fourth window |
| docs/kernel/README.md (new in this commit) | the C13 cells 39→37 and 13→11 | the rebase, per K32's C13 |
| docs/kernel/memory.md, Idle bullet | the hand-on after the release, the wake for a stranded hart, a drainer woken to work | round 2 |
| docs/plan/m2-usable-shell.md | the same SMP6 sentence, now after main's BEAM18 sentence | the rebase conflict |
| docs/testbench.md | drain length (~35,000 frames in 0.25 s / 0.32 s) and the window ranges; the paragraph's last sentence rewrapped | round 2 and the fourth window |
| kernel/src/arch/riscv/hart.rs | `wake_halted`'s doc: "an idle hart" instead of "a hart ending its idle drain" | round 2 |
| kernel/src/arch/riscv/mod.rs | `take_one` returns `wake`; `hart::wake_halted(wake)` after the release | round 2 |
| kernel/src/reclaim.rs | `take_one` returns `(frame, more, wake)`. It returns early while more are pending; otherwise it clears the role if held and returns the next hart with frames pending when `free` (every hart idle, no other drainer). Before, it sent the IPI under the lock and only from a finishing drainer. | round 2 |

Nothing else differs. Every other file's hunks are identical up to context lines.

**Gates on the new base.**
- **On 4beab7784**, whose code is identical to c3eaec9d7: the boot cases, model and host tests, and the budgets.
- **On c3eaec9d7 itself:** docs and formatting, rerun after the text amend.
- **Commands:** everything through `make -f scripts/jobs.mk` except launch-idle.
- **Results**, all rc 0:

| Gate | Result |
| --- | --- |
| prebuilt | 280 / 266 cases built |
| launch-idle rv64, 4 harts, alone, quiet cores, `q run --cores 4 --quiet` prebuilt | PASS: busiest hart 3.4/s (m_software 2.4, s_timer 0.9, s_software 0.1), 0.006 cores |
| launch-idle rv32, same | PASS: busiest hart 3.7/s (m_software 2.6, s_software 0.3, s_timer 0.8), 0.007 cores |
| smoke set, both widths: userland-boot, init-boot, bench-net-peer, ipc-outcomes, sum-clear, lend-untouched-page | rc 0 |
| smp-inflight-race at 2 harts, both widths | rc 0 |
| docs (C13 with 37 / 11) | rc 0 |
| size-budget, unsafe-budget, formatting | rc 0 |
| model-host-tests | rc 0 |
| model-mutations | 170 variants: main's 168 plus R81's 2; CTX3 is not on main |
| host-tests, memory-host-tests | rc 0 |

kernel-containment, map-anon-search-bound and sched-cluster-old-control were not rerun on this base. They passed on 4c1dd6a91, whose kernel code is identical; the rebase brought in K32 and BEAM18.

### kernel-red P2-1 (stranding path), on c3eaec9d7

**Already built, in round 2 (4c1dd6a91, carried through the rebase), so no new amend.** In `reclaim::take_one`:

- `free = all_idle && (drainer == 0 || drainer == me + 1)`.
- When no frame is taken with more left, the hart clears its claim if it held one. If `free`, it returns the first other hart with frames pending as `wake`, and `arch::idle` sends `hart::wake_halted(wake)` after the release.

kernel-red's path, step by step:

1. Drainer A sees `all_idle` false, clears its claim, wakes nobody and halts with frames.
2. B finishes its work and idles. Every hart is idle, DRAINER is 0 and B's list is empty, so `free` is true and `frame` is 0.
3. B's `take_one` returns A as `wake`, and B wakes A after the release.
4. A comes back to `take_one` with every hart idle and claims the drain.

This is kernel-red's prescription: the lock is held, this hart has nothing pending and DRAINER is 0, so it wakes one hart with frames, one IPI.

- The code comment says this ("…or by one with none of its own, for a hart that halted with frames while another worked").
- memory.md's Idle bullet says it ("…and so does a hart that idles last with none of its own, for a hart that halted with frames while another worked. A drainer woken to work drains no more until every hart is idle again, and gives the drain up at its next idle pass if one is not").
- A1, A3, the on-demand path and billing are untouched.

**Test coverage:** not expressible in the model or smp-inflight-race as they stand.

- The model has no harts or idle passes: a free puts the frame in flight, and "time passing" zeroes it (model.md).
- smp-inflight-race keeps its harts busy throughout, and its verdicts come from the witness, the checked kernel and the trace order, none of which sees an idle pass.
- The kernel's trace prints only at a reset, so no oracle sees what is pending at rest.

Covering it would need a new trace record, or a checked-build assertion: "at an idle pass with every hart idle and no drainer, no other hart's list is pending unless a wake was just sent". That is a new check, not added here. The measured evidence is indirect: launch-idle's per-cause numbers at rest match IDLE1's, with no stray-interrupt drain tail.

**Gates on c3eaec9d7** (prebuilt rebuilt for it). Run on this head now:

| Gate | Result |
| --- | --- |
| kernel-containment `--smp 2` rv64 | rc 0, PASS 556.1 s, deadline_notice net p99 35.0 ms (≤ 40) |
| kernel-containment `--smp 2` rv32 | rc 0, PASS 1032.2 s, deadline_notice net p99 36.7 ms |
| map-anon-search-bound, sched-cluster-old-control, both widths | rc 0 |

The rest of the list ran earlier, all rc 0:

- **On 4beab7784**, whose code is identical:
  - launch-idle, both widths, alone: rv64 3.4/s at 0.006 cores; rv32 3.7/s at 0.007;
  - the smoke set, both widths;
  - smp-inflight-race at 2 harts, both widths;
  - size-budget, unsafe-budget;
  - model-host-tests, model-mutations (170 variants), host-tests, memory-host-tests.
- **On c3eaec9d7 itself:** docs (C13 with 37 / 11) and formatting.

The range-diff is as in the section above.
