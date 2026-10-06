# B16 report: the memory scan counts a stack page from its maximal physical page

Branch wp-B16 from main 051a2f86c; one commit 3a3fcc71f (two files, staged by path). Not pushed.

## Delivered

- `tools/testbench/src/memory.rs`: `scan` keeps, per server and stack page k (= unit / 512), a
  bitmap of the units found at their in-page offsets in the physical page being scanned; at each
  physical page's end the page replaces the best so far when it holds more of k's units, or marks
  k tied when it holds equally many. Only the best page's units count toward the lowest missing
  unit. A tied stack page fails the scan as `NAME: stack page K's paint found twice, N units in
  each of two pages` (an error, so the dump is kept, as for the old duplicate). The out-of-range
  index check and the offset filter are unchanged.
- Tests: `scanner_ignores_a_single_paint_word_copied_into_another_page` (EROFS1's shape: unit 0
  copied alone into two pages, a touched unit copied into a third; peak unchanged at 1600);
  `scanner_ignores_a_copied_buffer_of_paint_words` (a 200-word buffer of since-touched units and a
  50-word one; a union of copies would report 992, the rule keeps 1600: the lowest missing unit
  unchanged by copies); `scanner_refuses_missing_duplicate_and_too_little_margin` now refuses an
  intact whole-page copy with the exact message.
- `docs/testbench.md`, The memory budget: the ruling's sentence verbatim after "...at an encoded
  offset."; the rest of the paragraph reflowed, no wording changed.

## One existing assertion changed (flagged)

The assignment says the existing tests stay unchanged, but the old duplicate assertion in
`scanner_refuses_missing_duplicate_and_too_little_margin` was one extra `stack_paint(1, 0)` word at
its offset in a second page: exactly the single copied word the binding ruling now ignores. It
cannot pass under the ruling, so that one case became the intact-page-copy refusal (the ruling's
"an intact page copy refused"). Every other existing test is untouched and passes.

## Regression input

EROFS1's kept dump (1 GiB, not reducible to a committed fixture without its other servers' pages
and records) was scanned with a temporary, uncommitted test using EROFS1's worktree manifest
(tests/userland-boot.toml at 80c6daca5): no error; every stack within twice its peak (beamlet 27192
of 17 pages). Its only failure, `beamlet: heap record capped at 24558 pages, declared 20846`, is a
manifest/dump mismatch from EROFS1's branch, not the scan. Committed tests are synthetic.

## Gates (all through the jobserver)

- `jobserver bounded cargo testbench memory-host-tests`: exit 0 (12 scanner tests pass).
- `cargo testbench formatting` exit 0 (after rustfmt), `docs` exit 0, `size-budget` exit 0,
  `no-cruft` exit 0.
- Boot cases, `make -f .wash/local/jobs.mk -C <worktree> rv64/init-boot rv32/init-boot
  rv64/userland-boot rv32/userland-boot rv64/userland-read-only rv32/userland-read-only`: exit 0,
  all six PASS (init-boot 10.6/9.9 s, userland-boot 143.4/138.2 s, userland-read-only
  157.3/159.5 s).
- memory-host-tests rerun on the final source: exit 0; `git diff --check` clean.
- Not run: the whole bench (forbidden), other cases.

## Documentation check

docs/testbench.md (owning page) updated. Searched README.md, GETTING-STARTED.md, docs/,
tools/testbench/src for "found twice", "duplicate unit", "stack paint", "paint word": only the
memory-budget paragraph claims the rule. `tools/testbench/src/qemu.rs` measure_stacks' comment
("a scan that fails, as on a duplicate or out-of-range unit, keeps it") stays true: no change.
The status line's tests (bench cases, memory-host-tests) exist; no change.

## Open risks (for the Architect, not acted on)

The ruling's "never undercounts" holds when a copy is a subset of the real page's current words.
A stale copy taken while the stack was shallower can hold units since touched: if it holds more of
stack page k's units than the real page still does (the boundary page nearly all touched), the copy
wins and the peak is under-reported; if equally many (e.g. one untouched unit left and one stale
copy of it), the scan refuses a duplicate. Neither occurs in the EROFS1 dump.
