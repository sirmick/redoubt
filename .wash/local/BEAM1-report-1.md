# BEAM1 report 1 (revised after the rulings): beamlet-boot on rv64, and the rv32 build

## Revision (rulings applied)
Branch now: 408db27f4 rt; e2422a27c testbench; ed2732ed9 userland/otp profile (debug = false,
strip = true; no opt s); 7892d0097 kernel INIT_PAGES 2,048 + budgets.md; e4cc30635 beamlet.
- beamlet ELF rv64 stripped: 2,888,296 B. init's bound for beamlet-boot: 1,059 pages (in the page).
- VM pages: 1,408 fails, 1,536 passes; budget 3,072.
- rv32: 0 errors, 3,816,396 B stripped (932 pages); bound fits under 2,048. Not booted.
- budgets.md: the ruled text says "(`init-boot` prints it)"; init-boot has no beamlet, so I wrote
  `beamlet-boot`. Otherwise as ruled; the list item before the new one ends ';' not '.'.
- BLOCKING: init-refuses-bound no longer refuses at 2,048 (bound 1,057); question sent (A: a
  `{ zeros = N }` file form, a ~6 MB program entry in bound.json).
- beamlet-boot PASS again on the committed tree.

(The text below is the first version, before the rulings; its sizes for opt "s" are superseded.)


Branch wp-beam1, base 7fbe59773, 3 commits:
- 408db27f4 rt: thread::spawn (libs/rt/src/thread.rs, one mod line, libs/rt/tests/thread.rs,
  native.md's table row and 2 host tests in its status, the unsafe and size budget lines)
- e2422a27c testbench: `workspace` on a package program; `{ erlang = ... }` and `{ otp = ... }`
  file forms (case.rs, build.rs, testbench.md's example)
- 9b041e599 beamlet: the `beamlet` bin, `run` moved into the lib, machine Threads/Modules,
  tests/beamlet-boot.toml, tests/data/beamlet/boot.json, the test module

## beamlet-boot (rv64): PASS
    init: started beamlet, console 16cdcc46110d37eb
    init: 2 public entries pushed to bootfsd, and sealed
    [con 16cdcc46110d37eb] beamlet-boot: hello from the VM
    [con 16cdcc46110d37eb] ok
    init: beamlet (PID 14) exited, code 0
(init restarts it after, as it does any exited program; the case ends at the exit line.)
Modules needed: the test module and OTP's io.beam, nothing else.

## Size and pages
- beamlet ELF rv64imac: 86 MB with debug info (userland/otp's release had debug = true);
  5.08 MB without; 2.89 MB stripped; 2.43 MB stripped at opt-level "s" (committed, pending ruling).
- init's bound for this boot: 947 pages of root's 1,023 (INIT_PAGES); root held 292 after.
- The VM's budget, by bisection: 1,152 pages fails (restart loop), 1,280 passes. Budget 2,560.

## Unsafe
redoubt-rt 10 -> 11: the one Box::from_raw, in a private `take` used by the trampoline and by
spawn's refused path, so a refused closure is dropped rather than leaked. 0 undocumented.
Size: libs/rt 2,920 -> 2,938 (it was at its ceiling).

## rv32
`cargo build --release --target riscv32imac-unknown-none-elf -p beamlet-redoubt --bin beamlet`
in userland/otp: 0 errors, 3.27 MB (stripped, opt "s"): 799 pages, over INIT_PAGES as well.
Not booted.

## Gates run (exit 0 unless said)
beamlet-boot PASS; rt-host-tests PASS; all *host-tests PASS (filter host-tests);
unsafe-budget PASS; size-budget PASS; no-cruft PASS; vendor-check PASS; docs PASS;
cargo +nightly fmt --all --check, root and userland/otp: 0;
cargo test -p beamlet-redoubt --features fake (userland/otp): 0, 7 passed.

## Open
- Q (blocking for the merge): INIT_PAGES vs beamlet's image (sent; A/B/C).
- Q: userland/otp's profile (sent; folded into the above).
- libs/rt/tests/thread.rs is outside the owned paths (in-crate tests cannot use the fake kernel).
- The fake kernel panics on thread_exit; the host test's closure parks instead.
- testbench.md line, as written:
  `{ package = "beamlet-redoubt", bin = "beamlet", workspace = "userland/otp" },  # a binary of a workspace of its own, built there`
  plus two lines for `{ erlang = ... }` and `{ otp = "io" }`.

## Update 2
- beamlet-boot passes on rv32 too (bound 1,285 pages on rv32); case is arch rv64+rv32 (56dadec00).
- beamlet-console PASS rv64 and rv32 (398f33711). Bug fixed with it: the reader packed 8 bytes per
  message word; the kernel refuses words over 32 bits on rv32 (eof at once). Now 4 bytes/word.
  Modules: io, io_lib, io_lib_format, unicode, crypto. Pages: rv64 1,536-1,792, rv32 1,792-2,048;
  budget 4,096.
- heap-flood draft (uncommitted): ends with "memory allocation of N bytes failed" (Rust alloc
  panic) then "exited, code 101", both widths: FINDING. Asked: second-program design (A: the
  restart) and whether run() should set max_heap_words from the budget.
- Fake gap: the fake kernel panics on thread_exit (not modelled).
