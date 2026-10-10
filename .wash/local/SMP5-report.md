# SMP5 report: per-hart frame magazines (R84, Option X)

smp5-implementer, 2026-10-10. Branch wp-SMP5 in .worktrees/SMP5.

- **Base:** 58326f13c (SMP6's merge). This is not on main: main has since been rewritten without
  SMP6 (orchestrator: SMP6 rides a later train). Per your instruction I stay on 58326f13c and
  rebase onto SMP6's fixed head when it lands (design §9 lists the idle-path assumptions).
- **Head:** 1ecb4d3c8 (kernel, testbench, docs) on 783aae576 (model). The gates ran on tree 84a71537e; 1ecb4d3c8 changes only its message (the measured figures).

## What the branch is

As ruled on thread SMP5-magazine-saving (Option X, conditions 1-6):

- **Magazines.** Each hart has a magazine of up to `MAG_MAX = 16` zero frames, owned by
  `MAGAZINE` (Pid 0xfffc) in the ownership table. `MAGAZINE` is no process, and a magazine frame is
  not free in the bitmap. Allocation draws from the magazine first and writes nothing.
  - Both constants carry their reasons beside them (reclaim.rs; condition 6).
- **Refill.** Each draw in a section lets the section take one replacement under the lock, at most
  `REFILL_MAX = 2` a section:
  - the hart's own pending frame first (already shot down);
  - else the lowest free frame: straight into the magazine if it is zero, into the refill if it was
    never used since boot.
- **Zeroing at the return.** The refill is zeroed after `return_registers` releases the lock.
  - This is the one call line, in **arch/riscv/irq.rs**, not syscall.rs: `return_registers` lives in
    irq.rs (condition 2).
  - It runs after `leave()` has closed the kernel's bill and started the caller's user time, so the
    caller pays, on its slice.
  - A section that ends any other way gives its refill back unzeroed at the next entry: a pending
    frame pending again, a never-used frame free and dirty again.
- **Return of magazines (condition 5).** An allocation that finds the bitmap empty takes from any
  hart's magazine, then from any refill not being zeroed, under the lock.
  - `wait_done` also wakes on a zeroed refill.
  - The destruction audit counts the frames `MAGAZINE` owns against the harts' lists. Every
    magazine changes only under the lock, so the partition holds across a steal.
- **Rule id R84** (condition 1). Mutations: `R84MagazineUnzeroed`, `R84MagazineStillFree`.
- **Docs.** memory.md gains "Frame magazines", R84, the state diagram, a residual ("Every allocation
  still holds the kernel lock") and a Why entry. The "never at an exit" bullet is narrowed with the
  sentence the ruling asked for. Also changed: model.md, SECURITY.md, invariants.md I9,
  scheduling.md and budgets.md (who pays a refill), testbench.md (records `1`/`2`/`3`, the
  per-cause line), the M2 step 6 text (the finding first: lock-free allocation waits for the lock's
  split, a later package after step 7) and its progress line, and todo/kernel-attack-gaps.md.

## Gates (exit codes)

| Gate | Result |
| --- | --- |
| docs | rc 0 |
| formatting, size-budget, unsafe-budget | rc 0 (size: kernel 10,595→10,871, model 11,125→11,155, each with its `Size budget:` line; unsafe unchanged, no new unsafe) |
| model-host-tests, model-mutations | rc 0 (172 jobs) |
| build-rv64, build-rv32, prebuilt (rv32 267 cases) | rc 0 |
| smp-magazine-race at 2 harts, both widths | PASS |
| smp-inflight-race at 2 harts, both widths | PASS |
| 44-case kernel set, own counts, both widths | 94 PASS, 0 FAIL (2 width-specific cases empty on the other width) |
| 44-case set, `--smp 1`, both widths | 86 of 88 rc 0; the two failures are smp-inflight-race (declared `smp = [2]`): its filler's 5 s handshake times out at one hart, as SMP6 recorded; smp-magazine-race passes at one hart |
| 44-case set, `--smp 2`, both widths | 88 of 88 rc 0 |
| host-tests, memory-host-tests | rc 0 |

The 44-case set is:
- every `sched-*` boot case and every `mem*`/`lend*`/`map*` boot case;
- the smoke set: userland-boot, init-boot, bench-net-peer, ipc-outcomes, sum-clear,
  lend-untouched-page;
- smp-inflight-race, smp-magazine-race and kernel-containment.

Logs: .tmp/SMP5/sweep-{own,1,2}/ (summary.txt each), the measurement logs .tmp/SMP5/*-smp2.log and
*-if.log; scratch worktrees under .tmp/SMP5/ (before, after, before-if, after-if, neg-*), each a
commit plus the toml/ring edits SMP6 used.

## The attack case

**smp-magazine-race**, 2 harts, both widths, checked trace kernel.
- **Setup.** RAM is held but for a few pages. An attacker's 160-page run is unmapped under its
  writer, 12 rounds.
- **The witness.** Once the fillers hold RAM, the witness (its own budget) runs two threads, one on
  each hart. Each maps 24 single pages one call at a time, checks them zero, marks them, re-reads
  the marks, and unmaps them one call at a time.

| Width | Refilled | From pending | Drawn | Taken for an empty bitmap |
| --- | --- | --- | --- | --- |
| rv64 | 1,245 | 1,157 | 304 | 936 |
| rv32 | 1,169 | 1,053 | 218 | 947 |

- The witness saw 0 non-zero words and 0 changed markers.
- The verdict is the system's (rule F): the witness's own pages; the checked kernel's checks (a
  frame drawn from a magazine is sampled zero, a frame entering one is checked word by word, and
  the I1 count holds); and the trace oracle `smp_magazine`.

Recorded negatives, both widths, on the head's tree:

| Negative | Fails at |
| --- | --- |
| `magazine-unzeroed` | mem.rs:656, "R84: frame … entered a magazine not zero" |
| `magazine-still-free` | mem.rs:524, "I1: refill frame … not in a magazine" |

smp-inflight-race now also forbids `R84:` failures. It passes with magazines live: rv64 2,447 frames
refilled from pending and 2,293 taken for an empty bitmap.

## Measurements: kernel-containment, rv64, 2 harts, hold-trace

Measurement config only, as SMP6: a 288 MiB ring and 640 MiB of RAM. "Before" is 58326f13c (R81
alone). Before ran twice, identical to the tick; after ran twice (the second pair in parentheses).

| | before | after |
| --- | --- | --- |
| endpoint_create sections (73,691), ticks in all | 249.53 M | 229.44 M (229.96 M) |
| endpoint_create, net per section | 3,386 | 3,113 (3,120): −8 %, ≈27 µs |
| map_anon (665), each | 5,708 | 5,262 |
| process_map (565), each | 9,986 | 9,679 |
| Lock waits | 2,634 M ticks | 2,672 M (2,670 M): +1.4 % |
| Lock waits behind other harts' sections | 1,386.8 M | 1,388.8 M |
| Kernel sections held | 722,958 | 739,178 (737,724) |
| Kernel − audits | 2,549.7 M | 2,579.0 M |
| Charged | 2,460.7 M | 2,489.2 M (the refill's zeroing, now callers' own time) |
| R10 p99 (bound 30 ms) | 26.43 ms | 26.36 ms |
| deadline_notice p99 (bound 40 ms) | 34.93 ms | 35.14 ms |
| lease end p99 (bound 125 ms) | 30.46 ms | 29.93 ms |
| driver_wake p50 / p99 | 1.89 / 3.98 ms | 1.71 / 4.15 ms |
| timer_wake p99 | 6.31 ms | 6.50 ms |

**The saving, stated (condition 3).**
- The allocating sections lost ≈20.9 M ticks of lock hold, under 1 % of the 2,550 M ticks of
  sections net of audits.
- The gate's lock waits did not fall: they are the hand-off to a halted waiter and the sections'
  other work.
- Total kernel time rose 1 % with 2 % more sections.

**A new longest section** in after: `time_now` 270k ticks (27 ms), in both after runs.
- It is a deadline's destruction run at a system call's entry. Every entry runs the deadlines due
  first (irq.rs:231). In "before" the same destruction fell in a timer interrupt's section.
- It is the same work under a different cause, not new work; R10's p99 is unchanged.

**In-flight high-water** (`inflight-trace` config, smp_inflight oracle):

| | before | after |
| --- | --- | --- |
| Most frames in flight | 80,734 | 24,396 |
| Frames retired | 80,764 | 80,764 |

- After, 56,433 frames were refilled from pending and 77,764 drawn from magazines.
- My prediction ("low thousands") was too low. Nearly all the frames retired are destructions'
  object chains, thousands at once on one hart, and refills take two a call at most.

A pre-SMP6 run (main 23d8ce412, recorded by mistake before I saw main had dropped SMP6) matched
"before" within 0.03 %: SMP6 saved nothing on this gate, as its report said.

## Pages and summaries checked

**Updated:** the docs listed above.

**Checked, no change:**
- README.md, GETTING-STARTED.md, kernel/README.md: no zeroing or magazine claims.
- docs/kernel/README.md: TCB unsafe counts unchanged, no new unsafe.
- docs/beyond/fpga-platform.md:206 lists magazines as implied kernel work, still true.
- model/README.md points to model.md.

## Open risks

- **Two frames' zeroing at a syscall's return, interrupts off.** About 60-90 µs. A shootdown aimed
  at this hart waits for that before its acknowledgement.
- **R84 saves little lock time on this gate** (above). Lock-free allocation waits for the lock split.
- **Frames in flight still reach 24k under destruction-heavy load.**
- **The new `smp-magazine-race`** is 8 s on each width at 2 harts.

Next: kernel-red review. A rebase onto SMP6's fixed head when it lands.
