# SMP5 design checkpoint: per-hart frame magazines

smp5-implementer, 2026-10-09, on main 58326f13c (SMP6 merged). No code yet.

## 0. The short of it, and the one blocking question

Every trap from user mode takes the one kernel lock (`irq.rs:158`), and every allocation runs
inside a call or a fault. So **an allocation already holds the lock**. On this kernel, a
magazine "drawn without the lock" saves only the bitmap walk: about five word reads and one
`mark_free`, under 1 µs. What the lock does hold per allocation in the containment gate is
**zeroing**. Under load the bitmap's frames are never-used (dirty) ones, because 79.5k freed
frames sit in flight and are never given back. Each allocation zeroes one under the lock, about
30–44 µs under `icount`. If the fill also zeroes under the lock (step 6 as written, and the
assignment's "the fill zeroes never-used or in-flight frames as allocation does today"), the
zeroing only moves into the fill. **Predicted saving: none.** The containment gate would show
nothing that SMP6 did not.

There is a real saving only if **the fill's zeroing runs outside the lock**. Two options:

- **Option P (plan as written).** Fill under the lock in batches, zeroing there. The magazine is
  structure for step 7 (finer locks), where "takes no lock" becomes true. Containment:
  no measurable change (§6).
- **Option X (recommended).** At a system call's return, after it releases the lock, the hart
  zeroes the frames that replace what its section drew, at most `REFILL_MAX = 2`, billed to that
  caller. They enter the magazine zero at its next entry. Most allocations then draw a zero frame
  and write nothing under the lock. Containment: endpoint_create sections lose ~30–44 µs each,
  about 2–3 s of ~225 s of net lock hold (§6). The refill takes this hart's **pending** frames
  first, so frames in flight get reused under load, and the 79.5k high-water should fall to
  roughly the magazines plus the frames freed since the last refill.

**Q1 (blocking, needs the Architect):** Option X amends R81's ruled "**Never at an exit to user
mode**" (memory.md, Frames in flight). That rule's reason was that the returning thread "did not
free those frames". Under X, a hart zeroes at an exit only frames that replace the ones **its
returning caller just took**: bounded at 2, billed to that caller. So "a call never pays for
frames it did not free or take" still holds. `icount`'s shared clock (A3's reason) is neutral
here: the same zeroing moves out of the lock, so it is not extra work beside another hart's. Go
with X, or with P and a stated nil saving?

Everything below holds for both options unless marked **X only**.

## 1. Size and owner representation

- **Owner.** One reserved owner, **`MAGAZINE`** (`Pid 0xfffc`, beside `IN_FLIGHT` 0xfffd, with
  the same `MAX_PROCESS_COUNT` assertion). A per-hart array says which hart holds the frame. The
  alternative, one reserved owner per hart, spends `MAX_HARTS` = 8 PIDs to say what the array
  already says, so I do not recommend it.
- **Not free, not anyone's.** `set_owner(i, Some(MAGAZINE))` clears the bitmap bit and the dirty
  bit (an owned frame is not free). Every owner check refuses it, as it refuses `IN_FLIGHT`.
- **Storage.** In `reclaim.rs` beside `LISTS`, one block per hart:
  `mag: [u32; MAG_MAX]` plus a count, holding frame number + 1, so 0 means none.
  **X only:** also `filling: [u32; REFILL_MAX]` plus a state word.
- **Sizes.** `MAG_MAX = 16` frames (64 KiB a hart, at most 128 frames or 512 KiB over 8 harts).
  `REFILL_MAX = 2`. The containment flood draws one frame a call (an endpoint), and a page fault
  or a one-page `map_anon` draws one or two. A larger run drains the magazine and falls back to
  the bitmap path, unchanged.
- **Guard.** The magazine is read and written only under the lock, with interrupts off (S-mode
  always runs them off outside `kmain`'s window). So another hart can take from it under the lock
  (§2, "return"). **X only:** the `filling` batch is the one thing touched outside the lock, and
  only by its own hart, exactly like idle zeroing's `zeroing` slot.

## 2. Draw, fill and return, and their bounds

**Draw.** `alloc_frame` takes in this order:

1. this hart's magazine (pop, `set_owner(MAGAZINE → owner)`, writes nothing);
2. the bitmap as today (a dirty frame zeroed under the lock);
3. `reclaim_wait` as today, with one new step before it zeroes a pending frame: take a frame from
   **another hart's magazine**, under the lock.

Constant work at every step (R12/K21).

**Fill, option P.** At each entry (`mem::entered`, after `commit`), a hart below `MAG_MAX / 2`
takes up to `REFILL_MAX` frames: done or committed first, then the lowest bitmap frame. It zeroes
any dirty frame under the lock. The bound is two frames of zeroing per section.

**Fill, option X.**

1. **Under the lock, at a system call's return** (`return_registers`, before `end_section`). If
   the section drew N frames (from the magazine or the bitmap) and the magazine is below
   `MAG_MAX`, the hart takes up to `min(N, REFILL_MAX)` frames into `filling`, owned by
   `MAGAZINE`:
   - its own pending frames first, which are shot down already, as idle zeroing's are. These
     are `IN_FLIGHT → MAGAZINE`, `in_flight -= 1`;
   - then the lowest bitmap frame, `None → MAGAZINE`.
2. **After `KERNEL_LOCK.release()`.** It zeroes each one (`kframe::zero`), with `hart::serve()`
   between frames, so a shootdown aimed at it waits at most one frame. Then it marks the batch
   done (an atomic, Release).
3. **At its next entry.** It moves the done batch into the magazine, under the lock.

So **a frame enters the magazine only zero**, and only under the lock. `resume` (a switch) and
`idle` do not refill: only the caller that drew is billed. The bound is two frames' zeroing added
to that call's return with interrupts off, about 60–90 µs under `icount`. The idle zeroing of
SMP6 is unchanged.

**Return.** Harts never stop once started (there is no HSM stop path), and a dying hart panics
the machine, so in this kernel "a hart that halts or dies" can only mean one that idles. I
propose that an idling hart **keeps** its magazine: returning it on every idle would cycle 16
frames per idle. Instead:

- the magazines are bounded (≤ `MAG_MAX` × harts);
- they are RAM no budget is charged for, like frames in flight;
- an allocation that finds the bitmap empty takes from any hart's magazine (step 3 of the draw)
  before it zeroes a pending frame.

So a budget able to pay always finds a frame (`map_fixed`'s `.expect` stays true). **X only:**
a `filling` batch is in flight to the bitmap's eye. `wait_done` also wakes on a batch marked done,
and the wait stays at most one frame's zeroing, since filling needs no lock. If you want a literal
return, the alternative is that an idling hart, when every hart is idle, gives its magazine back
to the bitmap. That costs refills on every wake, and I don't recommend it.

**R10 / the destruction audit.** A destruction frees nothing into a magazine: frees still retire
to in flight. The checked build's audit (`check_free_frames`) gains two counts:
`MAGAZINE`-owned frames = Σ magazine counts + Σ filling batches, and each such frame is on exactly
one hart's array. The partition is bitmap ∪ in flight ∪ magazines ∪ owned = RAM, and no magazine
frame is in any process's table (`check_frame_owners`).

## 3. How the in-flight drains feed magazines

- **Idle** (SMP6, unchanged). Done frames are committed to the bitmap at each entry. The
  magazine fills from the bitmap like any frame source, so idle work flows into magazines with
  no zeroing at fill.
- **On demand** (SMP6, unchanged in `reclaim_wait`). Its only addition is taking from another
  hart's magazine first (above).
- **X only, a third drain: the refill.** Under load, pending frames come back through
  magazines, zeroed outside the lock by the hart that freed them, at most two per call that
  allocated. This is the drain the containment gate lacks: today its frames in flight never come
  back.

## 4. Billing (R10, R12)

- **Under the lock** (both options): taking and moving frames is constant work per section and
  part of the section's own kernel time (R12). Option P's zeroing at fill is billed to the
  section's payer, as an allocation's zeroing is today. That is the next allocator, not the
  freer, the same price as before.
- **X only:** the refill's zeroing is timed (as `zero_taken` does) and billed at the hart's next
  entry to the budget of the thread whose system call drew the frames. That is its own demand:
  it took N frames and pays for at most N replacements. It is recorded as SMP6's `0` record with a
  payer, through `sched::zeroed`'s existing path. The thread's user time is closed by `leave()`
  before the release, so the zeroing is not user time. When a refill takes a pending frame, its
  freer's payer (the SMP6 per-hart payer FIFO) is not billed for it: the frame comes off the
  freer's debt unbilled, exactly as `zeroed_here` does for on-demand zeroing. So no one pays
  twice, and the freer never pays for a frame someone else's allocation zeroed.
- R12: `REFILL_MAX` is a kernel constant, so a call's added cost is bounded by a constant.

## 5. The rule, the model, two mutations

**R82 (frame magazines)**, beside R81 in memory.md. No branch claims R82; I will re-check at the
merge.

*A frame in a hart's magazine belongs to no process and is not free: the ownership table names
`MAGAZINE`, its bitmap bit is clear, and no call names it. It enters only zero and only under the
lock. Only that hart draws it, except when an allocation finds the bitmap empty, which may take
it, under the lock. A magazine holds at most `MAG_MAX` frames.*

**Model** (one hart, `model/src/kernel.rs`):

- `magazine: BTreeMap<u64, u64>` (frame → content).
- `alloc_frame` draws from it first.
- `Event::Commit`, which is time passing and already drawn, also refills the magazine up to
  `MAG_MAX` from `in_flight` (zeroing it), then from `free_frames`. **No new RNG draws.**
- Invariants: each frame is in exactly one of `frames`, `in_flight`, `free_frames` and
  `magazine`; every magazine content is 0; no table maps a magazine frame.

Mutations:

- **`R82MagazineUnzeroed`:** the refill from `in_flight` keeps the stale content. The new
  invariant fails, and I9 fails when the frame is handed out.
- **`R82MagazineStillFree`:** the refill copies a free frame without removing it from
  `free_frames`. The partition invariant fails, and the next two allocations alias.

Both go on R82's and memory.md's status lists. model.md's variant count rises by 2.

## 6. The attack case and the prediction

**`smp-magazine-race`** (2 harts, rv64 and rv32, checked kernel, `inflight-trace`). It builds on
`smp-inflight-race`'s programs and its memory-pressure setup.

- **The attacker** runs two threads on hart A: one-page `map_anon`, a pattern write, then
  `unmap`, at full rate, so that its frames cycle through pending, filling, magazine and drawn.
- **The racers** run in another budget on hart B. They allocate, first-touch, `map_fixed`, and
  `unmap`/lend/`process_map` their own pages at full rate, under pressure, so the bitmap empties
  and the steal path runs.
- **The witness** (rule F: a victim owning what is attacked, as in `mem-attack`) checks that every
  new page is zero and that its markers persist.

The kernel trace gains `m` (frame into hart h's magazine) and `d` (drawn, and by which hart).
Both letters are to be checked against what is free at the time. The oracle requires:

- every `m` lies after the frame's retire and before its draw;
- between `m` and its draw, there is no `o` (bitmap take), no retire and no other hart's draw
  unless it is a recorded steal;
- every frame entering a magazine is whole-frame zero (the checked kernel's audit, as
  `check_zero`).

The I1 partition stops the boot on a break.

Recorded negatives, both widths:

- `magazine-unzeroed`: the fill skips the zeroing. The entry audit and the witness fail.
- `magazine-still-free`: the fill leaves the bitmap bit set. The trace shows a frame drawn while
  in another's magazine, and I1 fails.

**Prediction from SMP6's hold-trace** (containment, 2 harts, rv64, main 050357e8a):

- 957,617 sections;
- 2,789 M kernel ticks, 540 M of them audits, so ~2,249 M net (225 s);
- lock waits of 1,947 M ticks: 1,048 M behind other harts' sections, 900 M with the lock free
  (the hand-off);
- 68,218 endpoint creations, each zeroing one never-used frame under the lock at ~30–44 µs.

| | Option P | Option X |
| --- | --- | --- |
| Net lock hold | unchanged (±noise) | −20 to −30 M ticks (2–3 s, ~1 %) |
| endpoint_create section | unchanged | −300 to −440 ticks each |
| Waits behind sections | unchanged | ~−10 M ticks (~1 %) |
| The hand-off (900 M) | untouched | untouched |
| In-flight high-water (79.5k) | unchanged | expected to fall to the low thousands |
| R10 p99 | unchanged | unchanged; destruction still retires |

The oracle prints only the five longest causes. I will add a report-only per-cause total for the
allocating causes (endpoint_create, map_anon, page faults) to the `hold-trace` line, so the
saving is measured, not inferred. One honest note: under X the saving is real but small against
the hand-off and the audits. The gate's lock time is dominated by the hand-off and the
sections' other work, not by zeroing, which was 15 s only while `kframe::zero` checked every word.

## 7. Paths (all owned)

- kernel/src/mem.rs (draw, steal, audit, `MAGAZINE`), reclaim.rs (magazine arrays, the X refill),
  arch/riscv/syscall.rs `return_registers` (X: the refill call).
  - **Q2:** syscall.rs is not in my owned list, though it is the return site. One call line is
    needed. OK?
- sched.rs (billing hook, trace letters), model/src/{kernel,mutation,invariants}.rs.
- tests/programs + tests/smp-magazine-race.toml, tools/testbench (the trace clauses and the
  per-cause line).
- docs: memory.md (R82, Frames in flight, the state diagram, Residual risks), model.md,
  budgets.md (who pays a refill), testbench.md rows (`m`, `d`, the per-cause line), the M2 page's
  step 6 and Progress, SECURITY.md (R82 row), invariants.md I1/I9.
- Size and unsafe budgets: no new unsafe expected (`kframe` already wraps the physmap).

## 8. Questions

- **Q1 (blocking).** Option X, which amends R81's "never at an exit" for at most two frames that
  replace what the caller took, billed to it? Or option P, with a stated nil saving on the
  containment gate?
- **Q2.** One call line in `arch/riscv/syscall.rs` (X only).
- **Q3.** The magazine is kept across idle, with the steal from another hart's magazine as the
  "return" (recommended), rather than given back whenever every hart idles.
- **Q4.** A new case `smp-magazine-race`, rather than a second phase of `smp-inflight-race` (which
  would lengthen a 2-hart case already near its timeout).
