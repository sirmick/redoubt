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
- **The owner's answers 1-126** (QUESTIONS.md) are in the notes; nothing is open. A package built
  before an answer that changed it gets a follow-up package rather than a silent edit (WP-A2, WP-W2,
  WP-W3, WP-M1, WP-R1b, WP-V1); WP-A3 was folded into WP-K2, so it has no branch of its own.
## Work packages

### Track M: model and contracts (start immediately, in parallel)
**WP-M0. Executable security model.** Size L.
- Reads: KERNEL-SPEC.md (all), CONTAINMENT.md, CAPABILITIES.md (steward policy parts used in
  milestone 1).
- Delivers: `model/`: a host Rust crate implementing every object, system call, error and
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
  swept), R11, R12 (one stride queue over every budget, no class or flag ordering: answer 103);
  `MAX_HANDLES` and the handle table charged per table page in use (questions 102, 111, 116); open
  calls, the current call and `serve`; exit notices with `blamed_labels`, held by the process
  object charged to its creator; a budget's own page charged to its parent; class inherited;
  `random` as one `u64`; message ids per receiving process and random PIDs; a badge-0 exit
  endpoint; I2, I5 (unconditional), I7, I8, I10, I11, I12, I15 (new).
  CONTAINMENT.md (`check`, metadata as reads, `admit` per badge for account 0 with a fair share per
  badge, reader budgets answering a call, blame per (account, label set) ending every budget of it,
  fixed sub-budgets per label set); CAPABILITIES.md (`MAX_LEASE` as steward policy, nested
  sub-agents, the approval screen, narrowing handles as revocation scopes, `keys` in leases).
- Delivers: `model/` updated: the README's interpretation choices the spec has now settled
  changed to match or marked settled (6 and 7 change: a `send` is never served or blamed; 8, 10,
  14, 20, 23, 24 stand), and the open questions it listed closed; mutations for each new rule (an
  abandoned call never reported, a lend charged to one side only, a delivery failing `receive`
  instead of `Refused`, blame of a call other than the current one, blame falling back to another
  thread's call, a revoked handle inside a queued message delivered, a badged exit endpoint
  accepted, a class argument honoured, a budget scheduled ahead of the queue, a handle table
  charged by its highest handle rather than by the pages in use, handles past `MAX_HANDLES`
  admitted).
- Accepted when: as WP-M0, at 10^6 sequences, adding I15 and the new mutations.
- Needs: WP-M0.

**WP-W1. Wire codecs.** Size M.
- Reads: WIRE.md.
- Delivers: `libs/wire/`: a `no_std` 9P2000 codec; the typed-message table format and a
  generator emitting Rust and Elixir codecs; a strict JSON (I-JSON) parser profile for manifests.
  Fuzz targets for all three.
- Accepted when: round-trip tests; fuzzing finds no panics or hangs (1 hour each in CI-equivalent
  runs); the Elixir codec round-trips the same vectors on beamlet.
- Needs: nothing. Merged.

**WP-W2. Wire generator follows answers 28, 41, 42, 56 and 98.** Size S.
- Reads: WIRE.md (Tables, Layout in a message).
- Delivers: `libs/wire/`: `handle[N] KIND` in the table format, the kind in the generated docs
  only (answer 56: kinds are checked by use, `WrongObject` on first use); code 1 `Malformed`
  reserved and added to every error table (`tables/example.md`'s code 1 renumbered); a table giving
  code 1 its own meaning refused; no `kind` column (every milestone 1 typed message is a `call`,
  answer 98).
- Accepted when: the drift test covers kinds and `Malformed`; a table with an unknown kind or its
  own code 1 fails generation; the Elixir codec round-trips the new vectors.
- Needs: WP-W1. Merged (`3715363a9`).

**WP-W3a. The 9P opcode floor, and the `copy_file` rename (answers 113, 155).** Size S.
- Reads: WIRE.md (Messages, Tables).
- Delivers: in `libs/wire/gen/`: the `<!-- wire: NAME ninep -->` marker for a protocol served on a 9P
  endpoint, whose opcodes must start at 16 (`ninep_common` reserves 1-15 there); a marked table using
  a lower opcode is refused by the generator. The `fsd` typed-operations table (NAMESPACES.md) is a
  marked table and must still generate, which needs answer 155's rename (`copy` -> `copy_file`,
  because `copy` camel-cases to the Rust type `Copy`, which every generated type derives).
- Accepted when: the drift test covers the marker; a marked table with an opcode below 16 fails
  generation, a marked table at 16 passes, and an unmarked table still starts at 1; and
  `cargo test -p redoubt-wire-gen` is green, including the `fsd` table, on `redoubt`.
- Needs: WP-W2. Carries answers 113 (the opcode floor) and 155 (the `copy_file` rename).
  **WP-W3b is deleted** (answer 154): answer 115 was already satisfied — `redoubt-rt`'s records are
  `Record([0; N])`, already backed, and the untouched-page assertion already lives in the
  `budget-syscall-attack` (WP-K1) and `lend-untouched-page` (WP-K0) cases.

**WP-A1. System call ABI crate.** Size S.
- Reads: KERNEL-SPEC.md (system calls, messages, errors).
- Delivers: `redoubt-sys`: call numbers, argument and result register encodings (one layout for
  both widths), the error enum, and a host round-trip test for every call and error (like `redoubt`'s
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
- Needs: WP-A1. Before WP-K2 and WP-R1b use the new records. Merged (`c98034520`); `budget_create`'s
  `first` flag, which it added, is removed again by WP-A3.

**WP-A3. The ABI drops `first` (answer 103).** Size S.
- Reads: KERNEL-SPEC.md (`budget_create` and its error row, Budget, R12, I8).
- Delivers: `redoubt-sys`: `budget_create`'s record without the `first` flag — the slot is removed
  and the fields after it move up, so the record is one slot shorter — and its error row without
  `ClassDenied`; `MAX_HANDLES`'s `TooLarge` on a call that would exceed it, and `OutOfMemory` on a
  reply whose handles do not fit (questions 102, 107, 116); the debug assertions that check each
  call's errors updated to match.
- Accepted when: round-trip and malformed-input tests for `budget_create`'s record; no encoding
  carries a priority or class argument; the fuzz target rerun; names match WP-M1.
- Needs: WP-A2. Before WP-K2, which removes the flag from the kernel. **Folded into WP-K2** (no
  separate package): the flag is gone from `redoubt-sys` and the kernel together, so there is no A3
  row in SWARM.md's claims table (HISTORY.md, WP-K2).

**WP-L1. littlefs in pure Rust.** Size L.
- Reads: NAMESPACES.md (filesystem section), littlefs `SPEC.md`.
- Delivers: `libs/littlefs/`: `no_std` littlefs over a block trait; custom attributes; host tests.
- Accepted when: differential tests against the C reference **on the host only** (images written by
  either read identically in the other); crash injection at every block write leaves a mountable,
  consistent image; fuzzed images never panic.
- Needs: nothing. Merged. It relies only on `blkd`'s contract (IO-ARCHITECTURE.md, Storage).

**WP-T1. Test bench extensions.** Size M.
- Delivers: in `tools/testbench`: an SSH client driving sessions over QEMU port forwarding;
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

**WP-V1. The bundle signing domain (answer 120).** Size S. (Emerged from WP-S1.)
- Reads: VERIFIED-BOOT.md (Signature, Testbench).
- Delivers: the loader verifies the boot bundle's signature over
  `"redoubt.bundle.v1\0" || u64_le(len) || tar`, with `len` taken from the initrd it is reading, and
  never over the bare archive (`loader/src/verify.rs`); the bench's signing path builds the same
  preimage (`tools/testbench`, the bundle builder), so the two change together — nothing boots if
  only one does; a case that signs the bare archive, with no domain and no length.
- Accepted when: every bench case still boots; the bare-archive case is refused by the loader and
  the machine powers off, as `verified-boot-rejects-tamper` does; `tamper_bundle` still fails; a
  signature made over a preimage with another domain, or with the wrong `len`, is refused.
- Needs: nothing. Changes what ships (the loader is TCB), so it lands on its own, before any
  production key exists.

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
  recorded (enforced by WP-K5), class inherited from the parent, a budget's own page charged to its
  parent, destruction sweeping stamped handles; per-process handle tables charged in pages
  (confirming the cost table's 128 handles per page, or changing it), at most `MAX_HANDLES` a
  process (question 102); 64-bit never-reused budget ids; `random` returning one `u64`.
- Accepted when: kernel cases for R6-R10 and I2, I5, I8, I10, I12; attack cases: carve beyond the
  parent, exhaust handle tables, a table filled to `MAX_HANDLES` refused with `TooLarge`, destroy
  while handles are held elsewhere, forge a handle index.
- Needs: WP-A1 (WP-A2 for the new `budget_create` and `random`). Merged (`e1d2c6216`); the `first`
  flag it recorded is removed by WP-K2 (answer 103).

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
  brings, `Refused` to the sender otherwise, handles that would take the receiver past
  `MAX_HANDLES` among the costs it cannot pay, and a reply's handles that do not fit the caller
  dropped (0 in their slots) with the reply delivered and `OutOfMemory` returned (questions 107,
  116); transfer opt-in; `Dead` for the calls a dying server had taken; R10's reach into messages in
  flight and into handles inside queued messages (K1 has no messages to test it with); message ids
  unique per receiving process.
- Also: the `first` flag removed from the kernel's `budget_create` (answer 103; K1 built it, and K2
  removes it from the kernel and `redoubt-sys` together, so **WP-A3 is folded in** and has no branch
  of its own). Nothing schedules on class or on a flag; R12's one queue is WP-K5's.
- Accepted when: kernel cases for R1-R4b and I3, I4, I7, I9, I11, I15; attack cases: steal a receive
  right, mint badge 0, mint into a foreign budget, unequal-label call, 10,000 sender threads
  attempting to call with another account still served in turn, a vault-labelled sender filling its
  `WAIT_CAP` with its owner's unlabelled sender unaffected, two system senders in different budgets
  not sharing a `WAIT_CAP`, open calls beyond `MAX_OPEN_CALLS` with sends still delivered, 64 calls
  parked with short timeouts (each reported abandoned once, freed by its reply), reply to a send,
  `serve` on a call the thread does not hold, lender destroyed mid-call with the server surviving,
  unrequested transfer, a message the receiver cannot pay for (`Refused` to its sender; `receive`
  unaffected), a message whose handles would take the receiver past `MAX_HANDLES` (`Refused`), a
  caller at `MAX_HANDLES` getting its reply without its handles and `OutOfMemory`, a revoked
  handle's queued message and taken call (no reply handle reaches the sender), a revoked handle
  inside a queued message arriving as 0, `mint` from a `send`'s id;
  `budget_create` takes no flag, so no argument of any call asks to run ahead of the queue.
- Needs: WP-K1, WP-A2.

**WP-K3. Device objects and interrupts.** Size M.
- Reads: KERNEL-SPEC.md Device, R5, R11, `map_device`, `dma_alloc`, `system_reset`; DEVICE-GRANTS.md.
- Delivers: the loader creating MMIO, IRQ and Reset objects from the device tree; IRQ receive with
  mask-on-fire; DMA allocation returning physical addresses.
- Accepted when: `uart-irq` rewritten on IRQ receive; attack cases: map an MMIO range without its
  handle, receive on an IRQ handle not held, name RAM by physical address.
- Also: `irq-attack` rewritten for IRQ handles, its verdict from a victim holding the handle;
  today its out-of-range and double-claim attempts prove only survival.
- Also: `map_anon`, `unmap`, `set_flags` (R11) on the Redoubt path, replacing the legacy memory
  calls. (Until a call's package runs, it decodes and returns `InvalidArgument`: WP-K1.)
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
- Also: `thread_create`, `thread_exit`, `process_exit`; the R8, R9 and I8 cases WP-K1 could not
  stage (a user-class caller needs `process_create`).
- Needs: WP-K2, WP-A2.

**WP-K5. Timer, timeouts and preemption.** Size M.
- Reads: KERNEL-SPEC.md R12, timeouts, deadlines; RESOURCES.md.
- Delivers: the kernel-owned timer; timeouts on `call`/`send`/`receive`; budget deadlines enforced;
  **one stride queue over every runnable budget** — no priority tier, no class ordering, no flag
  (answer 103) — with a waking budget re-entering at the current minimum pass; `rdtime` readable
  from user mode.
- Accepted when: a spinning budget cannot delay another beyond its weight; a sleeping-waking budget
  cannot exceed its share; a system-class server busy on one user's requests delays other users only
  by its weight; **a driver woken by an interrupt runs within about one `SLICE` while user budgets
  spin** (the stated cost of one queue, RESOURCES.md), and a large-weight server keeps its share
  under that load; every blocking call returns by its timeout (I13); a deadline
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
- Needs: WP-R1, WP-A2, WP-W2. Merged (`86117e7af`).

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
- Reads: INIT.md (all), WIRE.md (JSON), TENETS.md (The use case), CONTAINMENT.md (the channel table).
- Delivers: `init`: reads the manifest (refusing names outside INIT.md's name rule, and any grant of
  a server's own budget; refusing a manifest that gives `keyd` a principal's login or approval key
  or the key the loader verifies the bundle with, which it asks `keyd` for with `holds`, answers
  120 and 122-123; passing each server's arguments through unchanged, validating only their count,
  length and encoding; passing the `public` list to `bootfsd`, and refusing a `public` list naming
  the manifest itself or an entry the bundle does not hold), builds the budget tree with the
  manifest's weights (large for itself, the steward and the drivers; ordinary for the servers that
  work for users: answer 103), hands out
  device handles, starts every system server through the stub, restarts with the rate
  limit, passes crash blame by (account, label set) to the steward (the typed message whose table
  WP-S2 writes), reboot as last resort; the boot loader loading only the kernel and `init`, once
  `init` can start every bundle program through the stub.
- Accepted when: the milestone 1 manifest boots every server, each in a budget carrying the weight
  the manifest names; a manifest with a bad name, or one granting a server its budget, is refused;
  attack cases: a manifest giving `keyd` the bundle key stops the boot (the `holds` answer, not a
  derivation in `init`), and so does one giving `keyd` a principal's login or approval key; a
  manifest whose `public` list names the manifest, or an entry the bundle lacks, is refused;
  arguments reach their server byte for byte, and a count or length that would overflow the startup
  page is refused; no server's startup block holds a budget handle; a crashing server restarts on
  the same endpoint; blame case: 3 crashes blamed on one (account,
  label set) produce the steward signal naming that (account, label set) and no other (a vault
  session's crashes name its label set, not its owner's empty one; what the steward then destroys
  is WP-S2's case); more than 5 restarts in 60 s reboots.
- Needs: WP-R2, WP-W1, WP-K3, WP-K5. The `holds` operation it calls belongs to `keyd`'s table
  (WP-S1), so the refusal is written here and exercised end to end once WP-S1 has landed.
- **Confinement (answers 152-153; TENETS.md, The use case; CONTAINMENT.md, Push and the channel
  table; GAME.md, setup).** The manifest gains the `confined` flag (INIT.md, The boot manifest): one
  top-level boolean for the whole boot. `init` compares **label sets** and refuses the boot when two
  entries with differing sets share a `servers` entry, a `volumes` entry, an endpoint name in
  `receives`/`handed`, an `ipd:*`/`netd` instance, or a core, and when a labelled domain would read a
  shared unlabelled volume. Refusal is a boot failure, not a warning. Attack cases: a `confined`
  manifest placing a labelled and an unlabelled domain on one `fsd` instance is refused (the verdict is
  the boot failing, not the manifest's claim), and one placing them on one `ipd` instance, one endpoint
  or one core is refused likewise.

**WP-R4. bootfsd and consoled.** Size S.
- Delivers: `bootfsd` (read-only 9P over the verified bundle), serving **only the bundle entries
  the manifest's `public` list names** (the list arrives as its arguments from `init`), matched
  byte for byte, never the manifest (answer 123); `consoled` (UART driver serving
  `/dev/cons` over 9P, IRQ receive); both serve `ninep_common` through the skeleton.
- Accepted when: 9P conformance vectors from WP-W1; typing on the UART reaches a 9P reader; attack
  case: a session walking `/boot` sees only the public entries, and a walk to the manifest's own
  name is refused exactly as a name the bundle never held.
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
manifest; **the byte quotas, metered here and nowhere else** (question 118): `new_connection`'s
`quota` carved from the granter's root through the shared library's grant hook, and returned
through its disconnect hook, with the library holding no byte counters; `admit` and `check` on
every request (writes need equal labels; a walk or `stat` is a read; directory reads list only
readable entries); typed `rename`, `copy`, `get_attr` and `set_attr` for within-volume operations
9P2000 does not express; relies only on
`blkd`'s contract (IO-ARCHITECTURE.md).
- Accepted when: 9P conformance; per-volume label cases (read up, write down and write up all
  refused, `Tcreate` in a labelled directory from an unlabelled caller revealing nothing);
  admission per (account, label set), and a client's fids freed by its launcher's `disconnect`; a
  byte quota per attach root (Bob filling the volume does not fail Alice's saves; a quota larger
  than the granter's free quota is `refused`, and a `disconnect` gives it back); a remove while
  another connection holds a fid succeeds; the no-leaky-state observer sees no change, qid versions
  included, from a vault writer.
- **Confinement (CONTAINMENT.md, channel table).** One `fsd` instance per volume, and a volume carries
  one label set, so two differing label sets are never served by one instance; a confined deployment
  gives each trust domain its own volume and instance. Attack case: an unlabelled connection cannot
  see, read or time-change anything in a labelled volume's instance (the no-leaky-state observer,
  extended to a second instance).
- Needs: WP-D1, WP-L1, WP-R1b.

**WP-D3. netd and ipd.** Size L. virtio-net driver; `ipd:lan` on `smoltcp` serving `/net` over
9P with IP-prefix-and-port capabilities that never include the box's own addresses; refuses
labelled callers.
- Accepted when: TCP connect and listen through `/net`; attack cases: connect outside the granted
  prefix, connect to the box's own address, a labelled caller refused. The bench's network is
  QEMU user mode with `restrict=on` (no outside peer); this package adds the peer it needs to the
  bench, with a self-check that the guest reaches nothing else.
- **Confinement (CONTAINMENT.md, channel table).** One `ipd` per network or trust domain, so a stack bug
  reached from one domain cannot touch another's; a confined deployment gives a labelled domain no
  `/net` at all (a sink refuses labelled callers).
- Needs: WP-R1b, WP-K3, WP-W2.

### Track S: security servers
**WP-S1. keyd.** Size S. Holds keys; signs through a badge-scoped capability naming one key and one
purpose (for SSH, a signature over the session identifier `keyd` computes itself), never arbitrary
bytes; never holds keys that authenticate a person to the box; constant-time signing. Its keys in
milestone 1 are the SSH host key and the steward's `audit` key; **no session and no lease holds
`keys`** (answer 124), since `grant` mints only the granter's own key and purpose and neither of
those is a principal's: a principal's key, with the one message shape it may sign, is milestone 2.
- Reads: INIT.md (keyd, the boot manifest's arguments), CAPABILITIES.md (the powerbox and
  approvals, agents 7), CONTAINMENT.md (covert and timing channels: constant time; minted badges),
  WIRE.md (granting and releasing).
- Delivers, besides signing: `grant` and `release` in WIRE.md's shape, written into `keyd`'s table;
  `holds(public key)`, answered yes or no, which is how `init` refuses a manifest that hands `keyd`
  the bundle key without deriving a public key itself (INIT.md, answer 120); its keys taken as the
  manifest arguments `name,purpose,seed`, defined in `keyd`'s own note (answer 122); its first
  minted badge drawn at random above 2^63 (answer 126, with the 9P skeleton, which changes with it).
- Accepted when: a signature round-trips through a badge-scoped handle; attack cases: a caller
  cannot sign with a key or for a purpose its badge does not name, a request to sign arbitrary
  bytes (a relayed SSH user-auth blob) is refused, no export operation exists, enrolling a login
  key is refused, a `release` of an id the caller never received is refused like one that does not
  exist, and a grant released with its parent leaves nothing usable behind; two boots of the same
  bundle mint different badges; the signing path is constant-time under the bench's timing check.
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
  labels, which fills its lend; each audit record signed through a `keyd` grant for the `audit`
  purpose and its signature stored beside it in the file (answer 125; the steward holds a grant,
  never a key, and verification is an operator tool in milestone 2); on the third blamed crash of
  an (account, label set), every budget of it destroyed and new sessions refused until the window
  passes; the typed message by which
  `init` reports blame, its table written into INIT.md; the work of any one request bounded, since
  the steward's promptness now rests on its large manifest weight in the one queue and on nothing
  else (answer 103); `check` on its own records.
- Accepted when: the model's policy traces (WP-M0) replay against it; approval cases (bidi and
  format characters escaped, swapped requests refused, labelled requests shown only to label
  owners and without their free text, their notifications reaching no unlabelled channel, floods
  capped); a lease request over `MAX_LEASE` refused; a sub-agent dies with its agent's lease; no
  reader budget outlives its declassification; an agent flooding the steward and `fsd` does not
  stop Alice opening a file and ending its lease; after three blamed crashes Bob's sessions and
  leases of that label set are gone and a new login is refused within the window; every audit
  record in the file carries a signature that verifies against `keyd`'s audit key, and one byte
  changed in a record makes its signature fail; a logout and an ended lease still complete promptly
  while every user budget spins (its weight, not an order); no
  server can destroy a session; a vault session's leases do not change the unlabelled sub-budget's
  free limits.
- **Confinement (answer 153; CONTAINMENT.md, Push; TENETS.md, The use case).** For a confined
  deployment the steward never mounts a shared unlabelled volume into a labelled domain; input enters
  by an audited **push**, the mirror of declassification: one item, triggered by the target label's
  owner through the powerbox with an out-of-band approval, carried out through a short-lived writer
  budget holding exactly the target label set, and audited. A confined domain cannot trigger, name the
  item for, or pull a push. It keeps each agent its own label set by default. Attack cases: a labelled
  session's attempt to read down is refused with the push offered instead; a push moves exactly one
  item and no path, queue or batch remains; a confined domain cannot cause a push or observe its
  timing.
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
merged:                  W1  W2  L1  T1  T1b  A1  A2  K0  K0b  K1  K2  K3  R1  R1b  S1  V1
                         (A3 folded into K2)
in review:               M0/M1 (the executable model)
building:                K4 (kernel track);  R4, D1, D3
the ready set:           W3a (the 9P opcode floor and the `copy_file` rename);  K5 (behind K4 on the Hotspots)
kernel, serialized:      K4 -> K5 -> K6 (after R1b)
runtime:                 R4 (after R1b, K3);  R2 (after K4) -> R3 (after R2, W1, K3, K5)
beamlet:                 B1 (after R1b, R4) -> B2 (after B1, R3)
storage and network:     D1 (after R1b, K3) -> D2 (after D1, L1);  D3 (after R1b, K3, W2)
security:                S1 (after R1b) -> S2 (after R3, B1, D2);  S3 (after D3, S1, S2)
conformance:             C1 (after M1, K1-K5, T1)
milestone:               E1 (after all)
```
**The owner's answers 1-126** (QUESTIONS.md) are all in the notes; nothing is open. Answers
102-119 add W3 (since split, and W3b dropped) and change K2, K5, M1, R3, D2 and S2 (A3 was folded
into K2); answers 120-126 add V1
and change R3, R4, S1 and S2. The critical path is unchanged: the kernel track (K4 to K5), then R3,
S2 and S3. V1 is off the path and
depends on nothing, but it changes the loader and the bench's signing path together, so it is one
package and no other package may edit either half while it runs (Hotspots).
Everything off that path (model, codecs, littlefs, bench, drivers, beamlet's platform) can proceed
in parallel. SWARM.md's waves follow this order.

## Hotspots (serialize edits to these)
- `kernel/src/syscall.rs`, `kernel/src/services.rs`, `kernel/src/mem.rs`,
  `kernel/src/arch/riscv/process.rs`: only the one kernel package in progress edits them.
- `redoubt-sys` (the ABI): changed only with a KERNEL-SPEC.md change.
- `loader/src/verify.rs` and the bench's bundle builder (`tools/testbench`): the signature's
  preimage lives in both, so only WP-V1 edits either until it lands.
- The boot manifest format (INIT.md) and the typed-message tables (WIRE.md and each server's note):
  one owner each; changes go through the design notes first.
- `docs/`: design changes only with a HISTORY.md entry; STATUS.md updated by whoever
  finishes a package.

## Review of each package
Before a package is accepted, the round's reviewers read it the way they read the design: a red-team
reviewer attacking it against the spec and the attack suite, a simplifier looking for code to delete,
and an editor checking that the code, its comments and the notes agree. Findings are fixed or
recorded before the package is marked done.
