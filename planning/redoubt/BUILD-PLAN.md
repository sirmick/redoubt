# Build plan: milestone 1

Draft for review. Owns: the work packages for milestone 1, their order, and how they are accepted.
The design is frozen (design v4); this plan changes, the design does not without a HISTORY.md entry.

**Milestone 1:** Alice and Bob logged in over SSH on QEMU, separated, and Alice's agent running under
a lease, contained. Every property backed by an attack test (PLAN.md).

## How to read a work package
Each package (WP) has:
- **Reads:** the design it implements. The spec wins over anything else; rules are cited as R1-R12
  and invariants as I1-I14 (KERNEL-SPEC.md).
- **Delivers:** the crates, programs or changes it produces, and the paths it may write.
- **Accepted when:** its tests. A package is done when these pass in `cargo testbench` (and the
  model's property tests, where named), a reviewer has checked it against the spec, and STATUS.md
  is updated. Every security property it touches has an **attack case** that tries to break it.
- **Needs:** the packages that must be done first.
- **Size:** S (a few hundred lines), M (up to ~1.5k), L (more).

Everything in milestone 1 is `no_std` + `alloc` Rust or Elixir on beamlet; the Rust `std` target is
not needed.

## Settled before the build
- **Randomness:** a `random` system call (KERNEL-SPEC.md), used by WP-B1, WP-S1, WP-S3.
- **Widths:** milestone 1 is built and booted on rv64. rv32 must keep **compiling** (kernel, loader,
  `redoubt-sys`, `redoubt-rt`, servers): a build check in the bench, no rv32 boots. Width-specific
  code only in paging geometry, trap entry and saved context, and the ABI's register encoding
  (PLAN.md).

## Work packages

### Track M: model and contracts (start immediately, in parallel)
**WP-M0. Executable security model.** Size L.
- Reads: KERNEL-SPEC.md (all), CONTAINMENT.md, CAPABILITIES.md (steward policy parts used in M1).
- Delivers: `redoubt/model/`: a host Rust crate implementing every object, system call, error and
  rule of KERNEL-SPEC.md with the same names and arguments; the steward's M1 policy (principals,
  sessions, vault sessions, leases, approvals, declassification) as a layer above it; property
  tests (random operation sequences) for invariants I1-I14 and the policy's properties; a trace
  format (a sequence of calls and their expected results) that WP-C1 replays on the real kernel.
- Accepted when: all property tests pass at 10^6 sequences; **each rule R1-R12, deliberately broken
  in the model, makes at least one property test fail** (so the tests are not vacuous).
- Needs: nothing. Handed to red-team agents as soon as it passes.

**WP-W1. Wire codecs.** Size M.
- Reads: WIRE.md.
- Delivers: `redoubt/wire/`: a `no_std` 9P2000 codec; the typed-message table format and a
  generator emitting Rust and Elixir codecs; a strict JSON (I-JSON) parser profile for manifests.
  Fuzz targets for all three.
- Accepted when: round-trip tests; fuzzing finds no panics or hangs (1 hour each in CI-equivalent
  runs); the Elixir codec round-trips the same vectors on beamlet.
- Needs: nothing.

**WP-A1. System call ABI crate.** Size S.
- Reads: KERNEL-SPEC.md (system calls, messages, errors).
- Delivers: `redoubt-sys`: call numbers, argument and result register encodings for both widths, the
  error enum, and a host round-trip test for every call and error (like `xous`'s `Result` test).
- Accepted when: round-trip tests pass for both widths' encodings; the names match WP-M0 exactly.
- Needs: WP-M0's call list (can start from KERNEL-SPEC.md directly).

**WP-L1. littlefs in pure Rust.** Size L.
- Reads: NAMESPACES.md (filesystem section), littlefs `SPEC.md`.
- Delivers: `redoubt/littlefs/`: `no_std` littlefs over a block trait; custom attributes; host tests.
- Accepted when: differential tests against the C reference **on the host only** (images written by
  either read identically in the other); crash injection at every block write leaves a mountable,
  consistent image; fuzzed images never panic.
- Needs: nothing.

**WP-T1. Test bench extensions.** Size M.
- Delivers: in `redoubt/testbench`: an SSH client driving sessions over QEMU port forwarding;
  multi-session scripted scenarios; a virtio-blk image and virtio-net per case; attack programs as
  first-class case inputs; model-trace replay support for WP-C1.
- Accepted when: a self-check case proves each new feature can fail (TENETS.md 6).
- Needs: nothing.

### Track K: the kernel (one integrator at a time; see "Hotspots")
**WP-K1. Budgets and handle tables.** Size L.
- Reads: KERNEL-SPEC.md objects (Budget, Handle), R6-R10, `budget_*`, `handle_close`, `time_now`, `random`.
- Delivers: budget objects with page and process accounting, carving, accounts, deadlines recorded
  (enforced by WP-K5), destruction sweeping stamped handles; per-process handle tables charged in
  pages; 64-bit never-reused ids.
- Accepted when: kernel cases for R6-R10 and I2, I5, I8, I10, I12; attack cases: carve beyond the
  parent, exhaust handle tables, destroy while handles are held elsewhere, forge a handle index.
- Needs: WP-A1.

**WP-K2. Endpoints and messages.** Size L.
- Reads: KERNEL-SPEC.md Endpoint, Messages, R1-R4, `endpoint_create`, `mint`, `call`, `send`,
  `receive`, `reply`.
- Delivers: endpoints; the four IPC calls with lend and transfer; `mint` with stamps; badge-0
  receive rights; the label check; fair waiting by account; lends that outlive their lender;
  transfer opt-in.
- Accepted when: kernel cases for R1-R4 and I3, I4, I7, I9, I11; attack cases: steal a receive
  right, mint badge 0, mint into a foreign budget, unequal-label call, 10,000 blocked senders with
  another account still served in turn, lender destroyed mid-call with the server surviving,
  unrequested transfer.
- Needs: WP-K1.

**WP-K3. Device objects and interrupts.** Size M.
- Reads: KERNEL-SPEC.md Device, R5, R11, `map_device`, `dma_alloc`, `system_reset`; DEVICE-GRANTS.md.
- Delivers: the loader creating MMIO, IRQ and Reset objects from the device tree; IRQ receive with
  mask-on-fire; DMA allocation returning physical addresses.
- Accepted when: `uart-irq` rewritten on IRQ receive; attack cases: map an MMIO range without its
  handle, receive on an IRQ handle not held, name RAM by physical address.
- Needs: WP-K1 (and WP-K2 for `receive`).

**WP-K4. Process creation and exit.** Size M.
- Reads: KERNEL-SPEC.md Process, `process_*`, exit notices, R10; PACKAGES.md (launching); INIT.md.
- Delivers: `process_create`/`process_map`/`process_start`; exit notices with cause and blamed
  account; per-thread current-message account; the boot loader loading only the kernel and `init`.
- Accepted when: kernel cases for exit notices (all three causes), blamed account on a server fault;
  attack cases: map into a started process, W+X through `process_map`, oversize handle list;
  `bench-bundle-file` becomes a clean boot in which a guest reads its `[[file]]` entry back (today
  the loader refuses data entries).
- Needs: WP-K2.

**WP-K5. Timer, timeouts and preemption.** Size M.
- Reads: KERNEL-SPEC.md R12, timeouts, deadlines; RESOURCES.md.
- Delivers: the kernel-owned timer; timeouts on `call`/`send`/`receive`; budget deadlines enforced;
  stride scheduling over budgets with the two classes; `rdtime` readable from user mode.
- Accepted when: a spinning budget cannot delay another beyond its weight; a sleeping-waking budget
  cannot exceed its share; every blocking call returns by its timeout (I13); a deadline destroys
  its budget; the old IRQ-0 timer path and `timer` case are gone.
- Needs: WP-K2.

**WP-K6. Delete the legacy interface.** Size M.
- Delivers: removal of SID connects, scalar message kinds, `ClaimInterrupt`, the `grants` entry,
  name lookup and `PlatformSpecific` timer calls; old test programs migrated or deleted.
- Accepted when: the bench passes with no legacy calls; the `unsafe` ratchet is lower than before
  WP-K1; kernel line count reported.
- Needs: WP-K1 to WP-K5, WP-R1.

**WP-C1. Model conformance.** Size M.
- Delivers: a bench case replaying WP-M0 traces on the real kernel and comparing every result;
  a system-call fuzzer program (random and hostile arguments; I14).
- Accepted when: 10^5 model traces replay with identical results; the fuzzer runs
  for its budget with no kernel panic.
- Needs: WP-M0, WP-K1 to WP-K5, WP-T1.

### Track R: user runtime and system servers
**WP-R1. Native runtime crate.** Size M.
- Delivers: `redoubt-rt`: startup-block parsing, typed handle wrappers, `call`/`send`/`receive`
  helpers, an allocator over `map_anon`, a panic handler; the shared server library:
  `admit(account)`, `check(caller_labels, object_labels, read|write)`, a 9P server skeleton and
  typed-message dispatch (WIRE.md).
- Accepted when: unit tests on the host; a bench echo server and client use only this crate.
- Needs: WP-A1, WP-W1 (and WP-K2 to run on the kernel).

**WP-R2. Loader stub.** Size S.
- Reads: PACKAGES.md (launching), INIT.md (startup block).
- Delivers: the flat-binary stub at its fixed address: parse the ELF from memory, map segments
  (code executable and read-only), free the image, jump.
- Accepted when: fuzzed ELF images never escape the child (the child faults or exits; nothing else
  is affected); attack case: a hostile ELF from a user parent hurts only the child.
- Needs: WP-R1, WP-K4.

**WP-R3. init and the boot manifest.** Size M.
- Reads: INIT.md (all), WIRE.md (JSON).
- Delivers: `init`: reads the manifest, builds the budget tree, hands out device handles, starts
  every system server through the stub, restarts with the rate limit, crash blame and the
  steward logout signal, reboot as last resort.
- Accepted when: the M1 manifest boots every server; a crashing server restarts on the same
  endpoint; blame case: 3 crashes blamed on one account logs that account out and nobody else;
  more than 5 restarts in 60 s reboots.
- Needs: WP-R2, WP-W1, WP-K3, WP-K5.

**WP-R4. bootfsd and consoled.** Size S.
- Delivers: `bootfsd` (read-only 9P over the verified bundle); `consoled` (UART driver serving
  `/dev/cons` over 9P, IRQ receive).
- Accepted when: 9P conformance vectors from WP-W1; typing on the UART reaches a 9P reader.
- Needs: WP-R1, WP-K3.

### Track B: beamlet on Redoubt (parallel with track K once WP-R1 exists)
**WP-B1. The Redoubt platform for beamlet.** Size M.
- Reads: beamlet DESIGN.md (I/O), PACKAGES.md.
- Delivers: a `no_std` `Platform` over `redoubt-rt`: console over `/dev/cons`, monotonic and wall
  time, randomness (`random`), module loading through `bootfsd`, the asynchronous 9P client on a small
  pool of I/O threads.
- Accepted when: **Elixir prints on the box** (the M1 step 2 milestone); beamlet's differential
  suite subset runs on the box with identical output.
- Needs: WP-R1, WP-R4.

**WP-B2. IEx on the UART console.** Size S.
- Delivers: an IEx session on the UART; the first Redoubt IEx helpers (`ls`, `cd`, `cat` over 9P).
- Accepted when: a bench case types expressions at IEx and checks the answers (the M1 step 3
  milestone).
- Needs: WP-B1, WP-R3.

### Track D: storage and network
**WP-D1. blkd.** Size M. virtio-blk driver (the `virtio-drivers` crate), partition table,
block-range handles, validation of every ring index and length.
- Accepted when: block round trips; a hostile-device model (malformed rings) never corrupts other
  memory or panics blkd.
- Needs: WP-R1, WP-K3.

**WP-D2. fsd.** Size M. 9P over littlefs on a block range; one label set per volume from the
manifest; `admit` and `check` on every request.
- Accepted when: 9P conformance; per-volume label cases (read up and write down refused);
  admission per account; the no-leaky-state observer sees no change from a vault writer.
- Needs: WP-D1, WP-L1, WP-R1.

**WP-D3. netd and ipd.** Size L. virtio-net driver; `ipd:lan` on `smoltcp` serving `/net` over
9P with IP-prefix-and-port capabilities that never include the box's own addresses; refuses
labelled callers.
- Accepted when: TCP connect and listen through `/net`; attack cases: connect outside the granted
  prefix, connect to the box's own address, a labelled caller refused. The bench's network is
  QEMU user mode with `restrict=on` (no outside peer); this package adds the peer it needs to the
  bench, with a self-check that the guest reaches nothing else.
- Needs: WP-R1, WP-K3, WP-W1.

### Track S: security servers
**WP-S1. keyd.** Size S. Holds keys; signs through a badge-scoped capability; never holds keys
that authenticate a person to the box; constant-time signing.
- Needs: WP-R1.

**WP-S2. steward (stateless, milestone 1).** Size L.
- Reads: CAPABILITIES.md, CONTAINMENT.md, INIT.md, PACKAGES.md (launching).
- Delivers: principals and SSH keys from the manifest; session budgets and namespaces; launching
  beamlet VMs through the stub; vault sessions (`alice+X`); leases for agents; the powerbox with
  rendering, binding, per-account caps and random request ids; declassification by snapshot;
  logout on blame; no-write-down on its own records.
- Accepted when: the model's policy traces (WP-M0) replay against it; approval cases (control
  characters stripped, swapped requests refused, labelled requests shown only to label owners,
  floods capped).
- Needs: WP-R3, WP-B1, WP-D2.

**WP-S3. sshd.** Size M. `sunset`-based; host key through `keyd`; user authentication through the
steward; rejects keys `keyd` holds; each channel labelled with its session's labels;
`ssh approve@box`.
- Accepted when: `alice@`, `alice+secrets@`, `bob@` and `approve@` sessions work from the bench's
  SSH client, with the box's host key pinned (`net.host_key`); loopback login with a `keyd` key
  refused. The bench's loopback self-checks log in as one host user, so per-user separation is
  first tested here.
- Needs: WP-D3, WP-S1, WP-S2.

### Track E: the milestone
**WP-E1. Alice's agent and the attack suite.** Size M.
- Delivers: the scripted hostile agent and the scripted hostile user (Bob), and every case in
  PLAN.md's milestone 1 attack suite not already delivered by the packages above. An attack's
  failure is asserted by the system (the kernel, the victim, or a clean power-off), never by a
  line the attacker itself prints: the console does not say who wrote a line.
- Accepted when: the whole suite passes, and **milestone 1 is declared done** in STATUS.md.
- Needs: everything above.

## Order
```
start now, in parallel:  M0  W1  L1  T1  A1
kernel, serialized:      K1 -> K2 -> K3 -> K4 -> K5 -> K6
runtime:                 R1 (after A1, W1) -> R2 (after K4) -> R3 (after K3, K5)
                         R4 (after R1, K3)
beamlet:                 B1 (after R1, R4) -> B2 (after R3)
storage and network:     D1 (after R1, K3) -> D2 (after L1);  D3 (after R1, K3)
security:                S1 (after R1);  S2 (after R3, B1, D2);  S3 (after D3, S1, S2)
conformance:             C1 (after M0, K5, T1)
milestone:               E1 (after all)
```
The critical path is the kernel track (K1 to K5), then R3, S2 and S3. Everything off that path
(model, codecs, littlefs, bench, drivers, beamlet's platform) can proceed in parallel.

## Hotspots (serialize edits to these)
- `kernel/src/syscall.rs`, `kernel/src/services.rs`, `kernel/src/mem.rs`,
  `kernel/src/arch/riscv/process.rs`: only the one kernel package in progress edits them.
- `redoubt-sys` (the ABI): changed only with a KERNEL-SPEC.md change.
- The boot manifest format (INIT.md) and the typed-message tables (WIRE.md and each server's note):
  one owner each; changes go through the design notes first.
- `planning/redoubt/`: design changes only with a HISTORY.md entry; STATUS.md updated by whoever
  finishes a package.

## Review of each package
Before a package is accepted, the round's reviewers read it the way they read the design: a red-team
reviewer attacking it against the spec and the attack suite, a simplifier looking for code to delete,
and an editor checking that the code, its comments and the notes agree. Findings are fixed or
recorded before the package is marked done.
