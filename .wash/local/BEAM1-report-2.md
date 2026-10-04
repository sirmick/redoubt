# BEAM1 report 2 (report 1 is BEAM1-report-1.md)

Branch wp-beam1, base 7fbe59773, 8 commits:
- 9b9f7c55b rt: thread::spawn (thread.rs, mod line, tests/thread.rs, unsafe 10->11, size 2,920->2,942)
- 0e0588886 testbench: `workspace` on a package program; `{ erlang }`, `{ otp }` file forms
- 5aea6a510 userland/otp: release profile debug = false, strip = true; the cfg-if comment
- 545325232 kernel: INIT_PAGES 2,048 + budgets.md; init-refuses-bound via `{ zeros = N }`
- e42ccd9e9 beamlet on the machine + beamlet-boot (rv64, rv32)
- 50f4c09b4 console, clock, randomness + beamlet-console (rv64, rv32); the rv32 word fix
- 8383dcdfd beamlet-heap-flood, shape A (the restart is the program started after the end)
- 4a80617c0 DROPPABLE: `max_heap_words=N` argument, flood killed in Erlang (pending the ruling)

## Cases (all PASS, both widths)
- beamlet-boot: started beamlet / `[con N] beamlet-boot: hello from the VM` / `[con N] ok` /
  `beamlet (PID n) exited, code 0`. Bound 1,059 (rv64), 1,285 (rv32).
- beamlet-console: prompt; `echo: typed into beamlet 42`; `slept: 2019xx us`; `random: <64 hex>`;
  exit 0. Modules: io, io_lib, io_lib_format, unicode, crypto.
- beamlet-heap-flood, at 4a80617c0: `flooding` / `{'EXCEPTION',exit,killed}` / `exited, code 0` /
  `restarted beamlet` / `flooding`. At 8383dcdfd (without the limit): `panicked ... memory
  allocation of 2056128 bytes failed` (4112256 rv32) / `exited, code 101` / restart / `flooding`.
- init-refuses-bound: `would cost init 2521 pages of root and root keeps 2047`, both widths.

## Pages used by one VM (by budget bisection)
boot: rv64 1,408 fails, 1,536 passes; budget 3,072. console: rv64 1,536-1,792, rv32 1,792-2,048;
budget 4,096. Binary: rv64 2,888,296 B, rv32 3,816,396 B (stripped).

## Red round 1
1. Which case exercises what: beamlet-boot runs no thread (no input is asked for). beamlet-console
   is the first to run spawn, the trampoline and the reader's stack on the machine. No case runs
   thread_exit: the reader waits on the console until the process exits.
2. spawn's doc: no guard page, and a panic ends the process. The reader reached 2,832 B (rv64)
   and 2,240 B (rv32), measured once by filling its stack with a pattern; STACK_PAGES = 2.
   No guard page: the kernel refuses a set_flags with no permission, and a read-only page would
   need the fake kernel to model set_flags (it panics on it).
3. Overclaim fixed: Err(InvalidArgument) (an unreadable start; regs.rs maps a bad tid to it) is
   treated as maybe-started, and the closure leaks (test an_unreadable_start_leaves_its_closure_alone).
   FINDING, not mine: handle.rs thread_create drops the stack Buffer on that same path, so the
   stack would be unmapped under a thread that may run. The fix is `stack.into_pages()` before
   the decode (libs/rt/src/handle.rs). Ask before I touch it.
4. otp_module matches code:which; a missing module fails "the pinned OTP has no module X";
   ERL_CRASH_DUMP=/dev/null; no erl_crash.dump.
5. cfg-if: comment fixed (macros only, left to Cargo.lock as the root does; the root does not
   vendor it either).
6. beamlet prints `beamlet: /boot/X.beam never showed: Rerror` (or no module/bootfsd) before exit 2.

## Pages
- budgets.md: "the bound is 1,059 pages on rv64 and 1,285 on rv32 (`beamlet-boot` prints it), and
  2,048 leaves 989 and 763 to spare." The ruled "above twice either" was false (2 x 1,285 = 2,570;
  even 2 x 1,059 = 2,118 > 2,048).
- beamlet.md status: "built · partly tested: files, programs, `/net` and the natives are not
  built, and the modules are read from `/boot` unchecked · tested: bench:beamlet-boot,
  bench:beamlet-console, bench:beamlet-heap-flood" (both widths, so no "rv64 only").
- beamlet.md paragraph, console-status clause, "On the machine it is the kernel's own.": as ruled.
- The Open (counter frequency): doccheck refuses an Open in a built section, so it became
  "The timer's counter frequency is not needed: `time_now`'s microseconds serve the clock and
  `idle`'s deadlines (bench:beamlet-console)." For the Architect to confirm.
- native.md row as ruled; testbench.md example lines for workspace, erlang, otp, zeros.

## Fake kernel gaps
thread_exit not modelled (panics); thread_create transmutes the usize entry (no provenance:
Miri UB), so thread is not on rt-miri; set_flags not modelled.

## Gates (exit 0)
the 3 beamlet cases + init-refuses-bound, init-boot, init-servers on rv64 and rv32;
unsafe-budget, size-budget, no-cruft, vendor-check, docs, rt-host-tests, init-host-tests;
cargo test -p testbench (67); fmt root and userland/otp; userland/otp
`cargo test --workspace --features beamlet-redoubt/fake`. Whole bench: not run (awaiting word).
