# BEAM1 handoff (worktree .worktrees/beam1, branch wp-beam1, base 7fbe59773)

Tip e533f695c. Final: 9b9f7c55b rt thread::spawn; 0e0588886 testbench workspace/erlang/otp;
5aea6a510 otp profile (debug=false, strip; cfg-if comment); 545325232 INIT_PAGES 2,048 +
budgets.md + init-refuses-bound via `{ zeros = 6291456 }` (expects 2521/2047, both widths);
e42ccd9e9 beamlet + beamlet-boot; 50f4c09b4 beamlet-console + rv32 word fix (4 B/word),
STACK_PAGES 2. WIP e533f695c: limits(budget_pages) in lib.rs (half the budget, as ruled),
`budget_pages=N` arg in beamlet.rs (servers hold no budget handle: manifest argument),
tests/limits.rs, beamlet-heap-flood reshaped to 2a, beamlet.md status line edit. Fold it.

## Cases
boot, console: PASS rv64+rv32. init-refuses-bound, init-boot, init-servers: PASS both.
heap-flood (2a, WIP): fails 101 at the ruled 1/2; see the table. Question e68f67b2.
budget-flood (2b, a native's big binary:copy; restart = 2nd program): NOT built.

## Heap-limit fraction (measured at budget 3,072): 1/2-1/6 exit 101 both widths; 1/8, 1/12 pass
rv64 only; 1/16, 1/24 pass both. Tree is at 1/2 (the WIP's).

## Next
0. RULED (BEAM1-heap-limit-ruling.md, supersedes part 1 of the flood ruling): both limits =
   budget/16 in words; beamlet's budget 4,096 pages in the image's and the cases' manifests;
   budget_pages=N REQUIRED (exit BAD_ARGS if missing or malformed); init does not touch it; add
   docs/todo/beamlet-budget-from-startup.md (text in the ruling file). Report per width.
1. 2a passes both widths; budget-flood per BEAM1-heap-flood-ruling.md;
   page lines in the ruling file ("Limits inside one VM" status tested (14), the sentence;
   the "beamlet on Redoubt" status lists both case names; its fraction must match the code).
2. handle.rs GRANTED: own commit, `stack.into_pages()` before the decode (finding). Then spawn
   may keep its InvalidArgument leak (stack kept too) or free: keep the leak, say so.
3. Rebase onto main b21a59493 (RT2 moved server/, start.rs exit module).
4. Whole bench, both widths, on word; report memory_mib cases that fail.

## Red round 1: notes 1-6 all done in the owning commits (detail in BEAM1-report.md)

## For the Architect at merge
budgets.md: "1,059 pages on rv64 and 1,285 on rv32 (`beamlet-boot` prints it), and 2,048 leaves
989 and 763 to spare" (ruled "twice either" false). beamlet.md Open (counter frequency) became
"not needed: time_now's microseconds serve the clock and idle's deadlines (bench:beamlet-console)"
(doccheck refuses Open in built). Fake gaps: thread_exit unmodelled; thread_create's
provenance-free transmute (Miri UB), thread not on rt-miri: add a comment in rt-miri's list.
Budgets: rt unsafe 11, size 2,942.

## Traps
5 restarts in 60 s reboots. A missing OTP module shows as ???. A budget rename is a drop for
the ratchet. Fold with amend!/fixup! + GIT_SEQUENCE_EDITOR=: git rebase -i --autosquash.

## What consumed my context
Budget bisections (bench runs), reading build.rs/case.rs whole, rewrites.
