Merge verdict: OK with notes

RT1 red, round 2, wp-rt1 at a0d3ac04c (main caf6f37b5). Untracked: RT1-report-1.md, RT1-report-2.md.
Unstaged: docs/userland/native.md.

Each guarantor checked against its source:
- Registers (handle.rs:304/318): the kernel's map_device maps len/PAGE_SIZE pages and returns
  len = d.size (kernel/src/device.rs:384-393). A d.size that is not a whole number of pages
  would leave the tail unmapped, but boot_devices asserts base and size are page multiples
  (device.rs:299), so base..base+len is mapped in full. The keeper reads and writes byte 0 and
  the last byte, and checks that len, len+1 and usize::MAX are refused. It runs over a real fake
  page under Miri, so loosening the bound to `>` would be a Miri error, and dropping the check
  overflows at usize::MAX. Its attack holds.
- premapped / bundle: the loader asserts the bundle is page-aligned and at most BUNDLE_MAX, maps
  every page up to the end rounded up to a page, and passes a1 = initrd_range.len()
  (loader/src/main.rs:261, 365-392). The bundle signature is verified before the bundle is
  mapped. The length premapped trusts is the length that was mapped.
- Heap GlobalAlloc / words: tests/heap.rs checks alignment (1/8/64/4096), tags every block and
  re-checks the tags before freeing, so an overlap would fail the test even outside Miri. It
  also checks the fixed arena's run reuse exactly (offsets) and the refusal of align above a
  page. Together with the const assert, this attacks what the comment claims.
- Mapping views: ipc.rs SAFETY names kernel R3/R4 and Mapping's adopt bound; mapping_views is
  in rt-miri.
- FSD1 rebase: servers/fsd/src/bin uses entry!(serve), so it gets the emitted handler, and it
  has no #[panic_handler] of its own.
- `cargo testbench rt-miri` via in-dev: PASS, 89.7 s, exit 0, with registers and heap listed.

Findings:
P2 docs/userland/native.md, unstaged in the worktree. It rewrites the "No safe call pulls
memory from under its owner" bullet to name `Dma` and `Registers` as `unmap`'s other owners,
which matches premapped's SAFETY text. That edit is not in a0d3ac04c, so as committed the page
still says `unmap` is private to "the heap and `Buffer`", contradicting start.rs's SAFETY. The
implementer should commit the edit (lock step) or drop it.
P2 (note) The fake's `anon` map now holds device pages too. The name and doc comment say
"MapAnon", so the comment should say it also holds `device()`'s pages.
