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
