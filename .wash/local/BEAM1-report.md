# BEAM1 report 3 (implementer-2; reports 1 and 2 are BEAM1-report-1.md, -2.md)

Branch wp-beam1, base a8cbbc7ad (new main; was b21a59493), tip 69e335a45, 8 commits:
- 82246f785 rt: thread::spawn (size 2,927->2,949 after RT2)
- f2b78fc4a rt: thread_create keeps the stack of a start it cannot read (FINDING, granted)
- 957554d27 testbench workspace/erlang/otp; c0f6c7760 otp profile; 8089fc12b INIT_PAGES 2,048
- 367501adf beamlet on the machine + beamlet-boot; "beamlet on Redoubt" built here, Open closed here
- 45bd3ec48 console/clock/randomness + beamlet-console
- 69e335a45 limits at budget/16, budget_pages required, heap-flood, budget-flood, todo page

## Cases (cargo testbench <case>, each exit 0; PASS rv64 and rv32)
- beamlet-heap-flood: flooding / the flood ended: killed / type a line / echo: still served
  after the flood / beamlet exited, code 0.
- beamlet-budget-flood: start line / panicked (alloc.rs) / beamlet exited, code 101 / restarted
  beamlet, console M / start line under M. The case ends there: one restart, no reboot.
- beamlet-boot, beamlet-console, init-refuses-bound: PASS both.

## VM's own use (beamlet-console, budget bisected, 128-page steps)
rv64: 1,536 fails, 1,664 passes. rv32: 1,792 fails, 1,920 passes. 4,096 >= twice either.
RAM: 256 MiB default; the cases' servers total 5,888 pages of system's ~15K. image/manifest.json
has no beamlet entry, so only the cases' manifests changed.

## thread_create / spawn
The decode is inside syscall: an unreadable answer arrives as Err(InvalidArgument), and
thread_create's `?` dropped the stack. Now only a refusal drops it; InvalidArgument keeps it
(test asserts the stack mapping; fails without the fix). spawn keeps its closure leak on
InvalidArgument: closure and stack now leak together.

## Gates (exit 0)
docs, rt-host-tests, unsafe-budget (unchanged: beamlet forbids unsafe), size-budget (libs/rt
2,955), vendor-check, cargo test -p testbench (67), cargo test --workspace --features
beamlet-redoubt/fake in userland/otp, cargo +nightly fmt --check (root and otp), git diff --check.
Whole bench: not run (awaiting word).

## Page lines
- Limits status: "built · partly tested: in a boot, only the process heap limit and the budget's
  backstop are attacked · tested (14)" + the two bench lines; the ruled paragraph, rewrapped.
- beamlet on Redoubt: "... · tested: bench:beamlet-boot, bench:beamlet-console,
  bench:beamlet-heap-flood, bench:beamlet-budget-flood".
- budgets.md: "the bound is 1,060 pages on rv64 and 1,286 on rv32 ..., and 2,048 leaves 988 and
  762 to spare" (beamlet grew a page; init prints 1060/1286).

## Departures for the Architect
1. todo page: doccheck C9 requires What/Why it matters/Where/Done when; "The fix" became "Done
   when" (text unchanged) and a "Where" was added.
2. docs/SUMMARY.md gained the todo's entry (not an owned path; doccheck's index).
3. The Open (counter frequency) closes in 367501adf (doccheck refuses Open in a built section);
   the console commit adds "(bench:beamlet-console)".
4. Host test of run() loads vm/tests/fixtures/limits.beam (read, not changed).
5. budget_pages also refuses 0, a sign, and a repeat.

## Round 2 (after the simplifier, editor, red round 2 and the Architect)
Tip 5ecef32e9, base a8cbbc7ad, 9 commits (new: e5bbef74a testbench `distinct`).
Whole bench: cargo testbench --allow-skip, exit 0, 359 PASS, 0 FAIL, 1 SKIP
(bench-ssh-loopback-openssh: podman not installed); log /tmp/beam1-bench-2.log.
STACK_PAGES 4: the reader's depth when made to panic at its read's system call is 5,600 B
(rv64) and 4,720 B (rv32). Bound pinned in beamlet-boot: 1059 (rv64) / 1286 (rv32).
f2b78fc4a not folded: thread_create's drop predates BEAM1 (532327375). `say` takes the
console, since a second open of the console would exceed consoled's fids per session.
