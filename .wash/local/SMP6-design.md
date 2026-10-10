# SMP6 design checkpoint: a freed frame is zeroed outside the lock

smp6-implementer, 2026-10-08, on main bf3fa75d2. No code yet. Rulings taken as given: zero on
reclaim (variant A of SMP4-design.md §5), shootdown waits stay under the lock (residual),
`hold-trace` is the measuring tool.

## 0. The short of it

- A freed RAM frame becomes **`IN_FLIGHT`**'s in the ownership table (a reserved PID like
  `OBJECT_OWNER`/`DMA_OWNER`), its bitmap bit clear. No call names it; nothing allocates it.
- The hart that freed it zeroes it **after releasing the lock**, which is after every shootdown
  its section made (shootdowns are synchronous under the lock). Committed back to the bitmap
  under the lock by whichever hart next holds it. Allocation zeroes nothing.
- One trap the SMP4 sketch missed (§3): the list cannot be linked through a frame's own word
  until that frame's shootdown has completed, or a stale-TLB store on another hart rewrites the
  link and the kernel follows a user-chosen pointer through the physmap. Frames freed before
  their call's shootdown are *staged* in a small per-hart array and linked only after it.
- Rule **R81** (CTX2 claims R80 on wp-CTX2; main's last is R79). Model: `Event::Commit`;
  mutations `R81InFlightAllocatable`, `R81CommitUnzeroed`. Attack case `smp-inflight-race`.

## 1. Where the state lives

| Item | Where | Guarded by |
| --- | --- | --- |
| Frame is in flight | `allocations[i] == Some(IN_FLIGHT)` (`Pid 0xfffd`); level-0 bit clear (`set_owner` does it: owned = not free) | the lock |
| Count in flight | `MemoryManager::in_flight: usize` (for the audit and allocation's wait) | the lock |
| Staged (freed, its call's shootdown not yet done) | per hart `staged: [u32; STAGE_MAX]`, `STAGE_MAX = 64`, in `MemoryManager` indexed by hart | the lock |
| Pending (shot down, not zeroed) | per hart `pending: Option<u32>` head, linked through word 0 of each frame | the lock while filled; the owning hart alone after its release |
| Done (zeroed but word 0) | per hart `done: AtomicU32` head, Treiber push by the zeroer, swap-all by the lock holder | atomics (one pusher, one swap-all consumer: no ABA) |
| Boot-dirty | `dirty`: a twin of bitmap level 0, set at boot for every frame free then, cleared at first allocation | the lock |

No frame table field is added; the ownership table already is the per-frame record.

## 2. The order

1. **Free (under the lock).** Every path that gives a RAM frame back calls one
   `retire(index, shot)` instead of `set_owner(index, None)`: `release_page`/`free_frame_of`
   (unmap, an abandoned lend, a header page's rollback, a failed `map_run`'s undo),
   `free_empty_tables`, `release_owned_frames`, `free_object_frame`/`free_deferred_frames`,
   `alloc_page`'s failed charge. Owner becomes `IN_FLIGHT`, `in_flight += 1`, the budget is
   uncharged now as today (R6 unchanged).
   - `shot = None`: no hart can hold a translation to it: an ended process after its `Leave`
     (destruction, `release_owned_frames`, checked: no hart runs the PID), an object frame (never
     mapped), a process that never ran, the failed-charge frame. Linked onto `pending` at once.
   - `shot = Some(pid)`: its entry was just cleared and the call's shootdown is still to come
     (`unmap`, `free_empty_tables`, `free_abandoned_lend`, all of which `shoot` at their end).
     Staged. `shoot_for` links the staged frames onto `pending` after its acks. If staging
     fills mid-call, the call shoots `pid` early (one more shootdown; measured ≈0) and links.
     Checked: staging is empty when the lock is released.
2. **Shootdown complete** (under the lock, as ruled): `hart::shootdown` returns only when every
   asked hart has acknowledged. A hart that ran the PID before but not now flushes before it
   runs it again, and a reused PID's ASID is flushed: neither can store through a stale entry.
3. **Zero (lock released).** At each release site that leaves the kernel (`return_registers`,
   `resume` to user, `idle` before its halt), right after `KERNEL_LOCK.release()`: pop each
   pending frame, zero words 1..511 through the physmap (one `memset`, `kframe::zero`'s check),
   push it onto `done` with word 0 as the link. Between frames it calls `hart::serve()`, so a
   shootdown aimed at this hart (its block still names the process it returns to) waits at most
   one frame, not the list. Interrupts stay off as today.
4. **Commit (under the lock).** At every acquisition, after the ticket, the holder swaps every
   hart's `done` (MAX_HARTS heads) and commits up to `COMMIT_MAX = 64` frames: checked
   `owner == IN_FLIGHT`, clear word 0, `set_owner(None)`, `in_flight -= 1`. The rest wait on a
   lock-held `committing` list for the next section. Constant work per section (K21/R12); each
   frame is committed once.

**Allocation** (`alloc_frame`) takes from the bitmap. Frames are zero there, so `map_run`,
`map_fixed`, first touch, `new_in`/`install_table`'s tables, the header page and
`alloc_object_frame` write nothing (the `zero_frame`/`kframe::zero` calls go; a checked build
asserts one sample word, and the commit audit checks whole frames, §6). If the bitmap is empty:
commit from `committing`/`done` lists; else take a frame from this hart's own `pending` (shot
down already) and zero it under the lock; else, if `in_flight > 0`, wait under the lock, halted,
for another hart's `done` push (the zeroer sends an IPI when the allocator's `want` flag is set),
like the shootdown wait. Bounded by one frame's zeroing; no deadlock (zeroing needs no lock,
and a hart with a pending list never waits for the lock before emptying it). So `map_fixed`'s
`.expect("charged for above")` and `process_map`'s stay true: a budget able to pay finds a
frame now or within one frame's zeroing.

**Boot frames.** Free at boot is not known zero. Recommended: the `dirty` twin (one bit a frame:
16 KiB at 512 MiB), checked at `alloc_frame`; a dirty frame is zeroed there under the lock as
today, once. No boot cost; the dirty set only shrinks; lowest-first reuse means a running system
mostly reuses committed frames. Alternative (Q3): zero all free RAM at boot; I will measure its
boot time under `icount` at 512 MiB before choosing.

## 3. The race a second hart can make (the attack case's matter)

| Racer does | During | Refused by |
| --- | --- | --- |
| allocate the frame | staged/pending/zeroing/done | its bit is clear; `alloc_frame` reads only the bitmap |
| map it (`map_anon`/`map_fixed`/first touch/`process_map`'s tables) | any | every map path takes frames from allocation; no call names a physical frame |
| free/lend/transfer/`process_map` it | any | those act on frames the ownership table credits to the caller; `IN_FLIGHT` is no one's (`claim_release_move` returns `InUse`) |
| store through a stale TLB entry | between clear and ack | the zero comes after the ack (step 3 follows the release, which follows the section's shootdowns); the link word is written only after the ack (staging) |
| store through a stale entry after the ack | | none exists: the asked harts flushed; others flush before running the PID |
| `dma_alloc` | | the DMA pool is separate and never in flight (unchanged; it still zeroes on take, under the lock: residual, 1024 frames max) |
| double retire / commit of a frame not in flight | | panics as I1 |

## 4. Billing the zeroing

Zeroing after release would otherwise accrue as user time to whatever thread the hart resumes
(wrong after an exit, a destruction or a deadline's timer section). Proposed: the hart records
the releasing section's `Payer` with its pending list and times the zeroing; at its next
acquisition it bills those ticks to that payer (R10's payer: the destroyer, the deadline's
payer) and subtracts them from the user time it closes (the same subtraction SMP4's `excused`
makes, but in every build). A trace record `z` (id = ticks, pass = frames) after the `Q`, which
the oracle subtracts from shares as it does `y`. Q2.

## 5. The rule (R81, beside R11), the model, mutations

**R81 (frames in flight).** *A freed RAM frame is in flight until it is zero: it belongs to no
process and is not free. No call names it and no allocation takes it; only the hart that freed it
writes it, and only after every hart that could hold a translation to it has flushed; it
re-enters the free-frame bitmap only under the lock, and only zero. So every free frame is zero,
and allocation writes nothing.*

**Model** (one hart). `free_frame` moves the frame to `in_flight: BTreeMap<u64, u64>` with its
stale content. A new `Event::Commit` the generator interleaves among calls zeroes one (the
lowest) and moves it to `free_frames`. `alloc_frame` takes only from `free_frames` and no longer
zeroes; the fresh-frame branch stays 0. Invariants: each frame in exactly one of `frames`,
`in_flight`, `free_frames`; every `free_frames` content is 0; I9's "handed out zeroed" stays. The
new event picks from values already drawn (no extra RNG draws: they shift seeds and break the
mutation deadlines). Mutations: **`R81InFlightAllocatable`** (free also inserts into
`free_frames`: an allocation takes a stale frame, I9 fails, and the later commit zeroes a mapped
page); **`R81CommitUnzeroed`** (commit keeps the content: the new invariant fails).
`R11NoZeroing` moves to the commit (zeroing skipped at commit), or merges with
`R81CommitUnzeroed`: I would keep R11's name and add only `R81InFlightAllocatable` if you prefer
fewer mutations. Q4.

**Attack case `smp-inflight-race`** (2 harts, rv64 and rv32, checked kernel, feature
`inflight-trace`). Attacker process, two threads: T1 stores a pattern into its pages in a tight
loop; T2 `unmap`s them (big runs, past `STAGE_MAX`), re-maps, and destroys a child that filled
pages, in a loop, so frames are always staged, pending and done. A witness in another budget, a
victim that owns what is attacked (rule F, as `mem-attack`): `map_anon`s, first-touches,
`map_fixed`s, creates endpoints at full speed, checks every new page is all zero, writes its own
marker and re-reads it after a delay (catches a late zero landing in its page and a stale
store); it prints the verdict line. The kernel's trace (`i` retired, `o` taken from the bitmap,
`b` committed, by frame index; free kinds b d i o p s z) lets the oracle require: no `o` of a
frame between its `i` and its `b`; every `b` after its `i` and after the retiring section's
shootdown record; and the checked kernel's commit check (whole frame zero, an audit) and I1
partition (bitmap ∪ in flight ∪ owned = RAM, no in-flight frame in any table) stop the boot on
a break. Negatives, recorded runs, both widths: `inflight-early-commit` (retire goes straight to
the bitmap, zeroed later: the oracle clause and the witness's marker fail) and
`inflight-zero-unshot` (link and zero before the call's shootdown, under the lock: T1's stale
store lands in a bitmap frame, the witness reads the pattern). If QEMU's softmmu TLB cannot be
made to show the second deterministically under `icount`, its verdict is the oracle's ordering
clause and I say so in the status line.

## 6. Pages to change (drafts)

**R11 bullet** "Every page is zeroed" becomes: *"**Every free frame is zero.** A frame enters the
free-frame bitmap only zero, by R81, and a frame free at boot is zeroed when first taken; so
anonymous and fixed pages, backed reservations, page tables and the header page are zero when a
process first sees them, and `dma_alloc` pages are zeroed as they leave the pool. Pages moved by
lend, transfer or `process_map` carry their contents."* Backing and zeroing, the state diagram
(Mapped → In flight → Free, "zeroed on the way back"), the figure note and Why ("Zero on
allocation" → "Zero on reclaim, outside the lock") follow. R11's status line keeps "attacked
only in the model" unless the witness case is accepted as attacking reuse (it reads reused
frames: I think it closes that gap; your call).

**M2 page, step 5:** *"5. **Lock to decide, unlock to do.** A freed frame is zeroed outside the
lock by the hart that freed it: it passes from its owner into an in-flight state no other hart
can name, is zeroed only after the shootdown of its last mapping has completed on every hart
asked, and enters the free-frame bitmap zero, under the lock again; allocation zeroes nothing
([R81](...)), with an attack case (a second hart racing to allocate, map or free the in-flight
frame). Measured at two harts under `icount`, zeroing was the only frame work long enough to
move: 220 µs a page before `kframe::zero` lost its per-word check, a fifth of that after, and 15 s
of zeroing in the 68,218 endpoint creates of the containment gate. Shootdown acknowledgements
waited 0.9 ms over that whole gate and the kernel makes no large copies (messages are words,
pages move by remapping), so both stay under the lock ([memory](...#residual-risks))."*
Step 6: "frames zeroed as they enter a magazine, by step 5's rule" → "a magazine is filled from
the bitmap, whose frames are already zero (step 5)". The closing paragraph's package split is
unchanged.

**Also checked:** invariants.md I9 (wording "zeroed"), SECURITY.md register (new R81 row, R11
row), memory.md Residual risks (shootdown under the lock, already written by SMP4; DMA pool
zeroed under the lock), scheduling.md (the zeroing's billing, R10), the model's README mutation
list, the M2 progress section, kernel README if it lists rules. Reported per path at the end.

## 7. Measurement (before = bf3fa75d2, after = the branch; hold-trace, 2 harts, both widths)

- Section length p50/p99/max net of audits, by cause, for `endpoint_create`, `map_anon`,
  `map_fixed`, page faults (first touch), `unmap`, `process_exit`/destroy, the timer: in
  `sched-lock-contention`(-4), `kernel-containment` (its endpoint flood) and the six SMP4 cases.
  Expected: map and create sections lose their zeroing; free sections unchanged (retire is a
  bit as before) plus at most an early shootdown per 64 staged frames.
- Zeroing outside the lock: total and per hart, from `z`.
- Lock waits and their split (the oracle's line), driver wake p50/p99, R10 p99 in
  `kernel-containment`/`sched-latency`, allocation waits for a commit (count, should be ~0 off
  the attack case).
- Boot time with the dirty twin against boot zeroing (Q3).
- `bench:scan-bounds` (K21) unchanged; size budget and unsafe count reported.

## 8. Questions

- **Q1.** Staging (per-hart array of 64, early shootdown when full) so the link is written only
  after the ack, rather than a link table (a u32 a frame, 0.1 % of RAM) or reordering every free
  path to shoot first? I recommend staging.
- **Q2.** Bill the zeroing to the retiring section's payer at the next acquisition, subtracted
  from the resumed thread's user time in every build, with a `z` record? Or let it fall as user
  time of whoever the hart resumes (simpler, misbills exits and deadline destructions)?
- **Q3.** Boot frames: the dirty twin (recommended) or zero all free RAM at boot (measure first)?
- **Q4.** Mutations: `R81InFlightAllocatable` and `R81CommitUnzeroed`, with `R11NoZeroing`
  moved to the commit; or fold the last two?
- **Q5.** Rule number R81 (R80 is CTX2's on wp-CTX2). Re-checked at the merge.
- **Q6.** Commit at every acquisition (up to 64 frames, any hart's done list), not in a second
  section by the zeroing hart: no extra ticket wait (~0.5 ms hand-off under `icount`). Agreed?
