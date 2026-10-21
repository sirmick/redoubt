# B105 report

Base 4750dd61c; branch wp-B105, head 1aeb664f9 (two commits: fbdcad2e7 beamlet with its tests
and cases, 1aeb664f9 docs).
Worktree .worktrees/B105. Tier B (no platform or allocator contract changed: the rule is held with
Rust's fallible `try_reserve_exact` against the existing GlobalAlloc null-on-refusal contract).

## What failed in B100's line

Not the binary (65,534 bytes). The host repro (`Code.eval_string` of the same comprehension under
the host `beamlet`, gdb on `__rust_realloc` over 1 MB): 284 large reallocations, every one the
process heap's cell vector (`Heap.terms: Vec<Term>`) growing by `Vec` doubling in
`push_object`/`cons` (none in the collector), 1.0-3.6 MB, the 3.27 MB failure among them. The
process counts cells in use, not capacity, so a heap of ~1.6 MB under the 2.85 MB sixteenth asked
for 3.27 MB while holding 1.6 MB, and `Vec`'s infallible growth panicked.

rv64 vs rv32: cells are 16 bytes on both widths and the limits the same bytes, so both ask for the
same; the difference is the pages left, which rv64's larger VM footprint leaves fewer of (not
measured separately; the rv64 boot below shows the VM short of pages for this line).

## Paths a process drives, and what each does now

| Path | Where | Now |
| --- | --- | --- |
| Heap growth between instructions | `Process::maybe_collect` | keeps 64 cells free at each safe point, grown fallibly (`Heap::try_room`); refused: collect, retry, else killed |
| Heap growth inside an instruction not asking | `Heap::grow` (push_object, cons) | fallible steps, then exact (the backstop); the next safe point collects or kills |
| A collection's copy | `Heap::collect`, `Collector::evacuate` | first part fallible (heap untouched); growth fallible, refused halfway: process wiped and killed |
| Binary construction / growth | `bs_create_bin` (`bits_room`, private_append) | ask: limit + fallible reserve; Collect = yield, collect, rerun; refused again = killed |
| Natives sized by the process | make_tuple/2,3, list_to_tuple, setelement, append_element, insert_element, tuple_to_list, `++`, `--`, lists:reverse/2, binary_to_list/1,3, bitstring_to_list, binary:bin_to_list, binary:copy, list_to_binary, iolist_to_binary, list_to_bitstring (iolist_size too) | `Ctx::room` / `Ctx::buffer` / `Ctx::grow_bits` before building; temporaries removed where they were the size of the result |

Growth asks for 2x, then +1/2, +1/4, +1/8, never past the process's limit and never less than an
eighth more (`memory::growths`): a first version that fell back to the exact amount regrew a
1.6 MB heap 64 cells at a time when pages were short (10,818 reallocations, 84 s over the line on
rv64, timing out the case twice; found with a scratch-only counter in the session's output).

Kill = `Stop::Exit(exit(killed))`, the same as the slice-end `max_heap_size` kill. Collect-and-rerun
uses the existing `Process::retry`/`Resume` for natives; BIF instructions now honour a retry too
(they did not before), and `bs_create_bin` rewinds its pc.

Stated backstop (beamlet.md): a native's own allocation (zlib, crypto, term_to_binary's encoding,
bignum arithmetic within 2^24 bits), a native not named above building more than the headroom, a
message's copy into its receiver, several processes at once.

## Cases and gates (exit codes)

- Host, `q run --cores 4 -- cargo test -p beamlet-vm --release`: all pass, 12 new:
  `tests/allocation.rs` (11, fixture `tests/src/allocation.erl`) and
  `memory::tests::a_growth_is_never_a_sliver`. Each of the 11 aborts the test binary on base
  ("memory allocation of N bytes failed", SIGABRT), run one by one in a scratch worktree at
  4750dd61c.
- `make -f scripts/jobs.mk set CASES=...` (exit 0): docs PASS, formatting PASS, beamlet-lookup-host
  PASS, beamlet-lookup-cli-host PASS, rt-host-tests PASS.
- `./test-shell` (q, 8 cores): exit 0, every stage passed.
- `tools/difftest erlang atomvm` (q, 8 cores): 511/512; the one, atomvm/hello_world, was a BEAM
  oracle result cached as TIMEOUT under load; with that oracle entry removed, atomvm 460/460, exit 0.
- clippy -p beamlet-vm --all-targets: clean.
- Boot (from scratch worktree .tmp/B105/run at the tested commit 3721e5cee, which differs from head
  only by one doc comment): beamlet-heap-allocation rv64 PASS 15.6 s, rv32 PASS 15.0 s;
  beamlet-compile-flood rv64 PASS, rv32 PASS. Earlier rounds: rv64 heap-allocation failed twice
  (the sliver growth above), passed once on the 64-cell build.
  On rv64 the comprehension answers `{:built, 65534}` (~12 s), the 3 MB binary is killed, the next
  line answers. beamlet-compile-flood: `{:flood, :killed}`, then `{:after, {:compiled, 42}}`, no
  init exit or restart, both widths.
- Memory set at 135fdc7a2 (orchestrator's go): beamlet-footprint, -compile-footprint,
  -agent-footprint, -heap-flood PASS both widths. beamlet-budget-flood FAILED both widths by this
  package's rule: its backstop allocation, binary:copy of 32 MiB, is now checked, so the process
  was killed, the VM exited 0 and init rebooted after 5 restarts. The module now reaches the budget
  through term_to_binary's own buffer (a list sharing one 512 KiB binary 64 times, 32 MiB
  written; a stated-backstop path); rerun at fbdcad2e7: PASS both widths (exit 101, restart).
- docs, formatting at head 1aeb664f9: PASS.

Figures (bench and the footprint report, against the page): heap peak after one command 5,541 /
5,366 pages (page 5,542 / 5,362); after a compile 9,130 / 8,799 (page 9,135 / 8,798); runtime heap
at the prompt held 4,191 / 4,017 (page 4,196 / 4,023), peak 4,635 / 4,463 (page 4,601 / 4,431,
+34 / +32, about 0.7%); accounted 3,966 / 3,898 (page 3,968 / 3,901). The page's table is left as
is: the bench's peaks are unchanged within 5 pages.

Host timing: B100's line evaluated on the host CLI, 0.60 s base vs ~0.62 s (pinned core, 7 runs).

## Pages

- docs/userland/beamlet.md, "Limits inside one VM": status (tested 55, the 12 tests and the case),
  a new "One allocation" bullet, the backstop paragraph narrowed to what remains, "several
  flooding at once, or one of the allocations caught afterwards"; the compile residual rewritten:
  one flood after a compile now kills its process alone; residual is several at once / unchecked
  growth, same follow-up.
- docs/userland/shell.md, "Take the session's memory on the heap": a large binary is no longer
  "bounded only by the session's page limit"; one allocation is checked, several together are.

Summaries checked, no change needed: docs/plan/m2-usable-shell.md line 35 ("a runaway allocation
... is ended by that process's heap limit, and the session's processes together by its budget":
still true, now for one allocation past free pages too); docs/userland/shell.md line 475 (a line
larger than the heap limit ends the session: the group process's limit, unchanged);
README.md, GETTING-STARTED.md, userland/otp/README.md, userland/otp/DESIGN.md (no claim on
allocation failure or these limits); SECURITY.md (no beamlet limits row).

## Open

- The runtime heap's peak at the prompt (the VM's own report) reads 32-34 pages above the page's
  table; one run each, not established as this change's or run-to-run.
- The rule changed beamlet-compile-flood's outcome (flagged at the checkpoint).

## Round 1 (servers-red, wording)

Head af712aa86 (c7c150b4b beamlet with tests and cases, af712aa86 docs).

- P1: shell.md and beamlet-heap-allocation's description now claim only the allocations
  beamlet.md names as checked; beamlet.md's backstop paragraph names the unconverted natives
  (maps:to_list/keys/values/from_list, ETS lookup/tab2list/match copies,
  unicode:characters_to_list, binary_to_term, get_keys/erase, integer_to_list of a large bignum,
  natives' held list copies), their last refusal the allocator's.
- P2: beamlet-heap-allocation on base 4750dd61c (scratch worktree, case file only): FAIL both
  widths. rv64: the VM panics at the comprehension, "memory allocation of 3576384 bytes failed"
  (step 4, forbidden 'panicked'). rv32: the comprehension answers, then the 3,000,000-byte binary
  is answered {:big, 3000000} instead of killed (step 7 timed out). Logs .tmp/B105/logs-base/.
  The case and the page say so.
- P2: budget-flood's module comment and description say the case must move when
  term_to_binary's buffer is checked.
- P3: history words removed (beamlet.md, memory.rs, the case).
- Gates at af712aa86: docs PASS, formatting PASS (exit 0); beamlet-heap-allocation PASS rv64
  15.2 s, rv32 14.3 s; beamlet-budget-flood PASS both widths (exit 0).
