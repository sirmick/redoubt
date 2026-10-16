# B75 report: a spawned thread's stack above an unmapped guard page

Head: wp-B75 54972509a (one commit, rebased onto main 43588e0d6; was f9a1fa206 on 32a448875), worktree .worktrees/B75, clean. Never pushed.

Gates (exact commands under scripts/q, exit codes):
- `cargo test -p redoubt-rt`: every host test green (5 in thread.rs, 2 new). rc 0.
- `cargo test -p testbench --bin testbench memory::`: 14 passed (1 new). rc 0.
- `cargo test -p redoubt-doccheck --test docs`: 2 passed. rc 0.
- `make -f scripts/jobs.mk prebuilt`: rv64 and rv32, 307 cases each, 0 failed (the consumer
  sweep: every bin linking redoubt_rt builds for both widths). rc 0. (A first run failed "tree
  changed" because I ran host tests beside it; rebuilt alone.)
- `make -j4 -f scripts/jobs.mk set CASES="thread-stack-guard beamlet-footprint" WIDTHS=rv64,rv32`: 4 PASS.
- thread-stack-guard twice more both widths: 4 PASS (3/3 each width).
- `set CASES="size-budget unsafe-budget <scripts/shell-cases>"` (57 shell-set cases, both widths):
  113 PASS, 1 FAIL: size-budget, libs/rt 3994 lines over its 3943 ceiling. Raised to 3994 in the
  commit with its `Size budget: libs/rt:` line; rerun after the commit: size-budget PASS,
  unsafe-budget PASS (no new unsafe in libs/rt/src; the fake kernel's two mmap/munmap unsafes are
  host-only, outside the ratchet, with SAFETY lines).
- rustfmt +nightly --check on every changed Rust file: clean.
- Review gate (steward-red-3): the host suites that spawn through rt on the fake kernel, whose
  stacks now come from map_fixed by host mmap, all green under q (logs .tmp/B75/host-*.log):
  `cargo test -p beamlet-redoubt --features fake` (userland/otp): 81 passed, 0 failed (console,
  files, limits, lookup, pack, schedulers 27, system, userland, unit tests), rc 0;
  `cargo test -p redoubt-client`: 43 passed, rc 0; `cargo test -p redoubt-consrelay`: 26 passed,
  rc 0; `cargo test -p redoubt-sshd`: 34 passed, rc 0. Head unchanged (f9a1fa206).
- Not run: the whole bench (the train's); beamlet-compile-footprint does not exist on main (AGT4
  is unmerged), so the helper peak is quoted from beamlet-footprint.

## Regate on the rebased tree (main 43588e0d6, with AGT1-4 and B74)

Conflicts resolved to both truths: docs/testbench.md and docs/userland/beamlet.md (main's new
budget numbers and compile peaks kept, my helper-stack clause folded in), docs/userland/native.md
(the `thread` row: the guard now exists), userland/otp/redoubt/src/lib.rs (SCHEDULER_STACK_PAGES
comment). AGT4's beamlet.md bullet "a helper thread's stack has no guard page ... inferred, not
measured" rewritten: the guard exists, the scan measures spawned stacks, and no case compiles on a
helper (beamlet-compile-footprint runs one scheduler by AGT4's design, so its thread lines are the
platform's three workers; a helper's peak under a compile is not measured, stated on the page).
Gates (logs .tmp/B75/rebased/): prebuilt rv64+rv32 rc 0; thread-stack-guard, beamlet-footprint,
beamlet-compile-footprint PASS both widths; size-budget PASS; docs rc 0; `cargo test -p redoubt-rt`
rc 0, no failures; rustfmt check clean. Scan lines: compile-footprint rv64 `stack beamlet 34280`,
threads 1,576 / 1,000 / 1,176; rv32 25,832, threads 1,424 / 752 / 976. beamlet-footprint rv64:
first thread 34,456, thread 3 (second scheduler) 21,872 and 21,496 over two runs (25,200 before the
rebase), threads 1,2,4 under 1,600; rv32 24,568, thread 3 21,752. The pages quote the rv64 range
21,496-25,200 over three runs.

## What was built

- libs/rt/src/thread.rs: `spawn` places each stack by `map_fixed` in a thread-stack area of the
  runtime's, `STACKS_BASE` 0x7000_0000 up to `STACKS_END` 0x7FF0_0000 (the stub's STARTUP_AT),
  slot after slot (`stack_pages + 1` pages, a bump counter, never given back, as stacks are spent
  for good); the slot's lowest page is never mapped: the guard. Why it stays unmapped under the
  process's own calls: the kernel places nothing in that area (map_anon's area ends at
  0x7000_0000, memory.md "Where map_anon puts pages"); only a map_fixed of the program's own can
  take a slot, and then that spawn is refused (memory.md residual, extended). A refused mapping or
  thread spends its slot. Past the area: OutOfMemory.
- Paint: before the thread starts the runtime paints the stack with `thread_paint(tag, thread,
  index)` (THREAD_PAINT high word; launch tag from the heap record, the spawn's number from 1,
  the unit's index from the top, since the size is the program's). Not painted: spawn 256+,
  tag > 255, stacks > 128 pages.
- libs/rt/src/handle.rs `map_fixed` (pub(crate)), ipc.rs `Buffer::fixed`.
- libs/rt/fake: `MapFixed` by mmap MAP_FIXED_NOREPLACE at the named address; `Unmap` munmaps
  those; `Fake::mappings`; libc dependency (dev-only crate). Both Cargo.locks gain libc.
- tools/testbench/src/memory.rs: the scan reads THREAD_PAINT units at their mirrored page offset
  and prints `stack NAME thread N PEAK` per spawned stack found (the peak: the deepest overwritten
  unit under the deepest found; no margin check, the size unknown). beamlet-footprint now prints
  4 beamlet threads: thread 3 (the second scheduler) 25,200 B rv64 / 21,752 B rv32, threads 1,2,4
  (platform workers) under 1,600 B. SCHEDULER_STACK_PAGES (18) stays; its comment quotes the peak.
- tests/thread-stack-guard.toml + tests/programs/src/bin/thread-stack-guard.rs (tester in init's
  place, heap-cap's shape) and thread-stack-overflow.rs (child on the runtime: a helper on a
  two-page stack recurses without end). Verdicts from the system (rule F): the kernel's line
  `PROGRAM HALT: CPU Exception on PID N: Store page fault of 0x70000ff0 at ...` (rv64; rv32
  0x70000fe0), the address in the first slot's guard page, and the exit notice `Faulted` read by
  the tester; forbid Illegal instruction and other fault kinds.

## Design notes / what the kernel would need

- The body asked for "the kernel's fault line blaming the thread". The release kernel prints the
  PID only (irq.rs; the thread id is a debug-print/kernel-failure diagnosis). Not changed, as
  instructed; the guard's address identifies the slot, and the case expects it.
- A frame larger than a page whose first store skips the one guard page is not caught (stated
  on thread.rs). No stack in the tree has such a frame.

## Pages

docs/kernel/memory-layout.md (Regions row + figure + status, bench:thread-stack-guard),
docs/userland/native.md (`thread` row; rt status +2 host tests +1 bench), docs/testbench.md
(memory-budget scan paragraph; beamlet's helper peaks), docs/userland/beamlet.md (scheduler
stacks: guard costs no page; the peak), docs/kernel/memory.md (residual: map_fixed in the
thread-stack area). Summaries checked, no change needed: README.md, GETTING-STARTED.md (no claim
on thread stacks), docs/plan/m2-usable-shell.md and m3-agents.md (none), docs/servers/init.md
"Stacks" (first-thread stack only; still true), docs/servers/steward.md:491-496 (44 pages outside
the heap and "a thread's stack is never given back": both still true, the guard costs no page),
libs/rt has no README. Large pages were read at their changed passages, not whole.

## Logs

.tmp/B75/run1..run3 (guard case logs x3 both widths, footprint logs), set2.log (the shell set),
prebuilt-make2.log; the footprint scan lines are in run1/*beamlet-footprint.log.
