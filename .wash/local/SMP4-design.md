# SMP4 design checkpoint: lock to decide, unlock to do

smp4-implementer, 2026-10-08, on main 19e3439b9. No code committed. Everything below was measured
at 2 harts (4 for `sched-lock-contention-4`), both widths, checked builds under `icount`, as the
cases run.

## 0. The short of it

1. **The waits that keep the five cases on one hart are not zeroing, copies or shootdowns.** In
   four of them most of a waiting hart's time sits behind the checked build's own checks: the
   destruction audit, about 20 to 25 ms a destruction (112 ms at the gate's full fill), and a
   whole-RAM ownership scan, `check_frame_owners`, about 7 ms at every process end. The scan is
   not stamped as an audit, so the oracle counts it as kernel work. A release kernel runs
   neither. In the share cases most of the "lock wait" is the lock standing free while the
   halted waiter waits for its turn under `icount`.
2. Real work does take long sections: zeroing through `kframe::zero` (about 220 µs a page, 30
   to 50 times a `memset`), `map_anon`'s whole-area search (9.4 ms), a fault's report printed under
   the lock (about 6 ms), and big unmaps and walks (72 ms for a full area once). Of these, the
   plan's step 5 (frames in flight) moves only the zeroing. The shootdown waits measure about
   0 (0.9 ms in total over the whole containment gate), and the kernel makes no large copies.
3. So step 5 as written does not un-keep the SMP4 node's list. What un-keeps it is an audit and
   billing change (§4, P1 and P2), plus the fault report (P3). I need a ruling on scope (§8)
   before writing code.
4. For the in-flight state itself I recommend **zero on reclaim** (variant A, §5): a freed frame
   goes into flight, is zeroed outside the lock by the hart that freed it, and only then returns
   to the bitmap. So every frame in the bitmap is zero, and allocation zeroes nothing under the
   lock. The page's own wording ("enters a process only after the work is done") describes
   variant B, zero on allocate with every mapping call split in two. B is much harder and costs
   a second lock acquisition per call. Choosing A is a design decision (§8, Q2).

## 1. Method

A scratch `hold-trace` kernel feature, in a detached worktree at `.tmp/SMP4/m` (never committed):
- at each release of the kernel lock it writes an `h` record (the section's cause and the raw
  ticks it was held) and `p` records for its parts: `kframe::zero`, the wait for shootdown
  acknowledgements, `find_virtual_address`, `destroy_subtree`, every `sched::audit`, and the
  destruction audit apart from the rest. A process's `terminate` is split into its steps
  (`release_ipc_frames`, `release_owned_frames`, `dma_release`, `process_ended`), and each
  section's release time is recorded;
- the oracle skips `h` and `p`;
- `.tmp/SMP4/holds.py` totals the sections by cause, net of audits. For each `Q` wait it
  attributes the wait to whatever the other harts' sections were doing as it overlapped them:
  audit or work, by cause.

The cases ran with `keep_smp` removed and `--smp 2`. Logs are in `.tmp/SMP4/logs/{before,attr,
parts,cont}`. Caveat: the records perturb timing a little. `sched-timer-flood` rv64 failed one
clause in the instrumented run and passed in the first.

## 2. What is long today (net of stamped audits unless said)

| Section | Length | Where seen | Release build? |
| --- | --- | --- | --- |
| Destruction audit (`audit_destruction`, stamped) | 19-25 ms; 112 ms after the gate's full-fill lease | every `budget_destroy`, every deadline destruction (inside the timer section) | no |
| `check_frame_owners` in `process_ended` (debug, **unstamped**) | 6.9-7.0 ms, every process end | `process_exit` and fault sections: 8.4-8.7 ms each; fault 15.6 ms | no |
| Fault report: `print_current_thread` + `print_map` under the lock | ~6 ms (fault 15.6 ms − 8 terminate − 1.7 audit) | `exc15` in `sched-exit-churn` | **yes** |
| `map_anon` whole-area refusal (search) | 9.4 ms (rv64), 9.2 (rv32) | `sched-lock-contention` | yes (R22-bounded) |
| `unmap` of a full area / `map_device` setup | 72 ms / 55 ms, once | `sched-lock-contention` | yes |
| `release_owned_frames` walk | ~0.95 ms small; 12.5 ms a full-area process | process ends | yes |
| `kframe::zero` | ~220 µs a page (two asserts and a volatile store a word); `paging::Window::zero_frame` is a `memset` | `endpoint_create` 0.43 ms p50 (zero 0.22); `map_anon` 1.8-3.6 ms (zero ~80 %); containment: 15.1 s of zeroing in 68,218 endpoint creates | **yes** (the asserts are `assert!`) |
| `destroy_subtree` net of audit | 0.8-1.0 ms small subtrees; 23 ms at full fill | all destructions | yes |
| Timer entry (slice end, reconcile) | 0.3-0.4 ms, on every hart every slice | everywhere | yes |
| Wait for shootdown acks | ≈0: 0.3 ms total in `sched-budget-churn`, 0.9 ms in the whole gate | | yes |
| Large copies | none: messages are words, pages move by remapping | | |

## 3. What each kept case's waits sit behind (attribution, rv64 / rv32)

| Case | Waits | Behind audits | Behind real work | Lock free (hand-off) |
| --- | --- | --- | --- | --- |
| `sched-budget-churn-shell` | 1902 / 1802 ms | **88 / 86 %**: `budget_destroy`'s audit | destroy 76, create 47, timer 63 ms | 5 / 7 % |
| `deadline-flood-billed-traced` | 1175 / 2626 ms | **85 / 89 %**: the deadline destruction's audit in its timer section | timer work 109-197 ms | 4 / 2 % |
| `sched-wake-no-preempt` | 224 / 264 ms | 24 % | `process_exit` 42 ms: that is `check_frame_owners`, unstamped | 44 / 55 % |
| `sched-exit-churn` (threads-exit 450 / 436) | 1985 / 2997 ms | 12 % stamped (+ the unstamped scan in each exit and fault) | timer 603-809, faults 135-411 (scan and print), exits 104-290 ms | 33 / 17 % |
| `kernel-containment` | 2 harts, 106 s and 101 s of waits | the 112 ms full-fill audit; 50 s of audits in all | 15 s of zeroing in endpoint creates, 180 s of timer sections | (built before the end-time record; not attributed) |
| `sched-share` (not kept) | 615 / 932 ms | 30 / 24 % of 101 ms | | **84 / 89 %** |
| `sched-lock-contention` | 3808 / 3851 ms | 17 / 18 % | `map_anon` searches 2.5 s | 1 % |

**Hand-off.** In the share cases about 0.48 ms per contended wait passes with the lock free: the
release has interrupted the halted waiter, and under `icount` (one host thread, round-robin) it
runs only at its turn. That is most of `lock waits 130-195 of 1000`. It is R78's halt under
`icount`, not anything in a section, and nothing in step 5 changes it.

**Why a share still misses net of waits.** The kernel bills a wait to the waiter and counts it
against its slice (scheduling.md, "Fair kernel entry is bounded by count"). So the stride queue
already charged the victim for its waits, and the oracle's subtraction cannot give back the picks
it lost: in `sched-budget-churn-shell`, 334 / 343 of 1000 net.

## 4. What would un-keep them (not step 5; scope question Q1)

- **P1. Every checked-build check under the lock is an audit.** Stamp `check_frame_owners` and
  `check_live_pids` (in `process_ended`) with `sched::audit`, so K18's rule ("a checked build's
  audits do not move its schedule") holds for them. This is a K18 gap, not a design change.
  Better still, make the scan proportional or move it to the destruction audit; the memory note
  "keep audits proportional" applies. Removes ~7 of every exit's 8.5 ms.
- **P2. An audit on one hart is not billed to another hart's wait.** Today the auditor's billing
  skips its audit, but a hart waiting behind it pays the wait. Proposed: a checked-build audit
  clock, the ticks every finished audit took plus the start of the one running. A waiter reads
  it at its draw and at its acquisition and takes the difference off its billing, as `audit`
  does for its own hart. The oracle subtracts the same from the wait (it already has `U`/`V`
  spans with harts). Release kernels have no audits and are unchanged. Predicted: 85-89 % of
  the waits gone in budget-churn-shell and deadline-flood-billed-traced, and all of
  wake-no-preempt's long waits (with P1).
- **P3. A fault's report leaves the lock**: print it after the release, or print a short line
  only (`print_map` is debug output). It is real release-build time, about 6 ms a fault. It
  overlaps CONW1 (one console writer).
- **P0 (cheap, real).** `kframe::zero` becomes one frame check and a `memset` (or calls
  `Window::zero_frame`): zeroing under the lock falls 30-50 times before anything moves out.
  That alone takes most of the 15 s from the gate's endpoint flood.

With P0-P3, `sched-exit-churn`'s remaining waits are timer sections and the hand-off. Whether
threads-exit reaches 450 on rv32 is then a measurement, not a promise.

## 5. The in-flight state (step 5 proper)

### Variant A, recommended: zero on reclaim

- **State.** A RAM frame in flight is owned by `IN_FLIGHT` in the ownership table, a reserved
  PID like `OBJECT_OWNER` and `DMA_OWNER`. Its bit is clear in the free bitmap. It is in no
  process's tables and on exactly one hart's reclaim list, linked through its own first word
  (it is about to be zeroed), so the list needs no memory.
- **Decide (under the lock).** Every path that frees a RAM frame (`unmap`, a destruction's
  `release_owned_frames`, page-table frees, an abandoned lend's free, object frames at
  `free_object_frame`, a failed `map_anon`'s rollback) calls one `retire(frame)`: the owner
  becomes `IN_FLIGHT`, the frame goes onto this hart's list, and the budget is uncharged now,
  as today. If the call shot the process down, the frames are retired only after the acks (the
  wait stays under the lock; §6).
- **Do (lock released).** Before the hart returns to user mode or idles, it zeroes its list
  through the physmap. The zeroing is billed to the budget whose call freed the frames (the
  destroyer, or the deadline's payer, as R10 bills today). That is the same payer as now, so
  R12's charge moves no one.
- **Commit (under the lock again).** The hart takes the lock on a fresh ticket and puts each
  zeroed frame into the bitmap (`set_owner(None)`: constant time per frame, K21). A call that
  freed nothing takes no second section.
- **Allocation zeroes nothing.** `alloc_page`, `alloc_object_frame` and `dma_pool_take`
  (DMA keeps its own pool and is unchanged) take a frame that is already zero. The physmap
  zeroing in `map_run`, `map_fixed`, first touch, page tables and the header page goes; a
  checked build asserts on a sample word (or the whole frame, as an audit) that a frame leaving
  the bitmap is zero.
- **Boot.** Frames free at boot are not known to be zero. Either the loader or `boot_budgets`
  zeroes all free RAM once (cost: 512 MiB under `icount`, to measure), or level 0 gets a
  "dirty" twin that is set at boot only, and an allocation of a dirty frame zeroes it under the
  lock as today. I prefer the twin: no boot cost, and the dirty set only shrinks.
- **RAM against the budgets.** `map_fixed`'s `.expect` assumes that a budget able to pay finds
  a free frame. With frames in flight, budgets' free pages are backed by the bitmap plus the
  frames in flight. An allocation that finds the bitmap empty while frames are in flight
  therefore waits: it releases the lock, halts until a commit's interrupt, and retries. The
  wait is bounded by one hart's list of zeroing per hart, and lists are capped (`RECLAIM_MAX`,
  say 64 frames; past it a hart commits early, with an extra section). This is the one new
  wait, and the attack case drives it.
- **SMP5 fit.** Step 6 refills magazines from a bitmap that is all zero, so "frames zeroed as
  they enter a magazine" holds with no work in the refill.

### Variant B (the page's literal wording): zero on allocate

Allocation retires frames to in-flight, releases the lock, zeroes them, takes the lock and maps.
Every mapping call becomes two sections, and the second must find its process alive and the
chosen range still free. Another thread of the process may `map_fixed` or `unmap` over it in
between (SMP3), or a destruction may end the process. So it needs placeholder entries in the
caller's tables and a revalidation step in every call. It also pays a second ticket wait in
every mapping call; at 2 harts that is up to one section more (§3's hand-off alone is ~0.5 ms
under `icount`). Its only gain over A is no boot-dirty handling.

## 6. What moves outside the lock, in order

1. **P0**, the cheap zero (independent; first).
2. **Variant A** frames in flight: `retire`, per-hart reclaim lists, zeroing between release and
   return, commit; boot-dirty twin; allocation's wait-for-commit; the checked build's audits
   extended (bitmap ∪ in-flight ∪ owned = all RAM; no `IN_FLIGHT` frame in any table).
3. **Destruction**: the walk stays under the lock (constant per frame, and it reads the
   ownership table), and the per-frame zeroing it now forces on the next allocator moves out.
   Measured, destruction net of audit is ~1 ms, or 23 ms at full fill; the gain there is the
   later allocations' zeroing, not the walk.
4. **Shootdown waits**: I recommend they stay under the lock for now. They measure ≈0 under
   `icount`, and moving them needs concurrent shooters: `SHOOTER` is one static that assumes
   the lock's holder, so each hart's block would need one request word per asking hart. If you
   want it anyway, the order is: clear entries and ask under the lock, retire the frames as
   "waiting for acks", release, wait halted, then zero and commit. Q3.
5. **`map_anon`'s search** stays: it reads the caller's own tables, which another thread of the
   process can change (SMP3). It is R22-bounded, and step 7 (finer locking) is where it can move.
6. **Copies**: none to move.

## 7. The rule, the attack case and the model

- **Rule, beside R11** (number to assign: R79 was just taken by the steward's contexts, so R80
  unless CTX packages claim it first): *A frame in flight belongs to no process and is not free.
  No call can name it, only the hart that retired it touches its contents, and it re-enters the
  bitmap only under the lock, zero, and after every hart that could hold a translation to it has
  acknowledged.* With variant A, R11's "every page is zeroed before a process first sees it"
  becomes "every free frame is zero".
- **How a racing hart is refused.** Allocate: it draws only from the bitmap, and an in-flight
  frame's bit is clear. Map: every map path takes its frame from allocation, and no call names a
  physical frame. Free, lend, transfer and `process_map` act only on frames the ownership table
  credits to the caller, and `IN_FLIGHT` is no one's. A double retire, or a commit of a frame
  that is not `IN_FLIGHT`, panics as I1.
- **Attack case** (`smp-inflight-race`, 2 harts, checked kernel with a feature that records
  `i` (retired), `z` (zeroed) and `c` (committed) per frame, and records each allocation's
  frame). On hart A, a budget fills pages with a pattern and destroys a child in a loop (and
  unmaps big runs), so many frames are in flight. On hart B, at the same time, a process
  `map_anon`s, `map_fixed`s, first-touches, lends and unmaps at full speed, driving the
  allocator into the empty-bitmap wait. The verdict is the system's (rule F): the oracle reads
  the trace and requires that no frame is allocated between its `i` and its `c`, and that every
  `c` follows its `z` and, for shot-down frames, the acks. The checked kernel's audit
  (bitmap ∪ in-flight ∪ owned is all RAM; no in-flight frame mapped; a frame leaving the bitmap
  is zero) stops the boot on a break. B's own read of zeroes is printed but not the verdict.
- **Its negative** (the kernel red's pattern): a kernel feature `inflight-early-commit` that
  puts a retired frame back in the bitmap before it is zeroed. Its audit must read the same
  broken rule, or the checked kernel's audit catches it first. The case must fail on the
  oracle's clause in a recorded negative run, both widths.
- **Model.** The model keeps one hart. A retire is a step, and the commit is a separate event
  the generator interleaves later, with other calls in between: `free_frame` moves a frame to
  `in_flight` (with its stale content), and `Event::Commit(hart)` zeroes it and moves it to
  `free_frames`. New invariant: each frame is in exactly one of owned, `in_flight` and
  `free_frames`, and every frame in `free_frames` is zero. Mutation **`R80InFlightAllocatable`**:
  `free_frame` also leaves the frame in `free_frames`, so an allocation can take it before its
  commit; the commit then zeroes a page a process owns, and the checker fails "a mapped page
  changed with no store". A second mutation, **`R80CommitUnzeroed`**, commits without zeroing,
  and the new invariant catches it. `R11NoZeroing` moves to the commit.

## 8. Measurements that will judge it (before → after, both widths, 2 harts)

- `h` sections by cause (the scratch tool, or a committed `hold-trace` feature if you want it
  kept: Q4): p99 and max net of audits; zeroing under the lock ≈0.
- Per kept case: its share line, `lock waits N of 1000`, and the attribution split (audits,
  work, hand-off). The table in §3 is the "before".
- `sched-lock-contention` / `-4`: driver wake p50/p99 (18.5/19.2 ms, and 3.9/59.4 and
  5.8/63.8 ms at 4 harts in this run); lock order's wait p50/p99/max.
- R10 p99 in `kernel-containment`/`sched-latency` (the destroyer now pays the zeroing it caused
  after its lock section; R10's kernel section shrinks, the call's total does not).
- The new `smp-inflight-race` and its negative; `bench:scan-bounds` (K21) unchanged.

**IRQ1 caveat (from irq1-implementer, after this was written).** Today a hart halted for the
lock or a shootdown spins while SEIP (boot hart) or a fired STIP is pending, because `wfi` ends
at once. IRQ1 masks `sie` to SSIE around those halts and enables device interrupts on every hart.
That changes the hand-off column of §3 (the lock-free part of a wait) and the driver-wake
numbers, and possibly the split between audits and work. Every "before" number here is taken
again on top of IRQ1 once it lands (or on wp-IRQ1), so that both packages measure the same
kernel. The audit and scan findings (§2, the section lengths) do not depend on the wait's halt.

## 9. Questions for the orchestrator

- **Q1 (scope).** The five cases are decided by checked-build audits (P1, P2) and the fault
  report (P3), not by step 5's work. Should SMP4 carry P0-P3 so that it can un-keep them, with
  step 5 (variant A) delivered alongside on its own merits? Or does SMP4 deliver step 5 only,
  with P1-P3 a separate package and the un-keeping moving to it? P2 changes billing in the
  checked build only. I would ask the kernel red to look at it, since "audits do not move the
  schedule" becomes a cross-hart rule.
- **Q2 (variant).** Zero on reclaim (A) rather than the page's zero on allocate (B)? A means
  editing several-harts step 5's wording ("enters the free-frame bitmap only after it is zeroed
  and acknowledged") and R11's "zeroed" bullet. It is a design decision: Architect?
- **Q3.** Shootdown waits stay under the lock (≈0 measured), with the reason written as a
  residual? Or move them anyway?
- **Q4.** Keep `hold-trace` (section lengths at release) as a committed test feature, as SMP2's
  handoff suggested, for this and step 7's measurement gate?
- **Q5.** The rule number: R80, unless you assign another.
