# Remaining milestone 1 work

Milestone outcome: Alice and Bob logged in over SSH on QEMU, separated, with Alice's agent
contained under a lease. [PLAN](PLAN.md) owns the product acceptance suite; [SWARM](SWARM.md#claims)
owns package state. Completed package recipes remain in git; the contracts live in their specifications.

Each package below states its contract, deliverables, acceptance and dependencies. Sizes are
S (hundreds of lines), M (roughly 1.5k), or L (larger). A passing host component is not a booted
service. Acceptance also requires the [shared checks and review](SWARM.md#roles-and-execution).
Attack verdicts come from the kernel, a victim or a trusted checker, not an attacker's claims.

Milestone 1 requires rv64 boots and rv32 compilation; existing rv32 boots add coverage.
The slice uses `no_std` + `alloc` Rust and Elixir; a Rust `std` target is later work.
Design changes need approval recorded once in [ANSWERS](ANSWERS.md) and applied to their owner.
Open decisions are in [QUESTIONS](QUESTIONS.md), including 163 (typed parking), 164–166
(security/latency claims), and device/lifecycle issues 127–149. No package resolves them by assumption.

## Work packages

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
  sub-agents, the approval screen, narrowing handles as revocation scopes, no `keys` in milestone-1 leases).
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

**WP-T1c. Fail-closed unsafe coverage (ASTRA C4).** Size S.
- Reads: TENETS.md 2/6, ASTRA.md C4 and WP-IPC1's verification gate. This repairs existing
  assurance machinery; it changes no frozen design or allowed unsafe budget.
- Delivers: correct moved source roots in `tests/unsafe-budget.toml`; make
  `tools/testbench/src/budget.rs` reject missing configured paths and configured roots with no
  Rust source, including explicit files; focused negative tests proving these mistakes fail.
  Keep normal zero-unsafe source valid and propagate filesystem errors with path context.
- Accepted when: regression tests distinguish a valid zero-unsafe crate from absent/empty roots;
  normal ratchet runs enumerate the actual production sources and report honest counts. Do not
  raise budgets to hide newly uncovered debt; report any pre-existing overage separately.
  The testbench host tests and its focused unsafe-budget case pass, with three review angles
  complete. No firmware, kernel or runtime implementation is owned by this package.
- Needs: WP-T1 (merged). Separate prerequisite for IPC1 acceptance; may run in parallel with
  IPC1's ABI/runtime work in its own worktree.

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

**WP-IPC1. Observable IPC ownership and delivery (answers 167-168).** Size L.
- Reads: ANSWERS.md 167-168; KERNEL-SPEC.md R3/R4/R4b, IPC outcomes, ABI and error/output
  validity; CAPABILITIES.md (the native runtime's IPC ownership contract); CONTAINMENT.md
  (shared server transaction cleanup); ASTRA.md C1/C2/C3/A1/D4.
- Delivers: the approved outcome encoding in `libs/sys`; matching kernel call/reply completion
  and rollback paths; a consuming lend API and delivered-partial-reply handling in `libs/rt`;
  delivery-aware grant/connection cleanup in the shared server library and `servers/keyd`;
  migration of affected native callers, server dispatch and tests. Update `model/` and its
  traces to the same result/output-validity contract. Owned paths are those components' IPC
  seams, their direct API consumers, and the relevant `tests/` and `tools/testbench` cases.
  This includes C3's initial input/output record validation, not a separate kernel fix;
  no unrelated process, scheduling or firmware redesign.
- Accepted when:
  - ABI tests round-trip every valid outcome on success and error, rejecting invalid result
    encodings, on both widths; existing error ordering and R3/R4/R4b ownership remain intact.
  - Runtime regressions cover queued cancellation with the buffer returned, taken-call
    abandonment with it consumed, server death with it returned, and normal reply. Reuse the
    former virtual address and prove that no stale safe buffer can access or unmap its replacement.
  - Partial reply-handle delivery preserves the valid reply and every surviving handle identity
    (or explicitly closes unused handles); no handle or admission charge is orphaned. Late output
    failure rolls back newly installed reply handles and exposes no valid reply record while
    still reporting lend ownership out of band.
  - An initially read-only call record is refused before the receiver sees a message; an invalid
    record takes precedence over a later invalid endpoint. Completion also protects output frames
    during copying: concurrent unmap/remap, permission changes, teardown and abandonment cannot
    redirect the write or publish incompatible outcomes. Test these races separately from the
    allowed changes after commit.
  - Actual grant/connection serving paths roll back provisional state on discard or failure of
    an operation's required handle delivery; partial-success policy and close ownership are
    explicit. Race abandonment against reply and verify counts return to their specified baseline.
  - Model traces cover the lifecycle table. Real-kernel cases cover cancellation before and
    after receipt, revocation, server death, partial delivery and output-record failure; real
    timer timeout cases run after K5. Verdicts come from the kernel or a separate trusted checker,
    not hostile-client output. Host substitutes do not satisfy this gate.
  - The full bench remains green, rv32 compiles, and the package has its own risk-bounded TCB
    round (red team, simplifier, editor). Do not rely on the false-green unsafe check in ASTRA C4:
    the separate C4 checker repair must correct configured roots and reject missing roots. Check
    coverage of IPC1's touched on-target ABI/kernel/runtime/server sources, explicitly listing any
    newly introduced production seam. Do not weaken existing budgets. Host-only model and test
    code uses its normal checks; this gate does not introduce a repository-wide unsafe policy.
- Needs to start: WP-A2, WP-K2, WP-R1c, WP-S1 (all merged). Kernel edits serialize behind
  the active WP-K4 and never overlap K5/K6; `libs/rt` edits serialize with WP-R1d and server ports.
  Model work coordinates with WP-M1's owner: its in-review state is not a merged dependency.
  **Completion gates:** WP-M1 integration, WP-K5 real timer support, and repair of ASTRA C4's
  verification gap. ABI/runtime work need not wait for those gates, but this package cannot be
  accepted or marked done without them. Filed after owner approval; not dispatched by that approval.

**WP-C1. Model conformance.** Size M.
- Reads: KERNEL-SPEC.md, errors and the order of checks (the model conforms to them).
- Delivers: a bench case replaying WP-M1 traces on the real kernel and comparing every result
  (usage compared in the model's placement profile, R11); a system-call fuzzer program (random and
  hostile arguments; I14).
- Accepted when: 10^5 model traces replay with identical results; the fuzzer runs
  for its budget with no kernel panic.
- Needs: WP-M1, WP-K1 to WP-K5, WP-T1, WP-IPC1 (traces include answers 167-168).

**WP-R1d. Let a typed call park (open question 163).** Size S.
- Reads: NAMESPACES.md (Holding a call), `libs/rt/src/server/typed.rs` and `ninep.rs`.
- Delivers: the park mechanism reaches the **typed** dispatch too, which today it does not. `serve_parking` hands a request back only when `answer_in_place` returns `Answer::Waiting`, and that comes only from `FileServer::read -> Read::Wait`; a typed opcode goes to the server's own dispatch, which is `Result<(), Error>` and must reply, and `Answer<R>` has no "wait" variant. Give the typed path the same hand-back: a way for a typed handler to say "hold this", routed through the same close-the-handles-and-empty-the-list path the read already uses. Nothing else changes: the parked call is still charged to the server's `Admission`, still reported abandoned, still resumed and re-read. One mechanism, two entry points.
- Accepted when: `redoubt-rt`'s host tests cover a parked **typed** call resumed and answered, one abandoned and freed, and a `Wait` from a typed handler refused by a plain `serve`; both widths build; the fuzz target reruns.
- Needs: WP-R1c. **Blocks the `resize` half of WP-B2a**; until it lands, B2a implements opcode 16 `size` and not 17 `resize`. `libs/rt` is shared, so its own review round (answer 157).

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
- Reads: INIT.md (all), WIRE.md (JSON), TENETS.md (Purpose and threat model), CONTAINMENT.md (the channel table).
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
- **Confinement (answers 152-153; TENETS.md, Purpose and threat model; CONTAINMENT.md, Push and the channel
  table; GAME.md, Setting up a match).** The manifest gains the `confined` flag (INIT.md, The boot manifest): one
  top-level boolean for the whole boot. `init` compares **label sets** and refuses the boot when two
  entries with differing sets share a `servers` entry, a `volumes` entry, an endpoint name in
  `receives`/`handed`, an `ipd:*`/`netd` instance, a **device object** (`devices`), or a core, and when
  a labelled domain would read a shared unlabelled volume. Refusal is a boot failure, not a warning.
  Attack cases: a `confined` manifest placing a labelled and an unlabelled domain on one `fsd`
  instance is refused (the verdict is the boot failing, not the manifest's claim), and one placing
  them on one `ipd` instance, one endpoint, one device or one core is refused likewise. (R-2 red team,
  2026-09-22: the device clause was the gap; the shared-volume tail is covered by the Push rule.)

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

**WP-B2a. The console library (answer 162).** Size S.
- Reads: NAMESPACES.md (The console), USERLAND-API.md (The console and the `Platform` contract).
- Delivers: the `consol` codec (`cargo run -p redoubt-wire-gen`); `consoled` answering the `size`
  call from its `cols,rows` manifest argument (default 80x24, the opaque-string rule of answer 122);
  `Redoubt.Console` and `Redoubt.Console.Key` in pure Elixir; the Redoubt platform's
  `console_size` via the `size` call (answer 162; `Some((cols, rows))` or `None`, never a silent
  80x24).
- Accepted when: a bench case asks `/dev/cons` for `size` and gets the manifest's size; a key
  sequence decodes to the right events; a server without `consol` yields `{:error, :unknown}`.
- Needs: WP-B2, WP-R4, WP-R1d (for `resize`; `size` does not need it).

**WP-B2b. Redoubt.Ed and Shell.top().** Size S.
- Delivers: `Redoubt.Ed` (a TUI editor over `/dev/cons`) and `Redoubt.Shell.top()`.
- Accepted when: a bench case opens `Ed` on `/dev/cons`, navigates with arrow keys, edits a line,
  saves and exits.
- Needs: WP-B2a. (Split from WP-B2 by answer 161: one acceptance per package.)

**WP-D2. fsd.** Size M. 9P over littlefs on a block range; one label set per volume from the
manifest; **the byte quotas, metered here and nowhere else** (question 118): `new_connection`'s
`quota` carved from the granter's root through the shared library's grant hook, and returned
through its disconnect hook, with the library holding no byte counters; `admit` and `check` on
every request (writes need equal labels; a walk or `stat` is a read; directory reads list only
readable entries); typed `rename`, `copy_file`, `get_attr` and `set_attr` for within-volume operations
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
- **Confinement (answer 153; CONTAINMENT.md, Push; TENETS.md, Purpose and threat model).** For a confined
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

**WP-E1. Alice's agent and the attack suite.** Size M.
- Delivers: the scripted hostile agent and the scripted hostile user (Bob), and every case in
  PLAN.md's milestone 1 attack suite not already delivered by the packages above, each asserted
  through the system (How to read a work package).
- Accepted when: the whole suite passes, and **milestone 1 is declared done** in STATUS.md.
- Needs: everything above.

## Order

Use [SWARM Claims](SWARM.md#claims) and each package's Needs above. K4's selective port and
model reconciliation are the next integration prerequisites; D1/R4 source must not be ported
again. K5 is dependency-ready but shares the kernel with K4. Preserve IPC1's accepted outcomes
through both changes. R2/R3 enable native startup; B1/B2 bring up the VM; D2/D3 and S2/S3
complete storage, network, policy and SSH before E1 acceptance.

IPC1's concurrency gate remains unresolved against [PLAN's post-M1 SMP scope](PLAN.md#smp-after-milestone-1).
Record the scope decision before accepting or waiving it; single-hart tests are not simultaneous
multi-hart evidence. Model properties and mutation tests remain required for M0/M1.

## Hotspots

Serialize kernel `services.rs`, `mem.rs`, `message.rs`, `redoubt.rs`, architecture mapping code
and syscall dispatch. Coordinate runtime IPC/server changes with native callers. Generated
wire code changes through its owning tables and generator; do not overwrite it from old branches.
