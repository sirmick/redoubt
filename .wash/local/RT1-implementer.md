# RT1: libs/rt's `unsafe` down to its floor, and what stays attacked

Tier A (the runtime every native server, `init` included, runs on), size S. Needs INIT2. INIT2
adds the bundle view (`start.rs`) and the fixed arena's run words (`heap.rs`, `Heap::fix`), so
RT1 starts from the merged runtime. The owner asked for this package. It is TCB hygiene, not a
hole: a process is memory-safe from other processes whatever its own runtime does.

Every cargo and bench command runs as `/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from
the worktree.

## Rules first

- `.wash/SWARM.md`: "The implementer" and "Staging, commits and handoffs". Stage by path. Never
  use `git add -A`, `git commit -a` or `git stash`.
- TENETS "`unsafe` is a budget": every `unsafe` states its invariant, lives in few small modules
  behind safe interfaces, and the count goes up only with a reason.
- The design is read-only. A gap is a blocking question to the Architect.
- No behaviour changes. Every rt and server host test, and every case, stays green unchanged.
  The only exception is the panic hook's registration (deliverable 1).

## Where it starts (after INIT2: `max_unsafe = 14`)

| File | Sites | Verdict |
| --- | --- | --- |
| `heap.rs` | `push` write, `pop` read (class free lists); `run` read, `set_run` write (arena runs) | 4 -> 2 (deliverable 2) |
| `heap.rs` | `unsafe impl GlobalAlloc`, `alloc`, `dealloc` | floor: the trait |
| `handle.rs` | `Registers::read_u8` / `write_u8` volatile | floor: the hardware; one read, one write |
| `ipc.rs` | `Mapping`'s shared and mutable views | floor (see "Not done") |
| `start.rs` | the startup page view, the bundle view | 2 -> 1 (deliverable 3) |
| `start.rs` | the panic hook's `transmute` back to `fn()` | 1 -> 0 (deliverable 1) |

The target is 10. Count what the merged runtime actually holds before you start, and report it.
If it is not 14, the table above is wrong where it differs: say so.

## Deliverables, in order

1. **The panic hook is known at compile time, not stored as an address.**
   - Today `set_panic_hook` stores `hook as usize` in an `AtomicUsize`, and the panic handler
     transmutes it back. netd is the one user: `arm_panic_reset` keeps the registers in its own
     statics and then registers the fixed `panic_reset`.
   - Instead, `entry!(run, panic_hook = f)` names the hook. The macro emits the program's
     `#[panic_handler]`, which calls rt's report-and-exit with `Some(f)`. rt's run-once logic
     (`HOOK_RAN` / `HOOK_DONE`) takes the `fn()` as an argument. `set_panic_hook` and
     `PANIC_HOOK` go.
   - netd's hook already does nothing until it is armed. `arm_panic_reset` keeps its statics and
     stops registering anything.
   - Check how `tests/programs` defines its own `#[panic_handler]` beside rt's, and keep that
     working.
   - If no sound way exists without an `unsafe` somewhere else (an `extern` symbol, or an
     `unsafe impl Sync` cell), keep the transmute with its reason, and say why in the report.
     That is a fine outcome.
   - Keeper: netd's `panic_reset` test, rewritten for the new form.
2. **The heap's raw words go through one private pair.**
   - `pop`/`push` and `run`/`set_run` all read or write the first two words of a block that is
     on no live allocation, in a page this heap keeps mapped read-write, under the lock.
   - Fold them onto one private `fn words(&self, &Locked, addr) -> [usize; 2]` and one
     `fn set_words(&self, &Locked, addr, [usize; 2])`, by value, with no references made.
   - The SAFETY comment is written once on each, and names every caller.
   - The smallest class is 16 bytes, aligned to its size, so two words fit on rv32 and rv64.
     A test asserts it.
3. **The startup page and the bundle view go through one private fn.**
   - Both are pages mapped into this process before its first instruction, never written by it,
     and never unmapped: the runtime's `unmap` is private to the heap and `Buffer`.
   - The guarantor differs: the parent through the loader stub for the startup page, the loader
     for the bundle. The one SAFETY comment names both.
   - Write it as a private, safe `fn premapped(addr, len) -> &'static [u8]`, like `push`: "private,
     and called only by ...".
4. **Attack what stays.** One keeper per site.
   - **Miri over the heap, in the bench.** `heap_over_map_anon` and INIT2's arena tests live in
     `tests/ipc.rs`, which `rt-miri` does not run (it takes minutes), so today the free lists and
     the runs are not under Miri in the bench. Move the heap's tests to their own fast file in
     `rt-miri`'s list. Cover: every class, a page's worth of blocks, frees in another order, a
     large block, and the fixed arena's first fit, split, exhaustion and reuse.
   - **`Mapping`'s views:** `mapping_views` already does this, under Miri. Keep it.
   - **`Registers`:** a host test over host memory, under Miri, if the fake kernel's `map_device`
     can give it real memory. Cover in-bounds, the last byte, one past it (`None`/`false`), and a
     `len` of one page. If the fake cannot, a target case already drives the registers (netd's,
     blkd's), so name it as the keeper and say so.
   - **`premapped`:** target only. Its keepers are the boot cases (`rt-build`'s servers,
     `bundle-mapped`, and init's boot). Name them.
   - **`GlobalAlloc`:** covered by the heap file under Miri.
   - Every SAFETY comment states the invariant and who guarantees it: the kernel (`map_device`,
     `map_anon`), the loader, the parent, the owner type (`Mapping`, `Locked`).

## Not done, and why

- **No per-page bitmap for the free lists.** Links in freed blocks are the standard design, and
  they are checked under Miri. A side table would need its own sizing and storage, a second
  structure to keep consistent, and more code, for no gain in safety. Simplicity wins.
- **`Mapping`'s two views stay two.**
  - An `unsafe fn slice_at` called from both views counts three, not two: the fn and two call
    sites.
  - A private safe fn cannot make a mutable view from a shared borrow soundly.
  - The views are one invariant in two borrows, already written once on `Mapping` and attacked by
    `mapping_views`.
- **The MMIO pair stays a pair.** A read and a write are two operations.
- **None of these removals costs clarity.**
  - The compile-time hook removes a registration race and a once-only flag.
  - The two folds state a shared invariant once, in the pattern `push` already uses.

## Owned paths

- `libs/rt/src` and `libs/rt/tests`, and `servers/netd` (the hook's registration and its test).
- Every `entry!` user, for the macro form only.
- `tests/rt-miri.toml`, and `tests/unsafe-budget.toml` (rt's row).
- Pages: native.md "`redoubt-rt`, the native runtime", and testbench.md "The unsafe budget" (the
  `rt-miri` paragraph).

Hotspots: K16 owns `libs/rt/fake`. Ask before you change it, for example if `Registers` needs a
fake `map_device` that gives real memory.

## Pages

- native.md "Start and end": add after "exits with `PANIC`;"
  > a program that names a panic hook in `entry!` has it run first, once per process, before the
  > report (netd stops its device there).
- native.md, a new last bullet of the runtime section:
  > - **Its `unsafe` is few and attacked.** The runtime holds N uses (`unsafe-budget.toml`): the
  >   allocator's trait, the heap's two words of a free block, a page buffer's two views, the
  >   device registers' read and write, and the pages mapped before the first instruction (the
  >   startup page and `init`'s bundle). Each states what it rests on and who guarantees it; the
  >   heap and the views run under Miri in the bench (`rt-miri`).

  Use the actual N.
- testbench.md "The unsafe budget": the `rt-miri` sentence adds the heap's own file and, if
  built, the registers test.
- The rt row's `name` in `unsafe-budget.toml` follows the sites. `max_unsafe` falls to the count
  reached.

## Gates

- `cargo testbench` on both widths.
- `rt-miri` with the new files.
- `cargo fmt --check`.
- The unsafe ratchet at the new count.
- The size budget. rt should shrink; if it grows, say why.
- doccheck clean.

Report each command with its exit code.

## Report

- The count before and after, site by site.
- Which deliverable-1 form landed, and why.
- The keeper for each remaining site.
- What was deleted.
