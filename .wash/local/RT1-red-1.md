Merge verdict: OK with notes

RT1 red, round 1, wp-rt1 at 5b5a6bfe2 (git diff main...HEAD; untracked: RT1-report-1.md only).

Checked, sound, not moved:
- Panic hook: entry!/first_entry! always emit panic_handler!; with no hook the handler passes
  None and run_hook_once is never called (the old "hook==0 -> HOOK_DONE" branch is no longer
  needed). A panic before main still reaches the emitted #[panic_handler]: it is a lang item,
  not something main installs. Two handlers (entry! plus panic_handler!, or a bin's own
  #[panic_handler]) is a duplicate-lang-item build error; none is a link error. Both are loud
  at build time, and rt-build/*-build cover them. The 40-odd tests/programs bins with their own
  #[panic_handler] link no rt (they would already have collided with the old handler). netd:
  arm_panic_reset only stores; panic_reset returns early while base==0; the test calls
  run_panic_hook(panic_reset) just as the emitted handler does.
- Heap words/set_words: every caller hands in a block of at least 16 bytes, aligned to its
  size, or a page-aligned run. pop now reads two words where push used to write one, and push
  writes [head, 0], so both words are initialised. The const assert evaluates on both widths.
  free_pages unmaps (unfixed) without writing; it writes only once the arena is fixed.
- premapped(): it has two callers, and each passes its own bounds (MAX_BLOCK on one aligned
  page; the loader's len). The SAFETY text attributes each bound to the right party.
- Ratchet: grep finds exactly 10 unsafe in libs/rt/src (start 1, ipc 2, heap 4, handle 2,
  plus the lib.rs deny lint, which is not a block).
- `cargo testbench rt-miri` via in-dev: PASS, 88.7 s, exit 0, with heap in the list.

Findings:
P2 (keep, note) libs/rt/fake/src/lib.rs:233. `Fake::device` hides its allocation from Miri the
same way MapAnon did before 688b9f717: alloc_zeroed(...) as usize, kept only as an integer and
never freed. tests/registers.rs is not in rt-miri, so the two volatile unsafe in handle.rs
(Registers read/write, :309 and :318) never run under Miri. Adding the test file as it stands
would hit the leak check. Fix (round 2): store the *mut u8 in Device (or in the anon-style
map) and add registers to tests/rt-miri.toml. Otherwise, state that the device views stay
Miri-unchecked.
P2 (note) fake:Unmap leaves the `anon` entry with a dangling AtomicPtr. This is harmless
because nothing dereferences it, and the doc says "replaced when mapped again", but the doc
could also say "kept after unmap".
