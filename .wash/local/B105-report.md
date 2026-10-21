# B105 report

Base 4750dd61c, branch wp-B105, worktree .worktrees/B105.

## Checkpoint: paths found and the plan (2026-10-21)

### What failed in B100's line

Not the binary. `m` is 9,362 × 7 = 65,534 bytes. The same line, evaluated by `Code.eval_string`
under the host `beamlet` (scratch in .tmp/B105/repro), with gdb on `__rust_realloc` over 1 MB:
283 reallocations of the process heap's cell vector (`Heap.terms: Vec<Term>`, 16-byte cells),
between 1.0 MB and 3.6 MB, the 3.27 MB failure among them; nothing else over 1 MB but one
1 MB alloc. erl_eval's interpreted comprehension keeps about 1.2 MB live
(`process_info(memory)` 1,246,448 at the end).

The vector grows by `Vec` doubling (`push_object`, `cons`: userland/otp/vm/src/term/mod.rs
796-833); a collection starts the to-space at `gc_at / 2` cells (`Process::collect`) and
evacuation doubles it again (term/gc.rs). The process's usage counts the cells in use, not the
capacity, so a heap of about 1.6 MB, under the 2.85 MB sixteenth, asked the allocator for 3.27 MB
while holding its 1.6 MB, and `Vec`'s infallible growth turned the refusal into a panic.

rv64, not rv32: cells are 16 bytes on both widths (`Ptr` is two u32s, `Int` an i64) and the
limits are the same bytes, so both ask for the same; what differs is the free pages. The VM's own
footprint (pointers, `Arc`s, `Vec` headers, decoded code) is larger on rv64, so at that moment
rv32 still had the room. (To be confirmed from the footprint lines of a boot of each width.)

### Paths a process drives to one large allocation

1. The heap cell vector's own growth (above): between collections, at a collection (to-space),
   and inside one instruction that builds a big object.
2. A binary's construction or growth: `bits::Builder`'s `Vec<u8>` in `bs_create_bin`, and the
   in-place `private_append` buffer, which also grows by doubling.
3. Natives that build one object of a size the process names: `make_tuple`, `list_to_tuple`,
   `tuple`/`append_element`, `lists:duplicate`, `binary_to_list`, `++`, `binary:copy`,
   `list_to_binary`/`iolist_to_binary`, `term_to_binary`, and the like.
4. A message's copy into its receiver's heap (term/copy.rs `reserve`): one sender, one
   allocation.

### Plan

- Free pages are the allocator's own answer: every growth on these paths goes through
  `try_reserve_exact` (amortized first, then exactly what is needed), never `Vec`'s infallible
  growth. Redoubt's heap already returns null at its cap or when `map_anon` is refused, so no
  platform or allocator contract changes (no re-tier).
- The heap limit is checked before the allocation: the process's usage plus what it asks,
  against the VM's `max_heap_words` and its own `max_heap_size`.
- Where: BEAM's `test_heap` shape. An instruction or native that would allocate past the heap's
  spare capacity says how much it needs before changing anything; the interpreter, now at a safe
  point, collects, checks live + need against the limit, reserves fallibly, and runs the
  instruction again. Past the limit or refused by the allocator, the process is killed as
  `max_heap_size` kills it (`killed`, the error report), its heap freed, and the VM serves on. The
  heap keeps a fixed headroom reserved at each safe point so small allocations never grow it
  mid-instruction; a collection reserves its to-space fallibly before it starts.
- Stated backstop, unchanged: a native's own internal allocation (a zlib codec, crypto), several
  processes at once, and any VM-side allocation (code loading, ETS, the VM's tables).

### Cases planned

- Host tests, one per path (vm/tests): a process asking past its limit is killed and the VM runs
  the next process; a capped test allocator stands in for free pages, so a refusal is exercised
  for the heap growth, a binary, a tuple, a list copy and a message copy.
- A boot case on both widths: B100's line at alice's shell, then a binary past the process's
  limit (killed, the shell printing the exit), then a next line answered. B100's line itself may
  now answer rather than be killed, since the exact fallback needs about half what doubling did:
  the case requires only that the VM serves on and prints one or the other.
