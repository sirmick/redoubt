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
Open decisions are in [QUESTIONS](QUESTIONS.md), including 163 (typed parking), 164–165
(security claims), 171 (late-invalid receive output), and device/lifecycle issues 128–149. No package resolves them by assumption.

## Order

### Next three tasks

G1 cleared the launch gates on 2026-09-23 (review debt and the K5 contract; SWARM records the
non-blocking follow-ups).

1. **R2 — loader stub and startup image fields.** Use the integrated K4 lifecycle, a hostile-ELF
   confinement test and coordinated startup codec migration. This enables native launch; it does
   not alone close K4's bundle-readback gate.
2. **K5 — timer, deadlines and preemption.** Implement the accepted single-queue contract
   (answer 166) and real timeout/fairness/deadline tests, preserving IPC ownership outcomes.
   Use the sole kernel writer; it may run alongside R2 only with disjoint owned paths.
3. **B1 — the Redoubt platform for beamlet.** Scope the Platform interfaces before launch; native
   acceptance additionally needs R3 startup/public modules and K5.

### Route to working SSH and milestone acceptance

Use [SWARM Claims](SWARM.md#claims) for ownership. The rows distinguish integrated prerequisites
from final acceptance; a reviewed implementation may supply a downstream dependency while its
explicit end-to-end gates remain open. This ordering changes no security contract or acceptance
threshold. Required reviews and shared checks still apply before integration.

| Stage | Integrated prerequisites | Exit evidence / remaining gate |
| --- | --- | --- |
| Native startup: R2 + K5 → R3 infrastructure | Reviewed K4 lifecycle, existing runtime/wire/device/keyd work; affected design decisions settled | Real init starts available servers through the stub; clean public bundle-file readback closes K4's retained gate. R3 full-stack acceptance remains open. |
| VM and shell: B1 → B2 | Existing R1b/R4; production R3 handoff and K5 for boot acceptance | Elixir prints; the bench drives IEx on UART, then S3 reuses it over each SSH channel. |
| Storage: D2 | D1/L1/R1b, filesystem contracts and R3 startup | Real fsd/blkd boots, quotas, labels, disconnect and no-leaky-state cases pass. |
| Network: reconcile D3 → D3 | Existing R1b/K3/W2; verify external ownership/source before replacing or accepting work | Real netd/ipd, scoped TCP and isolated bench peer; no outside-network access. |
| Policy: S2 | R3 infrastructure, B1, D2 and existing S1 audit signing | Sessions/leases, approvals, signed audit, revocation and real init/steward blame handoff; affected 164–165 decisions settled. |
| SSH: S3 | D3 + S1 + S2 + B2; B2a/R1d for full console acceptance | Pinned-key Alice, Bob, vault and approval sessions on the production stack; session separation, per-channel resize/abandonment and cleanup checked. |
| Final acceptance: E1 | All required packages and retained gates, including K6, C1 and IPC1 | The entire PLAN milestone-1 attack suite, full bench, rv32 compilation, unsafe and review requirements; then declare milestone 1. |

B2a/B2b and R1d remain in the milestone plan; they can follow the first SSH bring-up without
blocking its basic IEx session. B2a/R1d must land before full S3 console acceptance. This is
scheduling, not a waiver of E1's existing "everything" dependency. The hosted-kernel repair
remains separate verification work; native tests neither depend on that hosted target nor
establish its compatibility. D1/R4 source is already recovered.

### Acceptance gates and decision ownership

| Gate | Owner / enabling work | Closure rule |
| --- | --- | --- |
| K4 bundle readback | R2/R3 (answer 169) | Guest compares injected bytes after clean production boot; loader refusal is insufficient. |
| IPC1 model replay | C1, after integrated IPC1/K5 and R3 bundle handoff | Native replay evidence feeds IPC1 acceptance; C1 does not wait for that final acceptance. |
| IPC1 real timers | K5, then IPC1 cases | Real timer timeouts, not host substitutes. |
| IPC1 serving-path cleanup | Native server startup through R3 | Actual serving paths exercise terminal fallback/rollback; host coverage remains partial evidence. |
| IPC1 concurrent completion | Architect scopes against PLAN's post-M1 SMP section; owner decides | Preserve the gate until resolved; single-hart cases are not simultaneous multi-hart evidence. |
| Confined deployment scope | Architect reconciles INIT's per-core placement with PLAN's post-M1 SMP scope and question 164 | A working single-hart SSH session does not prove confined placement; retain the conflict for an explicit scope decision, not an implicit exemption. INIT's literal "more cores than budget groups" refusal also needs clarification; infer no validator inequality. |
| R3 full-server boot and blame | Later D2/D3/S2/S3 integration | Close with the real milestone manifest and steward; infrastructure integration is not acceptance. |
| S3 per-channel console | B2a/R1d after decision 163 | Size/resize, label separation and parked-call abandonment follow NAMESPACES; a working SSH byte stream is insufficient. |
| S2/S3 wire integration | Architect and both package owners | Owning protocol tables, decision provenance, generated codecs/drift checks and real authentication/session/approval exchanges. |
| Shared runtime/server assurance | IPC1 reviews, ASTRA A3 cleanup and raw-syscall/owning-view audit | Explicit findings and system-verdict regressions before relying on affected security claims. |

Questions 163–165 and 171 retain their IDs and remain open until approved. Scope questions 128–149
at the consuming package, including model conformance and device lifecycle; recommendations are
not defaults. Model properties/mutations remain host evidence, not kernel replay. The architect
owns specifications and decision records; the orchestrator owns this order, claims and progress.

## Work packages

**WP-M0 / WP-M1. Host model — integrated and validated.**
The current-contract host oracle, property families and mutation checks are complete within
[the documented domain](../model/VALIDATION.md): 5,001,000 acceptance sequences and all 99
mutations detected, with three reviews and full-bench evidence. Preserve these regression checks.
Remaining work is C1's native replay and the separately tracked open contracts, including 171;
this evidence does not accept IPC1 or establish kernel conformance. Historical implementation
recipes remain in git.

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
- Implementation is integrated. G1 reconciles the retained reviews and current coverage against
  these criteria; do not restart the checker repair. This remains a separate IPC1 acceptance gate.

**WP-K4. Process creation and exit.** Size M.
- Lifecycle implementation is integrated and reviewed. Retain the criteria below as regression
  requirements; remaining acceptance work is the R2/R3 bundle-readback gate, not another lifecycle port.
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
  the loader refuses data entries). This gate is completed by the R2/R3 loader-stub and init
  bundle handoff. Answer 169 permits the reviewed lifecycle implementation to land first,
  retaining clean guest readback as unfinished K4 acceptance.
- Also: `wx` re-based on exit notices (a checker launches the attacker and takes the verdict
  from the kernel's notice); this is implemented by the recovered lifecycle tests.
- Also: `thread_create`, `thread_exit`, `process_exit`; the R8, R9 and I8 cases WP-K1 could not
  stage (a user-class caller needs `process_create`).
- Needs: WP-K2, WP-A2.

**WP-K5. Timer, timeouts and preemption.** Size M.
- Reads: KERNEL-SPEC.md R12, timeouts, deadlines; RESOURCES.md.
- Delivers: the kernel-owned timer; timeouts on `call`/`send`/`receive`; budget deadlines enforced;
  **one stride queue over every runnable budget** — no priority tier, no class ordering, no flag
  (answer 103) — with a waking budget re-entering at `max(own pass, current minimum)`,
  deterministic wake-first tie handling and preemption only at slice end or a deadline
  (KERNEL-SPEC.md R12, answer 166); `rdtime` readable from user mode.
- Accepted when: a spinning budget cannot delay another beyond its weight; a sleeping-waking budget
  cannot exceed its share; a system-class server busy on one user's requests delays other users only
  by its weight; **deterministic wake-first tie handling is implemented and tested; real-boot
  acceptance under the named workload** (N spinning user budgets at the manifest user weight, one
  driver and the steward at their manifest weights) **reports the measured driver wake and steward
  lease-termination latencies with the recorded weights, runnable budgets and prior passes**, WP-K5
  proposes the numeric target with that evidence and a package reviewer accepts it (RESOURCES.md,
  Attack tests), and a large-weight server keeps its share under that load; every blocking call
  returns by its timeout (I13); a deadline destroys its budget; the old IRQ-0 timer path and
  `timer` case are gone.
- Needs: WP-K2.

**WP-K5a. `map_fixed` (answer 172).** Size S.
- Reads: KERNEL-SPEC.md R11, System calls and Errors (`map_fixed`); PACKAGES.md, Launching a process.
- Delivers: `map_fixed(addr, len, flags)` in the kernel, `redoubt-sys` and the executable model,
  appended last in the call table so earlier call numbers keep their values; it never replaces a
  mapping.
- Accepted when: rv64 boot and rv32 compilation; attack cases refuse an occupied range, a range
  outside user space, an unaligned address or length, len 0, `addr + len` overflow, W+X, W without
  R and an exhausted budget, each with nothing mapped and nothing charged; a model mutation shows
  the model's rule bites.
- Needs: WP-K4. Uses the sole kernel writer: the K5 implementer does it first, reviewed and
  integrated on its own so WP-R2 can finish; trusted-code review panel.

**WP-K6. Delete the legacy interface.** Size M.
- Delivers: removal of SID connects, scalar message kinds, `ClaimInterrupt`, the `grants` entry,
  name lookup and `PlatformSpecific` timer calls; old test programs migrated or deleted.
- Accepted when: the bench passes with no legacy calls; the `unsafe` ratchet is lower than before
  WP-K1; kernel line count reported.
- Needs: WP-K1 to WP-K5, WP-R1b.

**WP-IPC1. Observable IPC ownership and delivery (answers 167-168).** Size L.
- The implementation is integrated. Remaining work closes the acceptance gates below and fixes
  demonstrated gaps; do not rebuild the outcome ABI or repeat the completed recovery.
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
- Needs to start: WP-A2, WP-K2, WP-R1c, WP-S1 (all merged). Kernel edits use one writer and
  serialize with K5/K6; `libs/rt` edits serialize with WP-R1d and server ports.
  The WP-M1 host model is integrated; real-kernel trace replay remains WP-C1 work.
  **Completion gates:** WP-M1 integration (complete), WP-K5 real timer support, and repair of ASTRA C4's
  verification gap. ABI/runtime work need not wait for those gates, but this package cannot be
  accepted or marked done without them. Filed after owner approval; not dispatched by that approval.
  C1 native replay, actual serving-path evidence and the unresolved concurrency gate also remain
  required; see Order's acceptance-gate table for the retained completion dependencies.

**WP-C1. Model conformance.** Size M.
- Reads: KERNEL-SPEC.md, errors and the order of checks (the model conforms to them).
- Delivers: a bench case replaying WP-M1 traces on the real kernel and comparing every result
  (usage compared in the model's placement profile, R11); a system-call fuzzer program (random and
  hostile arguments; I14).
- Accepted when: 10^5 model traces replay with identical results; the fuzzer runs
  for its budget with no kernel panic.
- Needs to run native acceptance: integrated WP-M1, WP-K1 to WP-K5, WP-T1 and the integrated
  WP-IPC1 outcome ABI/implementation (answers 167-168), plus R2/R3's production bundle handoff
  for trace files (testbench.md, Files in the bundle). It does not wait for IPC1's final acceptance:
  C1 supplies replay evidence to that acceptance. No temporary boot-data ABI is implied.
- Contract gate: question 171 and relevant open model/kernel questions must have an explicit
  disposition before affected traces count toward conformance. Unsupported scenarios are not passes.

**WP-R1d. Let a typed call park (open question 163).** Size S.
- Reads: NAMESPACES.md (Holding a call), `libs/rt/src/server/typed.rs` and `ninep.rs`.
- Delivers: the park mechanism reaches the **typed** dispatch too, which today it does not. `serve_parking` hands a request back only when `answer_in_place` returns `Answer::Waiting`, and that comes only from `FileServer::read -> Read::Wait`; a typed opcode goes to the server's own dispatch, which is `Result<(), Error>` and must reply, and `Answer<R>` has no "wait" variant. Give the typed path the same hand-back: a way for a typed handler to say "hold this", routed through the same close-the-handles-and-empty-the-list path the read already uses. Nothing else changes: the parked call is still charged to the server's `Admission`, still reported abandoned, still resumed and re-read. One mechanism, two entry points.
- Accepted when: `redoubt-rt`'s host tests cover a parked **typed** call resumed and answered, one abandoned and freed, and a `Wait` from a typed handler refused by a plain `serve`; both widths build; the fuzz target reruns.
- Needs: WP-R1c. **Blocks the `resize` half of WP-B2a**; until it lands, B2a implements opcode 16 `size` and not 17 `resize`. `libs/rt` is shared, so its own review round (answer 157).

**WP-R2. Loader stub.** Size S.
- Reads: PACKAGES.md (launching), INIT.md (startup block).
- Delivers: the flat-binary stub at its fixed address: find the ELF image through the startup
  block's image fields (defined with R2 in INIT's owning `startup` table, answer 65), parse it
  from memory, map segments (code executable and read-only), free the image, jump, passing on the
  startup page's address it was started with (`arg`).
- Accepted when: fuzzed ELF images never escape the child (the child faults or exits; nothing else
  is affected); attack case: a hostile ELF from a user parent hurts only the child.
- Needs: WP-R1b, WP-K4; on-target stub mapping and the hostile-ELF boot acceptance also need WP-K5a
  (`map_fixed`, answer 172). Host-side work (the stub's ELF checks, startup fields, codec, fuzzing)
  does not.
- Uses K4's integrated lifecycle, not its still-open R2/R3 bundle-readback acceptance (answer 169).
  Define the image fields in INIT's owning startup table with the architect, regenerate the wire
  codec, and migrate its writers/readers together. Preserve PACKAGES' current copied-image path;
  question 134 is not permission to introduce shared image pages.

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
  `init` can start every bundle program through the stub. This handoff also completes K4's
  retained clean `bench-bundle-file` guest readback gate (answer 169).
- Accepted when: the milestone 1 manifest boots every server, each in a budget carrying the weight
  the manifest names; a manifest with a bad name, or one granting a server its budget, is refused;
  attack cases: a manifest giving `keyd` the bundle key stops the boot (the `holds` answer, not a
  derivation in `init`), and so does one giving `keyd` a principal's login or approval key; a
  manifest whose `public` list names the manifest, or an entry the bundle lacks, is refused;
  arguments reach their server byte for byte, and a count or length that would overflow the startup
  page is refused; no server's startup block holds a budget handle; a crashing server restarts on
  the same endpoint. Each attributed crash forwards its account and label set unchanged to the
  steward; three matching crashes within 10 minutes exercise S2's revocation/login-refusal policy,
  while unrelated and differently labelled sessions survive. A crash without a current call
  invents no blame; more than 5 restarts in 60 s remains init's separate reboot rule.
- Needs: WP-R2, WP-W1, WP-K3, WP-K5. The `holds` operation it calls belongs to `keyd`'s table
  (WP-S1), so the refusal is written here and exercised end to end once WP-S1 has landed.
- Integration order: first deliver reviewed startup/manifest infrastructure and the production
  bundle handoff with available servers. Later packages build on that integrated infrastructure;
  R3 stays unaccepted until its full-server boot and real steward blame tests pass. S2 owns the
  blame message table in INIT; coordinate that contract before either side implements the wire
  handoff, and test both sides together when S2 is available. No substitute server proves that gate.
- Contract gates: resolve the relevant device/startup questions 142–149 and confinement question
  164 before implementing their affected behavior. In particular, 147 governs safe driver restart,
  148 the private-bundle/public-bootfs handoff, and 149 device handle names.
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
- These permit platform work to start after its interface questions are scoped. On-target acceptance
  additionally uses R2/R3 startup, the public module handoff and K5 time/wait behavior; host VM
  tests alone do not establish that Elixir prints on Redoubt. Inventory the existing Platform
  methods against this slice before adding natives; unresolved answers are not implementation defaults.

**WP-B2. IEx on the UART console.** Size S.
- Delivers: an IEx session on the UART; the first Redoubt IEx helpers (`ls`, `cd`, `cat` over 9P).
- Accepted when: a bench case types expressions at IEx and checks the answers (PLAN.md's
  milestone 1, step 3).
- Needs: WP-B1, WP-R3.
- R3 here means integrated startup infrastructure. INIT's shell helpers `ps`/`budget` (own account
  and label set only) and approval notification must also have owners: B2 supplies the shell surface,
  S2 supplies policy/notification integration, and S3 tests that surface in a real SSH session.
  Define any unsettled interfaces with the architect before implementation; do not import the
  entire milestone-3 API into this slice. UART IEx remains bench-only (INIT, The shell).

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
- Dependencies are merged, but question 131's held-fid behavior and relevant filesystem interface
  questions (including 129/130/137 where consumed) require contract reconciliation. R3 supplies the
  production startup needed for boot acceptance; admission-chain question 141 affects shared-server
  assurance. Do not infer approval from an existing prototype or from target prose alone.

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
- Uses R3's integrated startup infrastructure; jointly closes R3's real steward blame gate.
  Resolve affected questions 164/165 before accepting mediation or authority-closure claims;
  latency claims follow answer 166's measured target. S1's audit signing is exercised here.
- Coordinate the S2/S3 authentication, session and approval protocol inventory with the architect
  before either endpoint implements it. WIRE requires a table in its owning specification,
  approval provenance as applicable, generated Rust/Elixir codecs and drift checks; settle any new
  authority, lifetime or error choices explicitly. S2 and S3 own end-to-end use of those contracts.

**WP-S3. sshd.** Size M. `sunset`-based; host key through `keyd`; user authentication through the
steward; rejects keys `keyd` holds; each channel labelled with its session's labels, a labelled
channel being a pty session with no forwarding, subsystems or `exec`; `ssh approve@box` (sharing
this `sshd` in milestone 1, a stated residual).
- Accepted when: `alice@`, `alice+secrets@`, `bob@` and `approve@` sessions work from the bench's
  SSH client, with the box's host key pinned (`net.host_key`); loopback login with a `keyd` key
  refused. The bench's loopback self-checks log in as one host user, so per-user separation is
  first tested here.
- Needs: WP-D3, WP-S1, WP-S2 and WP-B2 (IEx is the session shell).
- End-to-end integration: boot the real stack through init; `sshd` owns each channel's `/dev/cons`,
  the steward authenticates and launches its IEx VM, and `keyd` signs the exchange. Exercise
  concurrent Alice/Bob sessions, a labelled pty session and `approve@`, with a pinned host key.
  Check home/namespace separation, labelled-channel refusal of forwarding/subsystems/exec,
  approval-channel isolation, and logout/VM-death cleanup while the other session remains usable.
  These instantiate INIT's existing session contract; E1 still owns the complete attack suite.
- Full console acceptance additionally needs B2a's `consol` codec and R1d's typed parking
  (question 163). Implement `size` from that channel's pty request and `resize` from its window
  changes, with the per-channel receiving threads required by NAMESPACES, The console. Test two
  channels with different dimensions: `size` returns each channel's dimensions; a window change
  completes that channel's parked `resize` with the new size, a fresh `size` query agrees, and
  the other channel's waiter remains parked and learns no geometry. Parked byte reads and typed
  resize calls release their admission resources on abandonment/logout while the other channel
  remains usable.
  Verify nonblocking VM input, no-input distinct from EOF and no stale size cache (USERLAND-API,
  The console). Basic byte-stream SSH bring-up is not full S3 acceptance.
- S3 owns the necessary bench controls and negative self-checks for these cases. The existing
  session runner supplies pty requests, text steps and concurrent sessions, but no explicit
  window-change step; extend the harness as needed for real resize and forbidden-request tests.
  Reuse the pinned-host-key and concurrent-session machinery; host loopback is harness evidence,
  not Redoubt user-separation evidence.

**WP-E1. Alice's agent and the attack suite.** Size M.
- Delivers: the scripted hostile agent and the scripted hostile user (Bob), and every case in
  PLAN.md's milestone 1 attack suite not already delivered by the packages above, each asserted
  through the system (How to read a work package).
- Accepted when: the whole suite passes, and **milestone 1 is declared done** in STATUS.md.
- Needs: everything above.

## Hotspots

Serialize kernel `services.rs`, `mem.rs`, `message.rs`, `redoubt.rs`, architecture mapping code
and syscall dispatch. Coordinate runtime IPC/server changes with native callers. Generated
wire code changes through its owning tables and generator; do not overwrite it from old branches.
