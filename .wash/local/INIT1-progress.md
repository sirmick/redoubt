# INIT1 progress (init1-implementer)

Branch wp-init1, worktree .worktrees/init1, base main a9eed8300.

## Deliverable 1: device_info (early checkpoint)

Commits:
- 50f71a55e kernel, redoubt-sys: device_info says which device a handle names
  (libs/sys call/ret/error/lib/tests, kernel device.rs + redoubt.rs arm, abi.md rows,
  size budget kernel 7723->7734 and libs/sys 966->1006 with reasons in the message)
- a678eec1d tests: device-info-attack checks device_info against QEMU virt
  (program, toml, Cargo bin, rd::device_info)

Uncommitted, held for IPC2's merge (the commit touching model/src/kernel.rs goes last, rebased
onto IPC2): model syscall/trace/gen/mutation/ghost/invariants/kernel arm,
model/tests/device_info_contracts.rs, devices.md (device_info planned->built, map_device's
"says nothing" sentence, Open line gone), model.md R18 row. It needs `Size budget: model`
10092->10152 (+60) in that commit.

### Result encoding
a0 0; a1 kind tag (1 MMIO, 2 IRQ, 3 Reset); a2-a3 `a` (MMIO base / IRQ number, u64 lo/hi);
a4-a5 `b` (MMIO size in bytes); a6 flags (bit 0 DMA); a7 0. redoubt-sys `Return::Device(DeviceInfo
{ Mmio { base, size, dma } | Irq(u32) | Reset })`; the decoder refuses unknown kind, any MMIO
flag but DMA, a non-zero unused field, an IRQ number over 32 bits, non-zero a7.

### Case lines (rv64; rv32 identical)
    [device-info] ok: handle 4 is the Reset right
    [device-info] ok: the console is virt's UART: Ok(Mmio { base: 10000000, size: 1000, dma: false })
    [device-info] ok: the console's interrupt is virt's: Ok(Irq(a))
    [device-info] ok: 13 other MMIO regions (8 with DMA), then 9 interrupts ascending
    [device-info] ok: a budget -> WrongObject
    [device-info] ok: an endpoint -> WrongObject
    [device-info] ok: a closed handle -> BadHandle
    [device-info] ok: an index it does not hold -> BadHandle
    [device-info] ok: all ones in the handle register -> BadHandle, and nothing else in the registers
    [device-info] ok: device_info maps nothing and charges nothing
    [device-info] DEVICE INFO ATTACK TEST PASSED   (then power-off through Reset)
(The first two values print in hex since the last run: {:x?}.)
Rule F: the verdict is the first program's, which owns the console and holds Reset; the values
come from the kernel and are compared with QEMU virt's fixed layout.

### Mutation
DeviceInfoWrongKind (R18): an IRQ reported as kind 1. Caught by kernel_sequence seed 76 via the
new check `device_info_answer` (invariants.rs), fed by Flow::DeviceInfo (ghost.rs).
R18DeviceByNumber still caught (seed 10).

### Gates (all via .wash/local/in-dev)
- cargo testbench (whole, both widths): exit 1 with 3 FAIL:
  bench-ssh-loopback-openssh (podman missing on this host); size-budget (kernel over, since
  raised and committed; libs/sys raised too); uaf-lent-page rv64 "holder never received the
  lend (test setup)" under load from other agents' benches -- rerun 3x alone: PASS 3/3.
- cargo testbench device-info-attack: exit 0, rv64 + rv32.
- cargo testbench size-budget after commits: kernel and libs/sys pass; fails only on the
  uncommitted model (+60), which its commit raises.
- every *-build case rv32 + rv64 PASS; unsafe-budget PASS; no unsafe added (max_unsafe 12,
  max_undocumented 0 unchanged).
- cargo +nightly fmt --all --check: clean (after fmt).
- cargo run -q -p redoubt-doccheck: exit 0 (working tree, and an export of HEAD).
- redoubt-model: device_info_contracts 3/3; mutations (DeviceInfo,R18) 2/2 caught.

## Design note for the Architect
devices.md "Which process gets which device" (planned, INIT2's) still says "The kernel still
says nothing about which device a handle names". Now that device_info is built that reads as a
contradiction; I left it (not mine to rewrite).

## Deliverable 2 (init1-implementer-2), in progress

Rebased onto main 4ace1722d (rulings 8074ecf59 pages A, BUNDLE_AT row, 4ace1722d grants C).
Working tree (uncommitted): loader loads kernel + 2nd entry (PID 2), maps the initrd RO at
0x2000_0000 (owner PID 2), a0/a1 via InitialProcess.a0/a1 -> ProcessState::Setup -> setup_loader_process;
IniE gone; kernel boot_budgets: init in root, root keeps init frames + 1 thread + INIT_PAGES 1024,
system 1/4 of rest (15 procs), users rest (47); boot/log endpoints gone; grants assert gone and
loader-rejects-grants deleted; loader-rejects-{kernel-address,kernel-entry,truncated-elf} pack the
corrupt image alone (second entry). rd::log_rx -> rd::devices_end (first_free; nothing after the
devices); logsrv::start creates its log endpoint. New case bundle-mapped PASS rv64+rv32.
Pages: boot.md, budgets.md (Root, system and users), memory-layout.md (Regions row), testbench.md
Bundle files, devices.md R18, SECURITY.md R18.

### Whole bench on the D2 tree (before rebase): exit 1, 169 PASS, 100 FAIL, 1 SKIP
- Multi-program cases (wait for D3: the tester starts them): all-together, bench-attack-forgery,
  bench-poweroff-missing, bench-reporter-mismatch, budget-carve/destroy/forge/syscall/table-attack,
  ipc, irq-attack, kernel-half-attack, legacy-gone, lend-untouched-page, lender-touches-lent,
  mem-attack, move-borrowed-page, ninep-newconn-discard, redoubt-ipc(-attack), return-lent-unmapped,
  rng, rustsbi-boot, syscall-attack, touch-beyond-ram, uaf-lent-page, budget-destroy-kills.
- D4: bench-bundle-file.
- Fixed since: loader-rejects-grants (deleted), loader-rejects-{kernel-address,kernel-entry,truncated-elf}.
- Gates: formatting (not yet run), size-budget (loader 855 > 841, needs a raise with reason).
- Single-program cases whose first program now runs in root (INIT_PAGES) instead of system:
  - usage checks on rd::SYSTEM meaning "my own budget" (charges now land in root):
    budget (budget-test), device (device-test), dma-destroy-quarantine, ipc-outcomes,
    page-table-reclaim, process-attack (proc-attack), process-lifecycle (proc-lifecycle);
  - need more than INIT_PAGES of their own: map-fixed-attack, map-fixed-tables(-rv32) (fill page
    tables until short), map-anon-search-bound (whole 256 MiB area), pages-exhaustion;
    process (proc-test: no output after KMAIN, not yet diagnosed);
  - behaviour tied to the budget's size/class: redoubt-dead (a lend the receiver cannot pay for
    is now payable), redoubt-revoke (mint with ROOT stamp now permitted), redoubt-tight,
    dma-reset-quarantine (child faults at 0), sched-latency (target missed; N=16 driver wake).

## Handoff from init1-implementer-2 (state at f73d9ef42 / 6cb07ee56)

### Branch wp-init1 (base main 4ace1722d), oldest first
- b529e547f kernel, redoubt-sys: device_info (D1, accepted)
- 939ba3f77 tests: device-info-attack (D1, accepted)
- 5ac9f1851 WIP model work (device_info in the model). Lands after IPC2's merge; keep it LAST at the fold
  and reword it ("model: device_info and the check that its answer names the handle's device",
  keeping its "Size budget: model" line).
- f73d9ef42 WIP D2 (below). Fold into a real commit later; needs a "Size budget: loader 841 -> 868,
  the bundle mapping" line. The kernel shrank (7734 -> ~7699).
- 6cb07ee56 WIP D3 in progress (below).
At the fold: put D2+D3 (and the case moves) in an order that keeps the bench green, or merge
D2 and D3 into one commit (orchestrator's condition 4). A red commit must say so in its message.

### D2 as built (rulings: QA INIT1-root-pages; 8074ecf59, 4ace1722d)
- loader/src/main.rs: loads the kernel and only the second entry, as PID 2 (INIT_PID). It parses no
  later entry, and the grants assert is gone. MAX_PROCESSES and IniE are gone.
  - map_bundle maps the whole initrd (signature + tar) read-only (R|USER) at BUNDLE_AT = 0x2000_0000.
    Its frames' owner is set to PID 2, so the kernel charges them to root, and they are never
    reused while init lives.
  - It refuses (panics) on: an initrd not starting on a page; one over BUNDLE_MAX (0x4000_0000 -
    BUNDLE_AT, 512 MiB); one sharing a page with the DTB; an init image overlapping the mapping
    (map asserts "already mapped").
  - InitialProcess gained a0, a1 (5 usizes; the table is 2 records). The loader prints
    "bundle mapped read-only at 0x20000000 (N bytes)".
- Kernel:
  - arch/riscv/process.rs: InitialProcess a0/a1. setup_loader_process(pid, entry, sp, a0, a1) sets
    registers[9]=a0 via setup_first_thread and registers[10]=a1.
  - ptable.rs: ProcessState::Setup { entry, sp, a0, a1 }. init_from_memory(base) reads exactly 2
    records (no args parameter; main.rs call updated).
  - budget.rs:
    - New consts INIT_PAGES=1024 and INIT_PID.
    - boot_budgets: init_pages = frames owned by PID 2 + THREAD_PAGES + INIT_PAGES.
      rest = pages - init_pages - 2 budget pages. system = rest/4, users = rest - system.
      Processes: system 15, users 47, root keeps 1. Weights unchanged.
    - init is counted, created, charged and threaded in root, and gets budgets 1-3 then devices
      (boot_devices(system, Some(INIT_PID), stamp)).
    - boot_endpoint and boot_log_endpoint deleted.
  - device.rs: comment only.
- Tests:
  - New case bundle-mapped (program bundle-mapped.rs + toml), PASS on both widths. It checks:
    a0/a1; the kernel's entry first, then its own; its read-only segments equal its image; the
    15/47/1 split; root pays for the bundle; map_fixed over the bundle -> InvalidArgument with
    nothing charged; a write faults (the kernel's line "PROGRAM HALT: CPU Exception on PID 2:
    Store page fault of 0x20000000").
  - loader-rejects-{kernel-address,kernel-entry,truncated-elf}: programs = [the corrupt image]
    alone (it must be the second entry). loader-rejects-grants deleted.
  - rd::log_rx -> rd::devices_end (= first_free; nothing follows the devices), renamed at 13
    call sites.
- Pages:
  - boot.md: chain, bundle, loader steps, arg block (IniE row gone), handoff table, Authority,
    R16, R17, Failure. The section "The loader loads only the kernel and init" is BUILT; a new
    "What init does with the bundle" holds the planned INIT2 half (its last paragraph mentions
    bench-bundle-file: D4 moves that).
  - budgets.md "Root, system and users"; memory-layout.md Regions row and diagram;
    testbench.md Bundle files (grants sentence gone; the "Today the loader starts every
    entry..." sentence still stale: D4); devices.md R18; SECURITY.md R18.
  - Doccheck: clean at f73d9ef42.

### D3 as coded so far (6cb07ee56); rng PASSES on both widths through it
- log-server.rs is the tester:
  - _start(bundle, len); reads `programs` via test_programs::bundle::Bundle (a new lib module:
    a tar reader over the a0/a1 mapping).
  - It refuses the boot (Line::Refused(Refusal::..), then PowerOff) on: no `programs` entry; an
    unreadable line; an unknown program; an unknown budget; duplicate budgets; more than 15.
  - It starts each program in order: budget = rd::create(SYSTEM, spec(free_pages/n - 1,
    free_procs/n, 1000)).
  - Handles: [boot (receive right for the first started, else mint badge=place), log mint
    badge=place, named budgets...].
  - Launch through the stub: STUB_BIN at STUB_ENTRY, the image at IMAGE_AT, a 32-page stack at
    STACK_TOP, and a StartupBuilder block at STARTUP_AT. The exit endpoint is the log receive
    right.
  - It prints "[server] started <name> as pid <place>". Places start at 3 (the tester is 2), so
    the old "[pid N]" expects still match.
  - It uses redoubt_rt (StartupBuilder) and so takes rt's panic_handler and allocator. Its own
    panic_handler is removed. The launcher lives in this bin only, because rt in the lib would
    clash with every bin's panic_handler.
- logsrv.rs:
  - Line::{Started, Refused, StartFailed}, enum Refusal, receive_right().
  - GiftsGiven and TAKE_GIFTS are gone (op 6 is now unused). Sender docs say "place".
- rd.rs: GIVEN = 3 (the first named budget); Gifts/take_gifts removed.
- The 8 programs: GIVEN slots. Their tomls use { bin = "...", budgets = [...] } and expect
  "[server] started X as pid N". Their comments still mention TAKE_GIFTS and "in system": FIX.
- Bench:
  - case.rs: Program::{Package,Path} gain budgets; new Program::Bin { bin, budgets }; test_program()
    and budgets(); BUDGETS. check() refuses budgets on the first program, unknown names and
    duplicates. reporter_pid uses test_program().
  - build.rs: programs_entry(); bundle(path, kernel, programs, listing: Option<&[u8]>, files, ...)
    packs kernel, first, programs, the rest, files.
  - main.rs: a [[file]] named "programs" replaces the generated one (for the refusal attack
    cases).

### Orchestrator's answer to the case moves: A, with conditions
- rd::own() is a lib addition: say where it reads from.
- Each moved case keeps exactly what it tested. For class (c), state per case the old budget's
  size/class and what the carved budget gives.
- sched-latency must keep its targets and its LATENCY-SAMPLE protocol: rebase onto a678d8928
  (K15) before touching it.
- Diagnose `process`.
- Fold order keeps the bench green where possible.
- Report the moved cases as a table in this file.
Still open: my question on the own-budget slot. I recommended slot 3 = the program's own budget,
named budgets from slot 4; GIVEN = 3 assumes no own slot. rd::own() depends on that answer.

### The 17 cases (class: a = usage of SYSTEM as own budget; b = needs > INIT_PAGES; c = size/class semantics)
| case | program | class | first failure seen |
| --- | --- | --- | --- |
| budget | budget-test | a | usage(SYSTEM) pages 426 vs 427 |
| device | device-test | a | "map_anon charges its pages to the caller's budget" |
| dma-destroy-quarantine | same | a | "both runs' 8 pages are charged to system (25 -> 31)" |
| ipc-outcomes | same | a | panic at ipc-outcomes.rs:233 (usage(SYSTEM)) |
| page-table-reclaim | same | a | usage_pages() = usage(SYSTEM) |
| process-attack | proc-attack | a | panic at proc-attack.rs:265 (usage(SYSTEM)) |
| process-lifecycle | proc-lifecycle | a | panic at proc-lifecycle.rs:166 |
| map-fixed-attack | same | b | expect("fill") at :280 |
| map-fixed-tables(-rv32) | map-fixed-tables | b | expect("fill") at :76 |
| map-anon-search-bound | same | b | whole 256 MiB area at :120 |
| pages-exhaustion | same | b | map_anon at :50 |
| process | proc-test | ? | nothing after "kernel: checks on"; not diagnosed (look at the full log: target/testbench/process-rv64-smp1.log) |
| redoubt-dead | same | c | refused.err() None, want Refused (a lend the receiver cannot pay for) |
| redoubt-revoke | same | c | mint_from_handle(.., Some(ROOT)) not NotPermitted |
| redoubt-tight | same | c | TEST FAILED |
| dma-reset-quarantine | same | c | the child faulted at 0x0; the checker did not report |
| sched-latency | same | c | target missed (N=16 driver wake p50 11151 > ...): rebase onto a678d8928 first |
Also budget-destroy-kills (budget-destroy-system + budget-victim): the destroyer destroys system
and expected to die with it. It now needs ["log-server", {bin="budget-destroy-system",
budgets=["system"]}, "budget-victim"] or similar. Its program calls logsrv::start itself today.

### Learned / traps
- The equal shares change the numbers the budget attacks pin. E.g. budget-carve-attack's
  "13 processes -> OutOfProcesses": system now has 15 - n*floor(15/n) free. The victim's line
  "mapped and touched 64 pages in system" is the victim's own text and is no longer literally
  true. Re-read each premise: budget-carve's "starve the victim in system" no longer holds,
  because the victim's pages are its own budget's.
- Kernel lines that print a PID (PROGRAM HALT on PID N) need patterns now: PIDs are random but 2.
- `cargo testbench budget` was running at handoff: /tmp/init1-d3-budget.log.
- D4: bench-bundle-file becomes a read-back. The tester (or a sole first program) finds `data`
  via Bundle::find and compares it with tests/data/bundle-file.txt's bytes. Since the program
  cannot read the host file, put the expected bytes in the program or print a hash and match it
  in expect. Move testbench.md "Data entries for init" and the last paragraph of boot.md "What
  init does with the bundle" for the in-init's-place half.
- Attack case to add: a hostile `programs` entry via [[file]] name="programs" (unknown budget;
  unknown program), each judged by the tester's "[server] boot refused: UnknownBudget(1)" line
  plus a power-off, with no "started" line (forbid it).
- K14 (wp-k14 4516a0288) deletes setup_loader_process's reserve_range lines and
  DEFAULT_STACK_SIZE. My change touches the same function (signature and the a1 line): the
  rebase conflict is mechanical. USER_STACK_* in the loader are unchanged as one block.
- Commands: from the worktree, /home/mcloonan/redoubt/.wash/local/in-dev cargo testbench <filter>
  (--allow-skip for the whole bench, ~40 min, background it and tail it); in-dev cargo +nightly
  fmt --all [--check]; in-dev cargo run -q -p redoubt-doccheck. Case logs:
  target/testbench/<case>-<arch>-smp1.log. The bench prints nothing until it ends.
- Gates not yet re-run on 6cb07ee56: fmt, doccheck, size (the D3 lib growth may not be counted;
  log-server is a test program).

## init1-implementer-3 (rebased on main 908fb83c4: K14, K15, the own-budget ruling)

### Slots (QA INIT1-own-budget, ruled A): 1 boot, 2 log (badged), 3 own budget (rd::OWN), 4 on
named budgets (rd::GIVEN = 4). The tester also gives each program an exit endpoint of its own and
prints its notice: `[server] pid N ended: <Cause> <code>, kernel PID K` (processes.md: a launcher
tells children apart by exit endpoints).

### Two facts that decide where a case runs
1. A started program gets no device (slots 1-4 only): device, DMA, console-mapping cases stay in
   init's place, where their own budget is root (init's frames + 1 thread + INIT_PAGES).
2. Only init's stack is a reservation (memory.md, Backing and zeroing: map_anon backs at once; the
   tester's process_map hands over a backed stack). Untouched-page cases stay in init's place.

### The 17 cases (and the 4 others the move touched)
| case | program | class | runs | what changed (old budget -> new) |
| --- | --- | --- | --- | --- |
| budget | budget-test | a | tester | system -> own (equal share for 1 program: system's free bar 1 page, 15 procs, weight 1,000; system-class, depth 2, so the depth chain is one shorter); users in slot 4 |
| device | device-test | a | init (devices) | map_anon's charge read from root |
| dma-destroy-quarantine | same | a | init (DMA) | system now +8 (runs) -2 (the quarantined device objects); the drivers' process objects are root's (creator), which used to hide the -2 |
| ipc-outcomes | same | a | init (MMIO as record) | own charges and the revoke scope in root; child budget still from system |
| page-table-reclaim | same | a, b on Sv32 | tester | a Sv32 round (1024 pages) is more than root keeps; Logger + DONE; own budget |
| process-attack | proc-attack | a | init (untouched) | object page charged to root (creator); pending PID counted in system (parent) |
| process-lifecycle | proc-lifecycle | a | init (console) | own charges read from root |
| map-fixed-attack | same | a | init (untouched) | root; + "a record on an untouched page" (abi.md, The record check never allocates), moved here from budget-syscall-attack |
| map-fixed-tables(-rv32) | map-fixed-tables | b | tester | own budget (~1 GiB on Sv39); Logger + DONE |
| map-anon-search-bound | same | b | tester | own budget (256 MiB area); Logger after the placement checks |
| pages-exhaustion | same | b | init | now fills all three: children fill users and system, it fills root |
| process | proc-test | bug | init (console) | DIAGNOSED: _start(arg) took a0 (now the bundle's address) for a child's startup page and ran as a child; children enter at child_entry, so the dispatch went |
| redoubt-dead | same | c | tester | carves own budget (system -> equal share, system-class) |
| redoubt-revoke | same | c | tester | stamp = own budget; root (above) and users (beside) named, slots 4-5 |
| redoubt-tight | same | c | tester | carves own budget; same exact 4 pages |
| dma-reset-quarantine | same | c | init (DMA) | final search: checker carves system's free, the program searches root (users' pages return to root) |
| sched-latency | same | c | init (RTC IRQ) | see below |
| budget-destroy-kills | budget-destroy-system | - | tester | destroyer holds system (slot 4); verdict: two kernel kills + the tester's two `ended: Killed` lines; caller-last order NOT pinned (random PIDs, no backrefs): residual in the toml |
| process-map-untouched-attack (main) | same | a | init (untouched) | system -> root |
| lend-untouched-page | lend-untouched | - | init (untouched) | D3 had moved it behind the tester, where it PASSED VACUOUSLY (backed pages); back in init's place, lending to its own endpoint, powering off itself |
| budget-syscall-attack | same | - | tester | untouched-record entry moved to map-fixed-attack; records carved from own |

### The 8 attack programs and the victim
system has 0 free pages after the tester's shares. The victim gets `system` (+`users` in
carve) and checks every budget it holds is within its limits (I5) before touching its 64 pages;
destroying system still kills it (R10). Carve: pages past system's free on system, per-limit and
wrap attempts on users and on a hog carved from its own budget. Destroy: the B>C>D tree in its
own budget. Forge: the closed index is a child of its own; slots 1-5 aliases. Table: pool from
users, table pages charged to its own budget (63). Syscall: sandbox from its own, slots 1-4
spared. kernel-half, legacy-gone, lender-touches-lent: users in slot 4 (comments only).
New cases: programs-unknown-budget-attack, programs-unknown-program-attack (bad line is line 2,
so nothing starts: line 1 is good and no `started` line may appear).

## Handoff from init1-implementer-3 (parked; tip d0e385529, base main 908fb83c4)

### Commits on wp-init1, oldest first (all after 908fb83c4)
- d8a6330af D1 device_info (accepted); c31997716 D1 device-info-attack (accepted).
- 7bcdb9998 WIP model work: keep LAST, reword at the fold (after IPC2's merge).
- 0c032ae20 WIP D2 (as in the previous handoff; rebased: setup_loader_process now K14's form + a0/a1;
  memory-layout Regions status lists boot-stack-reservation and bundle-mapped; size-budget kernel
  max 7763 = 7752 (main) + 11 (device_info), set in d8a6330af's rebase).
- 58996a42c WIP D3 (implementer-2's tester).
- af6cc3b4a own budget in slot 3 (rd::OWN=3, rd::GIVEN=4); per-program exit endpoints + watcher
  threads; budget attacks reworked; programs-unknown-{budget,program}-attack; budget-test moved;
  map-fixed-attack -> root + the record-check probe; process-map-untouched -> root.
- bda27b631 init's-place cases read root (device, dma-*, ipc-outcomes, proc-attack, proc-lifecycle,
  proc-test arg fix); page-table-reclaim behind the tester.
- 5d8a8a82c pages-exhaustion fills root/system/users; map-fixed-tables(-rv32), map-anon-search-bound
  behind the tester.
- f9c250d21 redoubt-dead/-tight/-revoke behind the tester.
- 3cc48a07e lend-untouched-page back in init's place (own serving thread, own power-off).
- c71d2d158 formatting.
- 235f0f179 sched-latency: init's place carves a measurer budget under system (weight 240,000) and
  starts a copy of itself with init's whole table. Cause proven: INIT_WEIGHT=250,000 experiment
  passed; main N=16 rv64 p99 22/31 ms, branch now 33/42 (rv32 34/45): passes, smaller margin.
- d0e385529 pages (testbench.md "Starting a case's programs" built + new planned "The servers'
  cases under init"; relay/rule F/Gifts removed; Bundle files describes the read-back; Data
  entries trimmed; boot.md; abi.md Records cites bench:map-fixed-attack; budgets.md R10 residual
  "caller last not pinned"); tester prints "[server] <name> (pid N) ended: ..."; destroy-kills
  toml uses it; boot-stack-reservation -> root; D4 program tests/programs/src/bin/bundle-file.rs
  drafted. NOT YET BUILT OR RUN: log-server/logsrv name change, bundle-file.rs, destroy-kills.

### Verified (exit codes from in-dev cargo testbench <filter>, both widths, before d0e385529)
PASS: budget-* (all), budget, programs-unknown-*, rng, map-fixed-attack, process-map-untouched,
process*, page-table-reclaim, pages-exhaustion, map-fixed-tables(-rv32), map-anon-search-bound,
redoubt-dead/-revoke/-tight, dma-*, device*, ipc-outcomes, lend-untouched-page, sched-latency(+tcg).
fmt --check exit 0 at c71d2d158; doccheck exit 0 before the last page edits.
A whole-bench run was in progress on mixed code (/tmp/init1-3-whole1.log, 138 results): FAILs
bench-bundle-file (D4, expected), boot-stack-reservation (fixed in d0e385529, not re-run), docs
(C1 **Open:** in a built section, fixed in d0e385529). Treat it as stale.

### Left
- D4: add `{ name = "bundle-file", test = false }` to tests/programs/Cargo.toml (autobins off),
  replace tests/bench-bundle-file.toml with .wash/local/INIT1-bench-bundle-file.toml, run it.
- Build and run: budget-destroy-kills, rng (tester change), boot-stack-reservation, bench-bundle-file.
- Whole bench both widths: in-dev cargo testbench --allow-skip (~40 min, background, tail it);
  exactly one SKIP. Then fmt --check, doccheck, unsafe and size gates (log-server gained one
  documented unsafe in a test program, not a ratchet crate).
- Orchestrator's (a): the commit message must say budget-syscall-attack's untouched-record entry
  moved to map-fixed-attack, cited on abi.md "Records". (b) done as far as possible: names in the
  ended line; caller-last order cannot be pinned (no backrefs, notice order not defined): residual
  on budgets.md R10; orchestrator to tell the Architect.
- Fold: d8a6330af, c31997716, then ONE commit for D2+D3+D4 (bench-bundle-file must change with the
  loader), with "Size budget: loader: 841 -> 868, the bundle mapping" (re-check numbers), then the
  model commit last after IPC2's merge. Main has moved on (15b510297): rebase before the fold.

### Rulings: 8074ecf59 (pages A, BUNDLE_AT), 4ace1722d (grants C), 908fb83c4 (own budget slot 3).
### Traps
- Every command via /home/mcloonan/redoubt/.wash/local/in-dev from the worktree; never git in
  /home/mcloonan/redoubt; never stash. The bench prints PASS/FAIL lines as it goes; logs in
  target/testbench/<case>-<arch>-smp1.log.
- A started program has no device and no untouched page (all backed); system has 0 free pages
  after the shares; root has 1 process (init's) so carve children with processes from system/users.
- Don't edit compiled sources while a whole bench runs: later cases build them.

## init1-implementer-4: rebased, D4, folded (tip 10f1431fd, base main 0a82e2090)

### Branch wp-init1, four commits
- 5ce7bf05e kernel, redoubt-sys: device_info (D1, unchanged)
- 38e1a93e4 tests: device-info-attack (D1, unchanged)
- 325815dbf loader, kernel, testbench: the loader starts only init, which starts the rest
  (D2+D3+D4; moved-cases table, the untouched-record move to map-fixed-attack, R10 residual,
  "Size budget: loader: the bundle mapping, its four refusals and the first thread's a0 and a1")
- 10f1431fd model: device_info, and the check that its answer names the handle's device (last)
Folded tree == pre-fold tip e51c1e3e3 (git diff empty), which the bench below ran on, except
the message-only size-budget fix.

### Rebase notes
- Conflicts in boot.md, budgets.md, memory-layout.md, abi.md: status lines put in main's
  <details> form; loader-rejects-grants dropped from boot.md's lists (D2 deletes the case);
  block-order Mermaid figure loses IniE; the user-space memmap gains
  `0x2000_0000 | bundle, init only, read-only (at most 512 MiB)`.
- Two 7-test status lines folded: testbench.md "Starting a case's programs" (D2-D4),
  devices.md device_info (model).
- model: Mutation::ALL 142 -> 143 (IPC2 added one on main; the rebase auto-merged the list).
  size-budget model ceiling 10179 (main) + 60 = 10239.

### Runs (all via in-dev from the worktree)
- cargo testbench <f> for bench-bundle-file, budget-destroy-kills, boot-stack-reservation,
  rng: PASS rv64+rv32, exit 0 each.
- Whole bench, cargo testbench --allow-skip, on 342f82b64 (log /tmp/init1-4-whole1.log):
  275 PASS, 1 SKIP (bench-ssh-loopback-openssh), 4 FAIL: host-tests, model-host-tests,
  stride-host-tests (the ALL count) and size-budget (WIP message lacked the loader reason).
- After the fix: host-tests, model-host-tests, stride-host-tests exit 0 (each PASS).
- After the fold: size-budget, unsafe-budget, docs exit 0; doccheck exit 0;
  cargo +nightly fmt --all --check exit 0.
- rv32: every rv32 case in the whole bench passed (kernel, loader, sys, rt, servers built).

### Green at every commit
325815dbf's tree is the tip less the model commit, which touches only model/, model/tests,
devices.md, model.md and the model ceiling: its model is main's, so the host tests are main's.
Not separately bench-run.

### Trap found
The worktree's cached doccheck reported 236 false C1/C7 findings (sections in <details> form
"uncovered") until `cargo clean -p redoubt-doccheck`; touching its sources did not rebuild it.

### Fix round 1 (tips: 5ce7bf05e, 38e1a93e4, 56253e86c, 856bc0894)
All fixes folded into 56253e86c (the model commit unchanged but rebased). Applied: RED P2-1 text
(boot.md bundle paragraph + new boot.md residual "init keeps it so", memory-layout row); P2-2
residual kept, reason restated in budgets.md R10 + budget-destroy-kills.toml + message (ended
lines name programs but come in no defined order; only the kernel's kill lines are ordered and
carry drawn PIDs; no cross-line check). Editor 1-8: m1-separation.md, budgets.md (system-class
risk), devices.md (handed + residual, wrapped), loader emit_devices comment, kernel device.rs
"all held by init" (no case matches), docs/kernel/README.md:43,276, GLOSSARY bundle,
kernel-attack-gaps R16 (63-program gap gone); also objects.md device row, testbench.md toml
example; wrapped boot.md step 6, memory-layout Regions, testbench.md relay paragraph.
Simplifier: 1 comment at log-server give(); 3 budgets only on the `bin` form; 4 devices_end gone
(16 files call first_free); 6 loader clones (initrd_range moved, map_bundle takes &Range, one
pages clone); 2/5 kept, comment at bundle.rs.
Declined: abi.md Records keeps bench:map-fixed-attack (it checks a record on an untouched page,
InvalidArgument). bundle.rs's reason is independence from the loader's parser, not "tar cannot
serve": tar-no-std builds without alloc.
Runs: test-programs build rv64+rv32 exit 0; cargo testbench for budget-destroy-kills,
bundle-mapped, bench-bundle-file, map-fixed-attack, device*, dma*, irq-first-receive, timeouts,
scan-bounds, receive-bad-record, pid-reuse-authority, budget-carve-attack, sched-latency,
virtio-probe: 40 PASS, each exit 0. After the fold: fmt, doccheck, size-budget, unsafe-budget,
docs, host-tests (13 PASS) exit 0.
