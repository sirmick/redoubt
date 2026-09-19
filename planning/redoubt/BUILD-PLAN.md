# Build plan: milestone 1

Draft for review. Owns: the work packages for milestone 1, their order, and how they are accepted.
The design is frozen (design v4); this plan changes, the design does not without a HISTORY.md entry.

**Milestone 1:** Alice and Bob logged in over SSH on QEMU, separated, and Alice's agent running under
a lease, contained. Every property backed by an attack test (PLAN.md).

## How to read a work package
Each package (WP) has:
- **Reads:** the design it implements. The spec wins over anything else; rules are cited as R1-R12
  (with R4a and R4b) and invariants as I1-I15 (KERNEL-SPEC.md).
- **Delivers:** the crates, programs or changes it produces, and the paths it may write.
- **Accepted when:** its tests. A package is done when these pass in `cargo testbench` (and the
  model's property tests, where named), a reviewer has checked it against the spec, and STATUS.md
  is updated. Every security property it touches has an **attack case** that tries to break it.
  An attack case asserts its outcome through the system (the kernel, the victim, or a clean
  power-off), never through the attacker's own output: the console does not say who wrote a line,
  so a hostile program can print its own PASSED line.
- **Needs:** the packages that must be done first.
- **Size:** S (a few hundred lines), M (up to ~1.5k), L (more).

Everything in milestone 1 is `no_std` + `alloc` Rust or Elixir on beamlet; the Rust `std` target is
not needed.

## Settled before the build
- **Randomness:** a `random` system call returning one `u64` (KERNEL-SPEC.md), used by WP-B1,
  WP-S1, WP-S3.
- **Widths:** milestone 1 is built and booted on rv64. rv32 must keep **compiling** (kernel, loader,
  `redoubt-sys`, `redoubt-rt`, servers): a build check in the bench, no rv32 boots. Width-specific
  code only in paging geometry, trap entry and saved context, and the ABI's register encoding
  (PLAN.md); `redoubt-sys` needs none, since both widths share one register layout (KERNEL-SPEC.md,
  ABI).
- **Typed-message tables:** each server package (WP-D1, WP-D2, WP-D3, WP-S1, WP-S2, WP-S3) writes
  the tables of the protocols its server serves into that server's note, in WIRE.md's format, with a
  HISTORY.md line: a small design addition, reviewed as one.
- **The owner's answers 1-101** (QUESTIONS.md) are in the notes. Packages built before an answer
  that changes them get a follow-up package below (WP-A2, WP-W2, WP-M1, WP-R1b) rather than a
  silent edit.

## Work packages

### Track M: model and contracts (start immediately, in parallel)
**WP-M0. Executable security model.** Size L.
- Reads: KERNEL-SPEC.md (all), CONTAINMENT.md, CAPABILITIES.md (steward policy parts used in
  milestone 1).
- Delivers: `redoubt/model/`: a host Rust crate implementing every object, system call, error and
  rule of KERNEL-SPEC.md with the same names and arguments; the steward's milestone 1 policy
  (principals, sessions, vault sessions, leases, approvals, declassification) as a layer above it;
  property tests (random operation sequences) for invariants I1-I14 and the policy's properties; a
  trace format (a sequence of calls and their expected results) that WP-C1 replays on the real
  kernel.
- Accepted when: all property tests pass at 10^6 sequences; **each rule R1-R12, deliberately broken
  in the model, makes at least one property test fail** (so the tests are not vacuous).
- Needs: nothing. Handed to red-team agents as soon as it passes.

**WP-M1. The model follows answers 28-101.** Size M.
- Reads: KERNEL-SPEC.md as changed by answers 28-101: R2 (groups, system callers by budget), R3
  (lends charged to both sides, abandoned calls and their notice), R4 (delivery or `Refused`), R4a
  (at the limit no calls are taken), R4b, R10 (messages in flight, handles in queued messages
  swept), R11, R12 (`first`); open calls, the current call and `serve`; exit notices with
  `blamed_labels`, held by the process object charged to its creator; a budget's own page charged
  to its parent; class inherited; `random` as one `u64`; message ids per receiving process and
  random PIDs; a badge-0 exit endpoint; I2, I5 (unconditional), I7, I8, I10, I11, I12, I15 (new).
  CONTAINMENT.md (`check`, metadata as reads, `admit` per badge for account 0 with a fair share per
  badge, reader budgets answering a call, blame per (account, label set) ending every budget of it,
  fixed sub-budgets per label set); CAPABILITIES.md (`MAX_LEASE` as steward policy, nested
  sub-agents, the approval screen, narrowing handles as revocation scopes, `keys` in leases).
- Delivers: `redoubt/model/` updated: the README's interpretation choices the spec has now settled
  changed to match or marked settled (6 and 7 change: a `send` is never served or blamed; 8, 10,
  14, 20, 23, 24 stand), and the open questions it listed closed; mutations for each new rule (an
  abandoned call never reported, a lend charged to one side only, a delivery failing `receive`
  instead of `Refused`, blame of a call other than the current one, blame falling back to another
  thread's call, a revoked handle inside a queued message delivered, a badged exit endpoint
  accepted, a class argument honoured).
- Accepted when: as WP-M0, at 10^6 sequences, adding I15 and the new mutations.
- Needs: WP-M0.

**WP-W1. Wire codecs.** Size M.
- Reads: WIRE.md.
- Delivers: `redoubt/wire/`: a `no_std` 9P2000 codec; the typed-message table format and a
  generator emitting Rust and Elixir codecs; a strict JSON (I-JSON) parser profile for manifests.
  Fuzz targets for all three.
- Accepted when: round-trip tests; fuzzing finds no panics or hangs (1 hour each in CI-equivalent
  runs); the Elixir codec round-trips the same vectors on beamlet.
- Needs: nothing. Merged.

**WP-W2. Wire generator follows answers 28, 41, 42, 56 and 98.** Size S.
- Reads: WIRE.md (Tables, Layout in a message).
- Delivers: `redoubt/wire/`: `handle[N] KIND` in the table format, the kind in the generated docs
  only (answer 56: kinds are checked by use, `WrongObject` on first use); code 1 `Malformed`
  reserved and added to every error table (`tables/example.md`'s code 1 renumbered); a table giving
  code 1 its own meaning refused; no `kind` column (every milestone 1 typed message is a `call`,
  answer 98).
- Accepted when: the drift test covers kinds and `Malformed`; a table with an unknown kind or its
  own code 1 fails generation; the Elixir codec round-trips the new vectors.
- Needs: WP-W1.

**WP-A1. System call ABI crate.** Size S.
- Reads: KERNEL-SPEC.md (system calls, messages, errors).
- Delivers: `redoubt-sys`: call numbers, argument and result register encodings (one layout for
  both widths), the error enum, and a host round-trip test for every call and error (like `xous`'s
  `Result` test).
- Accepted when: round-trip tests pass; the names match WP-M0 exactly.
- Needs: WP-M0's call list (can start from KERNEL-SPEC.md directly). Merged.

**WP-A2. ABI follows answers 28-101.** Size S.
- Reads: KERNEL-SPEC.md (`process_start`, `budget_create`, `serve`, `random`, Messages, ABI,
  Constants, the error table).
- Delivers: `redoubt-sys`: `process_start`'s `arg`; `receive`'s one record (`call`, `send`,
  `interrupt`, `exit`, `abandoned`), with `blamed_labels` in the exit notice and no badge notice or
  handle kinds; the `serve` call; `random` returning one `u64` (`MAX_RANDOM` gone);
  `budget_create`'s `first` flag in place of a class; no `MAX_LEASE` (a steward constant); the
  error rows as the spec now lists them (`Refused` at delivery for `call`, no `Busy` or
  `OutOfMemory` from `receive`, `NotPermitted` for a badged exit endpoint).
- Accepted when: round-trip and malformed-input tests for every changed call and record; the fuzz
  target rerun; names match WP-M1.
- Needs: WP-A1. Before WP-K2 and WP-R1b use the new records.

**WP-L1. littlefs in pure Rust.** Size L.
- Reads: NAMESPACES.md (filesystem section), littlefs `SPEC.md`.
- Delivers: `redoubt/littlefs/`: `no_std` littlefs over a block trait; custom attributes; host tests.
- Accepted when: differential tests against the C reference **on the host only** (images written by
  either read identically in the other); crash injection at every block write leaves a mountable,
  consistent image; fuzzed images never panic.
- Needs: nothing. Merged. It relies only on `blkd`'s contract (IO-ARCHITECTURE.md, Storage).

**WP-T1. Test bench extensions.** Size M.
- Delivers: in `redoubt/testbench`: an SSH client driving sessions over QEMU port forwarding;
  multi-session scripted scenarios; a virtio-blk image and virtio-net per case; attack programs as
  first-class case inputs; model-trace replay support for WP-C1.
- Accepted when: a self-check case proves each new feature can fail (TENETS.md 6).
- Needs: nothing. Merged.

**WP-T1b. Attack verdicts from the system.** Size S. (Emerged from answer 26.)
- Delivers: every existing attack case takes its verdict from the kernel, a victim or an
  `attack-checker`, never from the attacker's output; `log-server` prefixes every relayed line with
  the sender's PID from the kernel; `bench-attack-forgery`.
- Accepted when: `bench-attack-forgery` shows a client cannot forge an unprefixed or another PID's
  line; `wx` and `irq-attack` are listed as survival-only until WP-K4 and WP-K3.
- Needs: WP-T1. Merged.

### Track K: the kernel (one integrator at a time; see "Hotspots")
**WP-K0. Kernel memory panics.** Size S. (Emerged from WP-T1b's audit.)
- Delivers: the three kernel panics reachable from any unprivileged process fixed (lending an
  untouched page, moving a page only lent, a lender unmapping its lent page), and the
  out-of-memory `expect`s beside them; the lend and move paths check a whole range before any page
  moves.
- Accepted when: cases `lend-untouched-page`, `move-borrowed-page`, `return-lent-unmapped`,
  `syscall-attack`, `touch-beyond-ram`, on 1 and 4 harts.
- Needs: nothing. Merged.

**WP-K1. Budgets and handle tables.** Size L.
- Reads: KERNEL-SPEC.md objects (Budget, Handle, the cost table), R6-R10, `budget_*`,
  `handle_close`, `time_now`, `random`; errors and the order of checks. (What R10 does to messages
  needs endpoints: WP-K2.)
- Delivers: budget objects with page, process and weight accounting, carving, accounts, deadlines
  recorded (enforced by WP-K5), class inherited from the parent, the `first` flag recorded
  (scheduled by WP-K5), a budget's own page charged to its parent, destruction sweeping stamped
  handles; per-process handle tables charged in pages (confirming the cost table's 128 handles per
  page, or changing it); 64-bit never-reused budget ids; `random` returning one `u64`.
- Accepted when: kernel cases for R6-R10 and I2, I5, I8, I10, I12; attack cases: carve beyond the
  parent, exhaust handle tables, destroy while handles are held elsewhere, forge a handle index, a
  user-class caller asking for `first`.
- Needs: WP-A1 (WP-A2 for the new `budget_create` and `random`).

**WP-K2. Endpoints and messages.** Size L.
- Reads: KERNEL-SPEC.md Endpoint, Process (open calls, the current call), Messages (abandoned-call
  notices), R1-R4b, R10 (messages in flight), `endpoint_create`, `mint`, `call`, `send`,
  `receive`, `reply`, `serve`, `handle_close`.
- Delivers: endpoints; the four IPC calls with lend and transfer; `receive` reporting a call or a
  send; open calls (up to `MAX_OPEN_CALLS` per process, each charged a page, its lend charged to the
  receiver too; at the limit no calls are taken, while sends, interrupts and notices still are);
  each thread's current call and `serve`; abandoned calls and their notices; `mint` with stamps,
  from an open call of the caller's thread; badge-0 receive rights; the label check against the
  endpoint's owner; fair waiting by (account, label set), and by budget for account 0, `WAIT_CAP`
  counting queued messages only; delivery only when the receiver can pay for everything the message
  brings, `Refused` to the sender otherwise; transfer opt-in; `Dead` for the calls a dying server
  had taken; R10's reach into messages in flight and into handles inside queued messages (K1 has no
  messages to test it with); message ids unique per receiving process.
- Accepted when: kernel cases for R1-R4b and I3, I4, I7, I9, I11, I15; attack cases: steal a receive
  right, mint badge 0, mint into a foreign budget, unequal-label call, 10,000 sender threads
  attempting to call with another account still served in turn, a vault-labelled sender filling its
  `WAIT_CAP` with its owner's unlabelled sender unaffected, two system senders in different budgets
  not sharing a `WAIT_CAP`, open calls beyond `MAX_OPEN_CALLS` with sends still delivered, 64 calls
  parked with short timeouts (each reported abandoned once, freed by its reply), reply to a send,
  `serve` on a call the thread does not hold, lender destroyed mid-call with the server surviving,
  unrequested transfer, a message the receiver cannot pay for (`Refused` to its sender; `receive`
  unaffected), a revoked handle's queued message and taken call (no reply handle reaches the
  sender), a revoked handle inside a queued message arriving as 0, `mint` from a `send`'s id.
- Needs: WP-K1, WP-A2.

**WP-K3. Device objects and interrupts.** Size M.
- Reads: KERNEL-SPEC.md Device, R5, R11, `map_device`, `dma_alloc`, `system_reset`; DEVICE-GRANTS.md.
- Delivers: the loader creating MMIO, IRQ and Reset objects from the device tree; IRQ receive with
  mask-on-fire; DMA allocation returning physical addresses.
- Accepted when: `uart-irq` rewritten on IRQ receive; attack cases: map an MMIO range without its
  handle, receive on an IRQ handle not held, name RAM by physical address.
- Also: `irq-attack` rewritten for IRQ handles, its verdict from a victim holding the handle;
  today its out-of-range and double-claim attempts prove only survival.
- Needs: WP-K1 (and WP-K2 for `receive`).

**WP-K4. Process creation and exit.** Size M.
- Reads: KERNEL-SPEC.md Process, `process_*`, exit notices, R10; PACKAGES.md (launching); INIT.md.
- Delivers: `process_create`/`process_map`/`process_start` (with `arg`, the startup page's address);
  the process object charged to its creator and holding the exit notice until it is received or
  dropped, freed with the creator's budget; a badge-0 exit endpoint; PIDs drawn at random from the
  free ASIDs; exit notices with cause, blamed account and blamed labels; a `process_exit` holding
  open calls reported `faulted`; blame by the failing thread's current call.
- Accepted when: kernel cases for exit notices (all three causes, and a labelled process's notice
  reaching a system-class `init`), blame on a server fault and on a panic with open calls going to
  the current call only (a thread holding several callers' calls blames one; after `serve`, the
  call it names), a `send` never blamed, a crash during event work blaming nobody, a panic in a
  thread without a current call blaming nobody while another thread holds calls, a creator's budget
  destroyed killing its children with no notice, a child finding its startup block through `arg`;
  attack cases: map into a started process, W+X through `process_map`, a handle list over
  `MAX_START_HANDLES`, a process in a weight-0 budget, a badged exit endpoint refused, creating and
  killing processes whose notices nobody receives (bounded by the creator's own budget);
  `bench-bundle-file` becomes a clean boot in which a guest reads its `[[file]]` entry back (today
  the loader refuses data entries).
- Also: `wx` re-based on exit notices (a checker launches the attacker and takes the verdict
  from the kernel's notice), replacing today's survival-only verdict.
- Needs: WP-K2, WP-A2.

**WP-K5. Timer, timeouts and preemption.** Size M.
- Reads: KERNEL-SPEC.md R12, timeouts, deadlines; RESOURCES.md.
- Delivers: the kernel-owned timer; timeouts on `call`/`send`/`receive`; budget deadlines enforced;
  stride scheduling over budgets, `first` budgets before all others; `rdtime` readable from user
  mode.
- Accepted when: a spinning budget cannot delay another beyond its weight; a sleeping-waking budget
  cannot exceed its share; a system-class server without `first` busy on one user's requests delays
  other users only by its weight; every blocking call returns by its timeout (I13); a deadline
  destroys its budget; the old IRQ-0 timer path and `timer` case are gone.
- Needs: WP-K2.

**WP-K6. Delete the legacy interface.** Size M.
- Delivers: removal of SID connects, scalar message kinds, `ClaimInterrupt`, the `grants` entry,
  name lookup and `PlatformSpecific` timer calls; old test programs migrated or deleted.
- Accepted when: the bench passes with no legacy calls; the `unsafe` ratchet is lower than before
  WP-K1; kernel line count reported.
- Needs: WP-K1 to WP-K5, WP-R1b.

**WP-C1. Model conformance.** Size M.
- Reads: KERNEL-SPEC.md, errors and the order of checks (the model conforms to them).
- Delivers: a bench case replaying WP-M1 traces on the real kernel and comparing every result
  (usage compared in the model's placement profile, R11); a system-call fuzzer program (random and
  hostile arguments; I14).
- Accepted when: 10^5 model traces replay with identical results; the fuzzer runs
  for its budget with no kernel panic.
- Needs: WP-M1, WP-K1 to WP-K5, WP-T1.

### Track R: user runtime and system servers
**WP-R1. Native runtime crate.** Size M.
- Delivered (merged, `8298608af`, with everything from answers 39-42 and 50-53): `redoubt-rt`:
  startup-block parsing and writing as INIT.md states it, found through `arg`; typed handle
  wrappers, `call`/`send`/`receive` helpers, an allocator over `map_anon`, a panic handler; the
  shared server library (CONTAINMENT.md): `admit(badge, account, labels)` (per (account, label
  set), per badge for account 0), `check(caller_labels, object_labels, read|write)` (read: object
  ⊆ caller; write: equal), a 9P server skeleton (fids keyed by (badge, account, label set), `..`
  kept inside the root, walks, qids, `stat` and directory reads checked as reads, a 9P call with
  non-zero words or no lend refused with `Malformed`, a hook freeing a badge's fids and slots when
  its badge notice arrives) and typed-message dispatch (WIRE.md, status 1 = `Malformed`).
- Accepted when: host tests against a fake kernel; a bench echo server and client use only this
  crate; an rv32 and rv64 build case; fuzz targets.
- Needs: WP-A1, WP-W1 (and WP-K2 to run on the kernel). Merged.

**WP-R1b. The runtime follows answers 64 and 69-101.** Size M.
- Reads: CONTAINMENT.md (the shared server library), INIT.md (Startup block), NAMESPACES.md
  (`ninep_common`), WIRE.md, KERNEL-SPEC.md (`receive`'s record, abandoned-call notices, `serve`).
- Delivers: in `redoubt-rt`: the startup block as the `startup` typed message (answer 75),
  replacing the tag-and-CRC format, with handle names under the manifest's name rule (answer 64);
  `ninep_common` (`new_connection` with a random connection id, `disconnect` freeing a connection
  and everything minted under it) served by the 9P skeleton, its table and INIT.md's `startup`
  table unfenced and generated (answers 69, 83); the badge-notice hook removed (answer 69); `serve`
  before resuming a parked call, and an immediate reply to an abandoned-call notice (answers 81,
  82); handles a request carries that the protocol did not ask for closed (85); caps sized so every
  bucket at its cap fits the server's budget and the open calls they allow sum to less than
  `MAX_OPEN_CALLS` with headroom, and a server-side deadline for parked calls (81, 85); a fair share
  per badge within a bucket (90); badge numbers never reused (86).
- Accepted when: host tests for each change (a disconnect freeing its fids and every connection
  minted under it; a stranger's id refused; an unasked handle closed; an agent flooding a bucket
  leaving its sponsor's share; a parked call resumed under `serve`); the startup-message fuzz
  target.
- Needs: WP-R1, WP-A2, WP-W2.

**WP-R2. Loader stub.** Size S.
- Reads: PACKAGES.md (launching), INIT.md (startup block).
- Delivers: the flat-binary stub at its fixed address: find the ELF image through the startup
  block's image fields (defined here and added to INIT.md's `startup` message, answer 65), parse it
  from memory, map segments (code executable and read-only), free the image, jump, passing on the
  startup page's address it was started with (`arg`).
- Accepted when: fuzzed ELF images never escape the child (the child faults or exits; nothing else
  is affected); attack case: a hostile ELF from a user parent hurts only the child.
- Needs: WP-R1b, WP-K4.

**WP-R3. init and the boot manifest.** Size M.
- Reads: INIT.md (all), WIRE.md (JSON).
- Delivers: `init`: reads the manifest (refusing names outside INIT.md's name rule, and any grant of
  a server's own budget), builds the budget tree (`first` for itself, the steward and the drivers),
  hands out device handles, starts every system server through the stub, restarts with the rate
  limit, passes crash blame by (account, label set) to the steward (the typed message whose table
  WP-S2 writes), reboot as last resort; the boot loader loading only the kernel and `init`, once
  `init` can start every bundle program through the stub.
- Accepted when: the milestone 1 manifest boots every server; a manifest with a bad name, or one
  granting a server its budget, is refused; no server's startup block holds a budget handle; a
  crashing server restarts on the same endpoint; blame case: 3 crashes blamed on one (account,
  label set) produce the steward signal naming that (account, label set) and no other (a vault
  session's crashes name its label set, not its owner's empty one; what the steward then destroys
  is WP-S2's case); more than 5 restarts in 60 s reboots.
- Needs: WP-R2, WP-W1, WP-K3, WP-K5.

**WP-R4. bootfsd and consoled.** Size S.
- Delivers: `bootfsd` (read-only 9P over the verified bundle); `consoled` (UART driver serving
  `/dev/cons` over 9P, IRQ receive); both serve `ninep_common` through the skeleton.
- Accepted when: 9P conformance vectors from WP-W1; typing on the UART reaches a 9P reader.
- Needs: WP-R1b, WP-K3.

### Track B: beamlet on Redoubt (parallel with track K once WP-R1 exists)
**WP-B1. The Redoubt platform for beamlet.** Size M.
- Reads: beamlet DESIGN.md (I/O), PACKAGES.md.
- Delivers: a `no_std` `Platform` over `redoubt-rt`: console over `/dev/cons`, monotonic and wall
  time, randomness (`random`), module loading through `bootfsd`, the asynchronous 9P client on a small
  pool of I/O threads.
- Accepted when: **Elixir prints on the box** (PLAN.md's milestone 1, step 2); beamlet's differential
  suite subset runs on the box with identical output.
- Needs: WP-R1b, WP-R4.

**WP-B2. IEx on the UART console.** Size S.
- Delivers: an IEx session on the UART; the first Redoubt IEx helpers (`ls`, `cd`, `cat` over 9P).
- Accepted when: a bench case types expressions at IEx and checks the answers (PLAN.md's
  milestone 1, step 3).
- Needs: WP-B1, WP-R3.

### Track D: storage and network
**WP-D1. blkd.** Size M. virtio-blk driver (the `virtio-drivers` crate), partition table,
block-range handles, validation of every ring index and length.
- Accepted when: block round trips; a hostile-device model (malformed rings) never corrupts other
  memory or panics blkd; `blkd`'s contract (IO-ARCHITECTURE.md): a flush on every `sync`, in-order
  completion, whole-sector writes.
- Needs: WP-R1b, WP-K3.

**WP-D2. fsd.** Size M. 9P over littlefs on a block range; one label set per volume from the
manifest; a byte quota per attach root; `admit` and `check` on every request (writes need equal
labels; a walk or `stat` is a read; directory reads list only readable entries); relies only on
`blkd`'s contract (IO-ARCHITECTURE.md).
- Accepted when: 9P conformance; per-volume label cases (read up, write down and write up all
  refused, `Tcreate` in a labelled directory from an unlabelled caller revealing nothing);
  admission per (account, label set), and a client's fids freed by its launcher's `disconnect`; a
  byte quota per attach root (Bob filling the volume does not fail Alice's saves); a remove while
  another connection holds a fid succeeds; the no-leaky-state observer sees no change, qid versions
  included, from a vault writer.
- Needs: WP-D1, WP-L1, WP-R1b.

**WP-D3. netd and ipd.** Size L. virtio-net driver; `ipd:lan` on `smoltcp` serving `/net` over
9P with IP-prefix-and-port capabilities that never include the box's own addresses; refuses
labelled callers.
- Accepted when: TCP connect and listen through `/net`; attack cases: connect outside the granted
  prefix, connect to the box's own address, a labelled caller refused. The bench's network is
  QEMU user mode with `restrict=on` (no outside peer); this package adds the peer it needs to the
  bench, with a self-check that the guest reaches nothing else.
- Needs: WP-R1b, WP-K3, WP-W2.

### Track S: security servers
**WP-S1. keyd.** Size S. Holds keys; signs through a badge-scoped capability naming one key and one
purpose (for SSH, a signature over the session identifier `keyd` computes itself), never arbitrary
bytes; never holds keys that authenticate a person to the box; constant-time signing.
- Reads: INIT.md (keyd), CAPABILITIES.md (the powerbox and approvals, agents), CONTAINMENT.md
  (covert and timing channels: constant time).
- Accepted when: a signature round-trips through a badge-scoped handle; attack cases: a caller
  cannot sign with a key or for a purpose its badge does not name, a request to sign arbitrary
  bytes (a relayed SSH user-auth blob) is refused, no export operation exists, enrolling a login
  key is refused; the signing path is constant-time under the bench's timing check.
- Needs: WP-R1b.

**WP-S2. steward (stateless, milestone 1).** Size L.
- Reads: CAPABILITIES.md, CONTAINMENT.md, INIT.md, PACKAGES.md (launching).
- Delivers: principals and SSH keys from the manifest; each principal's budget split at boot into
  fixed sub-budgets, one per label set the manifest names; session budgets and namespaces (a fresh
  connection per child, disconnected at logout and lease expiry); launching beamlet VMs through the
  stub; vault sessions (`alice+X`); leases for agents, at most `MAX_LEASE` (longer requests
  refused), with sub-agents inside the agent's budget and `keys` only when the approval named the
  key; servers given narrowing handles only as revocation scopes made for them, never a budget that
  holds processes or a system-class budget; the powerbox with rendering (printable-ASCII whitelist,
  the requester's kind and steward-assigned name, only steward-generated text for labelled
  requesters), binding, caps per (account, label set) with a fair share per badge, ending a lease
  accepted from the sponsor ahead of admission, notifications only to channels whose labels ⊇ the
  request's, and random ids; audit records carrying the request's labels, read under `check`;
  declassification by snapshot, the steward `call`ing a short-lived reader budget with the item's
  labels, which fills its lend; on the third blamed crash of an (account, label set), every budget
  of it destroyed and new sessions refused until the window passes; the typed message by which
  `init` reports blame, its table written into INIT.md; the work of any one request bounded;
  `check` on its own records.
- Accepted when: the model's policy traces (WP-M0) replay against it; approval cases (bidi and
  format characters escaped, swapped requests refused, labelled requests shown only to label
  owners and without their free text, their notifications reaching no unlabelled channel, floods
  capped); a lease request over `MAX_LEASE` refused; a sub-agent dies with its agent's lease; no
  reader budget outlives its declassification; an agent flooding the steward and `fsd` does not
  stop Alice opening a file and ending its lease; after three blamed crashes Bob's sessions and
  leases of that label set are gone and a new login is refused within the window; no server can
  destroy a session; a vault session's leases do not change the unlabelled sub-budget's free limits.
- Needs: WP-R3, WP-B1, WP-D2.

**WP-S3. sshd.** Size M. `sunset`-based; host key through `keyd`; user authentication through the
steward; rejects keys `keyd` holds; each channel labelled with its session's labels, a labelled
channel being a pty session with no forwarding, subsystems or `exec`; `ssh approve@box` (sharing
this `sshd` in milestone 1, a stated residual).
- Accepted when: `alice@`, `alice+secrets@`, `bob@` and `approve@` sessions work from the bench's
  SSH client, with the box's host key pinned (`net.host_key`); loopback login with a `keyd` key
  refused. The bench's loopback self-checks log in as one host user, so per-user separation is
  first tested here.
- Needs: WP-D3, WP-S1, WP-S2.

### Track E: the milestone
**WP-E1. Alice's agent and the attack suite.** Size M.
- Delivers: the scripted hostile agent and the scripted hostile user (Bob), and every case in
  PLAN.md's milestone 1 attack suite not already delivered by the packages above, each asserted
  through the system (How to read a work package).
- Accepted when: the whole suite passes, and **milestone 1 is declared done** in STATUS.md.
- Needs: everything above.

## Order
```
merged:                  W1  L1  T1  T1b  A1  K0  R1
start now, in parallel:  M0 -> M1;  A2 (after A1);  W2 (after W1)
kernel, serialized:      K1 -> K2 (after A2) -> K3 -> K4 -> K5 -> K6 (after R1b)
runtime:                 R1 -> R1b (after A2, W2) -> R2 (after K4) -> R3 (after K3, K5)
                         R4 (after R1b, K3)
beamlet:                 B1 (after R1b, R4) -> B2 (after R3)
storage and network:     D1 (after R1b, K3) -> D2 (after L1);  D3 (after R1b, K3, W2)
security:                S1 (after R1b);  S2 (after R3, B1, D2);  S3 (after D3, S1, S2)
conformance:             C1 (after M1, K5, T1)
milestone:               E1 (after all)
```
**The owner's round-4 answers** (QUESTIONS.md 56-101), which A2, W2, M1, R1b and K2 waited on, are
in the notes. The critical path is the kernel track (K1 to K5), then R3, S2 and S3;
A2 must land before K2.
Everything off that path (model, codecs, littlefs, bench, drivers, beamlet's platform) can proceed
in parallel. SWARM.md's waves follow this order.

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
