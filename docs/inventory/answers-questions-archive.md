# DOC1 Inventory: answers, questions and archive

Extracted per docs/legacy/{ANSWERS,QUESTIONS}.md and docs/legacy/archive/2026-09-22/{ANSWERS,QUESTIONS,ASTRA}.md,
for the DOC1 migration inventory (todo/DOC1-docs-rewrite.md, Migration step 3). Every accepted decision,
residual risk, security caveat, open question and rule ID before the legacy docs are deleted.


### A-1: Archived decisions through 2026-09-22 are accepted
- type: decision
- source: ANSWERS.md#Archived decisions through 2026-09-22
- statement: Accepted IDs 1-126, 150-160, 162, 167-168 from the 2026-09-22 archive stand as approved; 161 is the orchestrator's package split, not an owner-approved interface. Later answers revise earlier wording: 56 revised; 57/58 replaced by 82; 103 lost `first`/priority tiers; 120 added the bundle signature domain; 167/168 add observable IPC outcomes without changing lend ownership; 166 revises 103's one-SLICE latency claim.
- status: accepted
- destination (proposed): plan

### A-2: Ownership map of decision areas to specifications
- type: decision
- source: ANSWERS.md#table
- statement: Kernel objects/accounting/IPC/ABI/errors/scheduling belongs to KERNEL-SPEC; labels/confinement/mediation/admission to CONTAINMENT and TENETS; delegation/leases/approvals to CAPABILITIES; boot/manifest/startup/keyd to INIT and VERIFIED-BOOT; 9P/parked calls/console size to NAMESPACES and USERLAND-API; wire encoding to WIRE; package split 161 to BUILD-PLAN.
- status: accepted
- destination (proposed): plan

### A-3: Approval does not establish implementation
- type: security-caveat
- source: ANSWERS.md#Archived decisions through 2026-09-22
- statement: "Approval does not establish implementation": an accepted decision is not evidence the behavior is built or tested; STATUS is the source for that.
- status: accepted
- destination (proposed): SECURITY

### A-4: Process/notice context storage charged to creator's budget
- type: decision
- source: ANSWERS.md#127
- statement: The process/notice object remains one page charged to the creator's budget. `PROCESS_IMPL_PAGES` saved-context frames (one on rv32, two on rv64) are charged to the execution budget from address-space creation until teardown, one IPC page per thread plus all actual page-table pages, each frame counted once; the global per-PID context reservation is removed. Initial loader processes pay for their actual context frames; their temporary lack of process/notice objects is a boot integration limitation, not an extra unbacked charge.
- status: accepted
- destination (proposed): kernel

### A-5: Accounting/rollback/restoration coverage of both widths is unverified
- type: residual-risk
- source: ANSWERS.md#127
- statement: "accounting, rollback and restoration tests must verify both widths; approval alone does not establish that evidence."
- status: open
- destination (proposed): SECURITY

### A-6: Bundle readback retained as K4 acceptance dependency (legacy numbering)
- type: decision
- source: ANSWERS.md#169
- statement: Independently reviewed K4 lifecycle integration (legacy numbering) may land with clean guest `[[file]]` readback explicitly unfinished; that readback completes through the production loader-stub/init bundle handoff in R2/R3 and remains a K4 acceptance dependency. The testbench's attribution of the kernel/init-only loader transition to R3 is corrected. No temporary boot ABI is added and full K4 acceptance is not claimed from the lifecycle integration alone.
- status: accepted
- destination (proposed): plan (legacy numbering: WP-K4/WP-R3)

### A-7: Guest byte comparison after clean boot still unmet
- type: residual-risk
- source: ANSWERS.md#169
- statement: "the guest must still compare the injected bytes after a clean boot. The current expected loader rejection does not satisfy that gate."
- status: open
- destination (proposed): SECURITY

### A-8: Final thread exit is equivalent to process_exit(0)
- type: decision
- source: ANSWERS.md#170
- statement: `thread_exit` with surviving siblings cleans up only the exiting thread and its IPC, without a process-exit notice. The final thread's `thread_exit` is equivalent to `process_exit(0)`: open-call status and current-call blame are determined before cleanup — `faulted` if the process holds open calls (existing blame rule), otherwise `exited` with code 0. Normal process teardown, creator-paid notice lifetime and PID retention are preserved.
- status: accepted
- destination (proposed): kernel

### A-9: Final-thread exit test coverage required
- type: residual-risk
- source: ANSWERS.md#170
- statement: "real-kernel tests must cover final-thread exit both with and without open calls and surviving siblings."
- status: open
- destination (proposed): SECURITY

### A-10: Withdraw universal one-SLICE wakeup claim; add wake-first tie rule (revises 103)
- type: decision
- source: ANSWERS.md#166
- statement: Keeps answer 103's single stride queue, actual-runtime charging and `max(own pass, current minimum)` wake rule, but withdraws the claim that a driver or steward woken under load runs within about one `SLICE` — nothing in the mechanism establishes it and no deadline follows from weight. R12 gains a deterministic wake-first tie rule (a waking budget ranks ahead of already-queued budgets with an equal pass); preemption happens only at slice end or a deadline, never on wake alone. Wakeup is prompt but not bounded. The attack-test/WP-K5 bullets repeating the bound are replaced by a measured responsiveness target under a named workload (N spinning user budgets, one driver, the steward, recording weights/runnable budgets/prior passes/wake latency/lease-termination latency); WP-K5 proposes the numeric target with evidence and reviewer acceptance.
- status: accepted
- destination (proposed): kernel (rule-id: R12)

### A-11: Human control rests on measured, not proven, latency bound
- type: security-caveat
- source: ANSWERS.md#166
- statement: "human control (TENETS guarantee 3) rests on a measured steward lease-termination latency, not a proven bound, until something needs a real-time rule."
- status: open
- destination (proposed): SECURITY

### A-12: map_fixed syscall for loader-stub self-mapping
- type: decision
- source: ANSWERS.md#172
- statement: Adds `map_fixed(addr, len, flags)`: maps zeroed pages at exactly `addr` in the caller's own address space, charged like `map_anon`. `addr`/`len` page-aligned, `len` nonzero, range in user space and non-overlapping with caller's mappings, else `InvalidArgument` with nothing mapped; never replaces a mapping (unlike POSIX MAP_FIXED). Flags follow `map_anon`'s rules (not 0, not W+X, not W without R); errors `InvalidArgument`, `OutOfMemory`. `map_anon` and its callers are unchanged. The loader stub maps a program's segments with it before anything else and exits if a segment overlaps the stub, startup page, or image.
- status: accepted
- destination (proposed): kernel (rule-id: R11, System calls, Errors)

### A-50: Kernel package acceptance requirements for map_fixed
- type: residual-risk
- source: ANSWERS.md#172
- statement: Needs a kernel package (sole kernel writer) with rv64 boot and rv32 compile acceptance and attack cases covering occupied range, outside user space, unaligned, len 0, overflow, W+X, W without R, and budget exhausted; MEMORY-LAYOUT.md must record the stub's address once the loader-stub package fixes it, so program link bases avoid it.
- status: open
- destination (proposed): SECURITY

### A-51: Kernel resets and quarantines DMA device frames before reuse (resolves Q147)
- type: decision
- source: ANSWERS.md#173
- statement: When the last handle to a DMA-flagged MMIO device object is released (holder exits, budget destroyed, or closed), the kernel writes 0 to the device's virtio status register, reads it back until 0 (bounded), and only then returns that device's `dma_alloc` frames to the pool. If reset is not confirmed in the bound, or the device is not virtio, its frames are quarantined permanently (charged to the budget holding the device object's grant, not the dead driver) and the device is not handed out again until reboot. The kernel never services the device on the driver's behalf.
- status: accepted
- destination (proposed): kernel (rule-id: R10 destruction order)

### A-15: Quarantine covers hostile non-reset case at memory cost
- type: security-caveat
- source: ANSWERS.md#173
- statement: "This resolves the concrete vulnerability where freed DMA frames holding rx/tx rings let a device's DMA be steered to any physical address by a new owner (rx an arbitrary write with LAN bytes, tx an arbitrary read)." Quarantine trades memory for safety when a device ignores reset — exactly the hostile case.
- status: accepted
- destination (proposed): SECURITY

### A-16: WP-K5b clarifications to answer 173 (three readings, owner-approved 2026-09-24)
- type: decision
- source: ANSWERS.md#173 note
- statement: (1) The trigger is process end, not last-handle release: when a process ends, the kernel resets every device it allocated through or mapped, and frames return to the pool only if every device confirms reset in that step; otherwise all are quarantined, and a device quarantined earlier never counts as reset (R11). (2) The quarantine charge stays on the dead driver's budget; when that budget is destroyed, the charge moves to its parent after the destroyed budget's carve returns, so the parent never exceeds its limit (preserves I5, usage within limit). (3) "Not handed out again" means the device object is destroyed as R10 destroys one — every handle closes (including copies in unreceived messages) and its base stays flagged until reboot (R10); `map_device`/`dma_alloc` need no extra `NotPermitted` check since no handle can reach a destroyed object.
- status: accepted
- destination (proposed): kernel (rule-id: R10, R11)

### A-17: Architect settled a reading of the WP-K5b plan
- type: decision
- source: ANSWERS.md#173 note
- statement: The Architect settled the reading that `map_device`/`dma_alloc` need no flagged-device `NotPermitted` error, in Wash QA K5b-od6-sweep, 2026-09-25.
- status: resolved
- destination (proposed): kernel

### A-18: netd/ipd/net contracts for milestone 1
- type: decision
- source: ANSWERS.md#174
- statement: WP-D3's network contracts (owned by IO-ARCHITECTURE Networking/netd and NAMESPACES The network tree): `netd` is a virtio-net driver in blkd's manner — version 2 only, VERSION_1 and MAC only, two DMA regions, no address in any message; a structural lie breaks the device, a wrong-length wire frame is dropped, a device fault never makes it exit; its `netif` protocol (`info`, `transmit`) serves one client badge. Frames reach `ipd` as a `send` with a one-page `frame` transfer (first `send` in any table, giving WIRE.md tables their Kind column). `ipd` runs vendored smoltcp 0.14.0 (IPv4/TCP only, static address; `/net/udp` deferred). `/net` files are typed (`clone`, `ctl` with `net_ctl` ops whose read waits for connect/accept, `data` reads/writes wait, `remote` names the peer). Capability is at most 8 prefix-and-port rules; root scopes come from ipd's arguments, a typed `grant` never widens, `new_connection` keeps scope, `disconnect` frees it. The box's own addresses (QEMU self=10.0.2.0/24) are refused before scope is consulted; labelled callers are refused before admission. `libs/rt` gains `Write::Wait`, `NineServer::mint_rooted`, a panic hook, and a per-badge admission override for account-0 root badges. Each TCP connection's initial sequence number comes from a fresh kernel-random seed. Before `init` exists, WP-D3's acceptance is a rig launching real netd/ipd through the stub; manifest boot remains WP-R3's retained gate.
- status: accepted
- destination (proposed): servers/netd

### A-19: smoltcp is vendored, not only pinned
- type: decision
- source: ANSWERS.md#174
- statement: Mick approved the thirteen owner decisions of the WP-D3 plan with one change: smoltcp is vendored (`vendor/smoltcp` and dependencies) rather than only pinned, so the stack's exact source is read and built from the tree, not fetched.
- status: accepted
- destination (proposed): servers/ipd

### A-20: Residual risks of the network contracts
- type: residual-risk
- source: ANSWERS.md#174
- statement: An armed device after a netd death is answer 173's concern (WP-K5b); without it there is no netd restart and no off-bench use. An address outside self= that loops back to the box is refused only if listed, with sshd's refusal of keyd keys as the backstop. Blind TCP injection needs about 2^19 guesses per connection and yields at most a reset (SSH's MAC rejects injected bytes). A half-open flood holds a listener's backlog 3s per SYN.
- status: accepted
- destination (proposed): SECURITY

### A-21: No endpoint_destroy call; endpoint dies with its owner budget
- type: decision
- source: QUESTIONS.md#128
- statement: An endpoint cannot be destroyed independently — it dies only with its owner budget (R10); a process wanting to reclaim one creates it in a scope it can destroy. Bounded by budget size; a revocation scope gives a reclaim path.
- status: open
- destination (proposed): kernel

### A-22: Cross-directory rename via typed fsd call
- type: decision
- source: QUESTIONS.md#129
- statement: Plain 9P2000 rename cannot express cross-directory renames; recommendation is `fsd` serves a typed `rename(dir_fid, name, dir_fid, name)`, atomic within a volume, with cross-volume falling back to copy-and-remove (reported like POSIX EXDEV). Alternative: copy-and-remove everywhere, non-atomic, crash mid-rename leaves both or neither.
- status: open
- destination (proposed): servers/fsd

### A-52: File.stat/chmod semantics with no mode/owner/atime fields
- type: decision
- source: QUESTIONS.md#130
- statement: No mode, owner, or atime fields exist anywhere below (access is by capability). Recommendation: synthesise a fixed mode for stat, report mtime/size honestly, let chmod/chown succeed as no-ops (since Mix/escript tooling calls chmod and the bits mean nothing). Alternative: :enotsup for both, honest but breaks tools.
- status: open
- destination (proposed): servers/fsd

### A-24: Fid held across a remove
- type: decision
- source: QUESTIONS.md#131
- statement: NAMESPACES.md says a remove succeeds while another connection holds a fid but doesn't say what the holder then sees. Recommendation: the fid keeps serving the unlinked file until clunked (POSIX-like, cheap under littlefs copy-on-write); a fresh walk gets :enoent. Alternative: subsequent operations on the fid fail (simpler but surprising).
- status: open
- destination (proposed): servers/fsd

### A-25: Naming stdin/stdout/stderr without descriptor inheritance
- type: decision
- source: QUESTIONS.md#132
- statement: Redoubt has no descriptor table or inheritance, so a pipeline needs distinct per-child bound names. Recommendation: `/dev/stdin`, `/dev/stdout`, `/dev/stderr` namespace entries, with `/dev/cons` bound to all three for an interactive child. Alternative: `/fd/0`, `/fd/1`, `/fd/2`.
- status: open
- destination (proposed): userland

### A-26: Who serves a pipe
- type: decision
- source: QUESTIONS.md#133
- statement: A pipe is a 9P file somebody serves (no pipe object). Recommendation: the session's shell VM serves it, needing serve/reply natives and a server-side 9P codec in beamlet (also needed for System.cmd output capture). Alternative: a tiny `piped` server per session, keeping bulk bytes out of the shell VM at the cost of another server package.
- status: open
- destination (proposed): userland

### A-27: Program image copied fresh on every launch (no shared text/demand paging)
- type: decision
- source: QUESTIONS.md#134
- statement: The launcher copies the ELF into fresh pages charged to the child; there is no shared text or demand paging. Accepted for milestone 1 (legacy numbering); recorded as the reason the steward may cache a VM image; open question whether WP-R2's stub can map image pages read-only from a shared cache instead.
- status: open
- destination (proposed): plan

### A-28: Launching native programs from Elixir
- type: decision
- source: QUESTIONS.md#135
- statement: Milestone 1's shell (legacy numbering) needs `process_create`, `process_map`, `process_start` as beamlet natives, with the startup block written from Elixir. Recommendation: natives for the three calls plus a Rust `startup` block writer (encoder exists in redoubt-wire); namespace/handles come from the Elixir call so policy stays in Elixir and encoding stays in Rust.
- status: open
- destination (proposed): userland

### A-29: Location of Elixir-side modules over the beamlet natives
- type: decision
- source: QUESTIONS.md#136
- statement: Modules like Redoubt.Namespace/Process/Budget could be embedded in the VM or a Mix package loaded from the boot bundle. Recommendation: a Mix package in redoubt/elixir/, versioned with the system, not the VM; only what the VM needs at boot stays embedded.
- status: open
- destination (proposed): userland

### A-30: Two error vocabularies (POSIX atoms vs Redoubt errors)
- type: decision
- source: QUESTIONS.md#137
- statement: `File` expects POSIX atoms (:enoent, :eacces); Redoubt has refused/not_yours/label/budget errors. Recommendation: Redoubt errors keep their own atoms everywhere; the File/prim_file shim maps them to POSIX atoms only at that boundary, so OTP code sees what it expects and new code sees the truth.
- status: open
- destination (proposed): userland

### A-31: Intra-budget transfers should be free (clarifies R4)
- type: decision
- source: QUESTIONS.md#138
- statement: R4 counts a message's transferred pages against the receiving budget, but a transfer within one budget moves nothing between budgets, so the kernel should charge nothing and deliver even with no free pages. Recommendation: state in R4 that a transfer costs the receiving budget only what it doesn't already pay for (free within a budget); a lend is charged to both sides even within a budget (R3).
- status: open
- destination (proposed): kernel (rule-id: R4)

### A-32: Notices after an endpoint is destroyed
- type: decision
- source: QUESTIONS.md#139
- statement: Destroying an endpoint abandons calls taken through it, but a notice can never be offered there — the server's receive gets Dead. Recommendation: KERNEL-SPEC.md states Dead from receive is the server's cue to reply to every open call taken there; no notice follows since the endpoint is gone.
- status: open
- destination (proposed): kernel

### A-58: Lending untouched pages in a call is backed and charged to caller
- type: decision
- source: QUESTIONS.md#140
- statement: The ABI refuses an untouched record, but a call's lend of untouched pages is backed/charged to the caller first, like map_anon; a caller that cannot pay gets InvalidArgument since call's row has no OutOfMemory. Recommendation: state this in R3 or the call row so model and kernel agree.
- status: open
- destination (proposed): kernel (rule-id: R3)

### A-34: System client bucket-exhaustion via self-minted connections
- type: security-caveat
- source: QUESTIONS.md#141
- statement: Admission keys account 0 by badge; a share folds into its parent's only when the requester's client matches the caller's, so a system client of fsd/blkd minting connections for itself can open a fresh bucket each time and spend every bucket the server has, starving new connections. Recommendation: fold when the requester's (account, label set) matches the caller's, ignoring badge, while admission keys by badge as now — keeping the steward-minting-for-a-lease's-agent share rule but closing this chain, at the cost of system-to-system delegation losing its own share.
- status: open
- destination (proposed): kernel

### A-35: Device object cost and ownership
- type: decision
- source: QUESTIONS.md#142
- statement: The cost table has no Device row and nothing states which budget owns one. WP-K3 charges one page to the owning budget and revokes it with that budget (R10), like an endpoint. Recommendation: cost table gains a device row (1 page, owner-charged), the device object's owner is the budget that held the handle when the loader handed it out (init's, in practice), and there is no device_destroy (as with endpoints, Q128).
- status: open
- destination (proposed): kernel (rule-id: R10)

### A-36: Loader device-decision policy and interrupt-controller gap
- type: security-caveat
- source: QUESTIONS.md#143
- statement: WP-K3's loader sets the DMA flag from a compatible-virtio node, keeps interrupt controllers out of the device list, and refuses boot on an entry naming RAM or wrapping; BOOT.md documents this but the target policy remains open — an interrupt controller as a device object would let its holder mask anyone's interrupts. Addendum: the kernel refuses a Devs entry naming RAM but leaves the interrupt controller to a loader heuristic although the kernel holds the controller's range in the Plic tag; a Devs entry naming the PLIC would be mapped into userspace unguarded, giving its holder every interrupt source. Recommendation: the kernel checks that too; BOOT.md states all four rules plus the Devs tag layout.
- status: open
- destination (proposed): kernel

### A-54: Device handle copyable; two holders of one MMIO range; mapping outlives handle
- type: security-caveat
- source: QUESTIONS.md#144
- statement: A device handle is copyable, so two processes can both map_device the same range; WP-K3 treats the handle as authority and doesn't track mappings. Addendum: revoking a device handle does not unmap MMIO a holder already mapped, and since the handle is copyable, a process in another budget keeps register access after the owner budget is destroyed. Recommendation: state in KERNEL-SPEC.md that a device is shared like an endpoint (a driver that must be alone is the only holder because init gave it out once); either unmap on destroy, or state plainly that a device mapping outlives its handle.
- status: open
- destination (proposed): kernel

### A-38: R11 (page table freed when empty) not implemented; table pages can be stranded
- type: security-caveat
- source: QUESTIONS.md#145
- statement: "\"A page table is freed when it maps nothing\" (R11) is not implemented, and no package owns it." `unmap` returns pages but not their tables, so a process can strand its own table pages — bounded (charged to itself) but the rule as written is false. Recommendation: WP-K3's unmap frees an empty table and the cost table's page-table row states when pages come back; if too large a change, hand to WP-K5 and say so in the rule.
- status: open
- destination (proposed): kernel (rule-id: R11)

### A-39: Device mapping return value and named handle discovery
- type: decision
- source: QUESTIONS.md#146
- statement: The target syscall row returned only an address; the implemented ABI returns addr,len (BOOT.md); needs reconciling, and drivers currently discover devices positionally via dma_alloc rather than by manifest-supplied names. Recommendation: map_device -> addr, len; which device a handle names comes from the boot manifest, which gives each driver its handles by name (WP-R3); the kernel says nothing about naming.
- status: open
- destination (proposed): kernel

### A-40: Driver restart leaves device pointed at freed frames (superseded by A-14)
- type: security-caveat
- source: QUESTIONS.md#147
- statement: blkd's DMA pages return to the free pool when it dies, but nothing stops a device already programmed with their physical addresses from writing into them; the restarted blkd resets the device at bring-up only after those frames may belong to someone else — the concrete form of the K3 residual that a DMA handle is kernel-level trust. Recommendation (superseded by answer 173/A-14): the kernel resets a device whose DMA pages are freed before returning frames to the pool.
- status: superseded by A-14
- destination (proposed): kernel

### A-41: bootfsd never sees the bundle; typed protocol for sealing /boot
- type: decision
- source: QUESTIONS.md#148
- statement: WP-R4 chose that bootfsd never sees the bundle: init reads it and pushes public bytes over a two-message typed protocol (bootfs, opcodes 16/17: add appends to a name already in the argument list at the offset reached so far, seal ends setup and refuses both forever). Before seal the directory is empty; after it nothing can be added; neither message is accepted on a minted connection, so only init's founding handle can fill /boot. Recommendation: accept — answer 123 then holds by construction, and the bundle (carrying keyd's seeds until milestone 2, legacy numbering) never enters a server answering user requests.
- status: open
- destination (proposed): servers/bootfsd

### A-42: Naming convention for a device's separate MMIO and interrupt manifest entries
- type: decision
- source: QUESTIONS.md#149
- statement: The manifest's devices list gives one entry per device object, but a UART or disk is two objects (region + interrupt), and three packages chose their own convention (blkd: disk/disk-irq; consoled: uart/uart:irq). Recommendation: one INIT.md rule — an MMIO region and its interrupt are separate manifest entries and separate named handles, NAME and NAME-irq, under the manifest's name rule (allows `-` not `:`); WP-R3 enforces it and blkd/consoled follow.
- status: open
- destination (proposed): kernel

### A-43: Typed calls cannot park; resize blocked on libs/rt extension
- type: security-caveat
- source: QUESTIONS.md#163
- statement: Answer 160 makes consol's opcode 17 resize a call that parks, but the landed park mechanism (WP-R1c) reaches only the 9P read path: NineServer::serve_parking hands a request back only when answer_in_place returns Answer::Waiting, produced only by FileServer::read returning Read::Wait; a typed opcode goes to the server's own dispatch (Result<(), Error>, must reply) and typed Answer<R> has no wait variant, so resize as specified cannot be built yet. Recommendation: extend typed dispatch to park the same way, in its own libs/rt package (WP-R1d), owned by the round that first needs it (WP-B2a); until it lands, WP-B2a implements opcode 16 size and not 17 resize. Alternative: make resize a 9P file (/dev/cons-size) instead — needs no libs/rt change but splits one concern across two mechanisms. Alternative rejected: a send-based push (heavier than milestone 1 needs, per answer 160).
- status: open
- destination (proposed): servers/consoled

### A-44: Confined placement vs. approved steward mediation paths (ASTRA D1)
- type: security-caveat
- source: QUESTIONS.md#164
- statement: Answer 152 and INIT.md forbid differing label sets sharing any server or endpoint including the unlabelled steward's domain, yet answers 101/153 require labelled reader/writer helpers and owner-approved declassification/push, and CAPABILITIES.md requires labelled requests to reach the powerbox — the missing piece is a permitted topology and its post-boot preservation. Guessing an exemption changes the confined guarantee in TENETS.md and the boot refusal WP-R3 must implement. Recommendation: retain ordinary per-label placement and explicitly name the trusted control-plane mediation exception in TENETS.md, INIT.md and CONTAINMENT.md, permitting only the specified request/owner-approval path, per-item reader/writer operations, and labelled lifecycle supervision needed to end leases, with one worked configuration giving each edge its caller labels/allowed data/authority/lifetime; do not exempt shared data servers, devices or cores. init validates the declared graph at boot; the steward enforces it dynamically; system servers enforce their own handoffs. Alternative: retain answer 152 literally with a separate control-plane instance per label set and external owner-mediated transfer.
- status: open
- destination (proposed): TENETS

### A-45: Named mediators are trusted across the labels they serve
- type: residual-risk
- source: QUESTIONS.md#164
- statement: "the named mediators are trusted across the labels they serve; the confinement claim must say so explicitly."
- status: open
- destination (proposed): SECURITY

### A-46: Closure of authority under same-label delegation (ASTRA D2)
- type: security-caveat
- source: QUESTIONS.md#165
- statement: Answer 150 says equal-label budgets are one trust domain and a handle passed between them is not a crossing; CAPABILITIES.md permits copying handles, so a recipient can acquire authority absent from its own initial handle set without new approval — while TENETS.md (Purpose and threat model) and GAME.md (Authority expansion) read as forbidding that increase. Recommendation: define the closure claim over a trust domain's initial granted authority plus human-approved additions, closed under permitted delegation/service paths; a process exercises only its currently held grants and may receive attenuated delegations, the union being a bound not a minting permission; align TENETS.md, CAPABILITIES.md, GAME.md so lawful delegation is not scored as escape; keep R9, stamps, lease revocation and answer 150 unchanged. Alternative: define per-agent reachable authority as the transitive closure of an explicitly recorded delegation/proxy graph (tighter bound, but needs recorded edges).
- status: open
- destination (proposed): TENETS

### A-47: No per-agent non-collusion guarantee within a label set
- type: residual-risk
- source: QUESTIONS.md#165
- statement: "different handle sets within one label set do not supply a per-agent non-collusion guarantee."
- status: open
- destination (proposed): SECURITY

### A-48: Receive output-record commit failure semantics undefined
- type: open-question
- source: QUESTIONS.md#171
- statement: KERNEL-SPEC.md's ABI Records/Errors requires initial output-record validation, and its IPC completion rule plus late-output InvalidArgument describe call/reply explicitly, but the receive error row lists only Timeout and Dead after decoding; neither answers 167-168 nor their archived proposals settle whether a failed receive-output commit consumes its message, notice, or interrupt and associated resources. Current code (kernel/src/message.rs deliver) revalidates before delivery, leaving a message queued on failure, and before taking an exit notice, but answer_record documents a later write failure after consumption as the receiver's loss — current code is evidence of behavior, not approval for an independent oracle. Recommendation: every successful receive output commits transactionally; revalidate and protect validation/copying/delivery together against mapping changes and teardown; on failure, return InvalidArgument with no valid record and no delivery effects, leaving a queued sender/message unchanged, not consuming an exit notice or releasing its object/PID, not marking an abandoned-call notice delivered, not consuming a pending interrupt; no new return-register encoding needed. Alternative: defer and exclude late-invalid receive outputs from the model's conformance claims pending a decision.
- status: open
- destination (proposed): kernel

### A-49: Open call limit per process, not per thread (answer 2)
- type: decision
- source: archive/2026-09-22/ANSWERS.md#2
- statement: A thread may hold several taken-but-unreplied calls up to a per-process limit, new constant MAX_OPEN_CALLS = 64; each open call is charged one page to the receiving process's budget; beyond the limit, receive returns Busy; I5's R3 bound restated as per open call (at most MAX_LEND_PAGES per open call), not per thread.
- status: accepted
- destination (proposed): kernel (rule-id: I5, R3)

### A-50: Handle size correction for cost table
- type: decision
- source: archive/2026-09-22/ANSWERS.md#13
- statement: A handle is about 24-32 bytes (object reference, 64-bit badge, 64-bit stamp), so a handle table page holds 128 handles, not 256; the kernel implementer confirms and the figure goes into the cost table.
- status: accepted
- destination (proposed): kernel

### A-56: Error table and check order live in KERNEL-SPEC, not the model README
- type: decision
- source: archive/2026-09-22/ANSWERS.md#14
- statement: The error table and order of checks are copied into KERNEL-SPEC.md as the single owner; the model conforms to the spec. The decoding-errors-first rule (BadHandle, then TooLarge, then InvalidArgument) is part of what moved.
- status: accepted
- destination (proposed): kernel

### A-52: Manifest JSON integer typing fixed by field
- type: decision
- source: archive/2026-09-22/ANSWERS.md#23
- statement: 64-bit quantities (ids, accounts, byte/page sizes, deadlines) are always JSON strings; small counts (weights, depths, restart limits) are always numbers; a wrong JSON type is an error.
- status: accepted
- destination (proposed): kernel

### A-53: Answers 1,3-12,15-22,24-27 accepted as recommended
- type: decision
- source: archive/2026-09-22/ANSWERS.md#1,3-12,15-27
- statement: Accepted as recommended, with two notes: (17) the pending-request cap WAIT_CAP and R2's round-robin key by (account, label set), not account alone; (24) amended tenet 3 permits host-only test oracles and fuzz drivers in crates outside the workspace build, never linked into anything that runs on the machine; (26) attack success is asserted by the system (kernel, victim, or clean power-off), never by the attacker's own output, applied to every attack case.
- status: accepted
- destination (proposed): kernel

### A-59: Blame goes to most-recently-taken open call, not all open calls
- type: decision
- source: archive/2026-09-22/ANSWERS.md#37
- statement: Blame goes to the account of the call the faulting thread took most recently (kernel records this per thread on receive delivery); the exit notice's blamed_account is that one account. mint accepts any message id among the caller's open calls; a send is never open.
- status: accepted
- destination (proposed): kernel

### A-57: Delayed corruption can misattribute blame
- type: residual-risk
- source: archive/2026-09-22/ANSWERS.md#37
- statement: "delayed corruption can still misattribute; the consequence is a logout."
- status: accepted
- destination (proposed): SECURITY

### A-56: Write requires equal labels; blind write-up removed
- type: decision
- source: archive/2026-09-22/ANSWERS.md#51
- statement: Every write needs equal labels (caller's label set = object's); reads still need the object's labels subset-of caller's. This removes append-only blind writes, the fixed error text, and the name-existence leak through Tcreate. check(caller, object, read|write) becomes: read => object subset caller; write => object = caller.
- status: accepted
- destination (proposed): kernel

### A-57: Panic with open calls is a fault, blamed per answer 37
- type: decision
- source: archive/2026-09-22/ANSWERS.md#55
- statement: A panic with open calls counts as a fault, blamed to the account of the most recently taken open call, not every open call's account.
- status: accepted
- destination (proposed): kernel

### A-58: MAX_LEASE constant and sub-agent budget nesting
- type: decision
- source: archive/2026-09-22/ANSWERS.md#33
- statement: MAX_LEASE = 24h is a constant in KERNEL-SPEC.md. Sub-agents are budgets inside the agent's own budget, so R10 already ends them no later than the agent; an agent holds only its own budget handle so cannot create siblings. The steward refuses lease requests above MAX_LEASE rather than clamping silently.
- status: accepted
- destination (proposed): kernel (rule-id: R10)

### A-59: Declassification via short-lived labelled reader budget
- type: decision
- source: archive/2026-09-22/ANSWERS.md#54
- statement: The steward (system class, unlabelled) creates a short-lived reader budget carrying exactly the item's labels, which reads and snapshots the item and returns it to the steward (system class, so R1 does not apply). No standing universal reader; the steward itself stays unlabelled.
- status: accepted
- destination (proposed): kernel (rule-id: R1)

### A-60: Answers 28-32,34-36,39-50,52-53 accepted as recommended
- type: decision
- source: archive/2026-09-22/ANSWERS.md#28-53
- statement: Accepted as recommended, including (29) the INIT.md name rule (1-64 bytes of [a-z0-9_:+-], starting with a letter), (40) the arg register carries the startup page's address with no fixed address, (41/42) one rule: status 1 = Malformed everywhere. Note (50): a launcher never passes its own connection to a child — each child gets a fresh connection, stated as a rule in CAPABILITIES.md and INIT.md. Note (53): one kernel notice for last handle with a badge closed, minimal — same delivery path and label rule as exit notices, one pending slot per badge, charged to the endpoint's owner.
- status: accepted
- destination (proposed): kernel

### A-61: Fault blame refinements (57 decided, 58 changed)
- type: decision
- source: archive/2026-09-22/ANSWERS.md#57-58
- statement: (57) Blame goes to the most recently taken call that is still open — a replied call is finished and carries no blame. (58) A panic in a thread with no open calls, while other threads of the process hold some, blames nobody: the exit notice says faulted with blamed_account 0 and blamed_labels empty; the crash counts only toward the restart rate limit and, past it, the reboot.
- status: accepted
- destination (proposed): kernel

### A-62: Idle-thread crash with no blame can follow replied-call corruption
- type: residual-risk
- source: archive/2026-09-22/ANSWERS.md#58
- statement: "corruption left behind by a replied call can crash an idle thread later without anyone being blamed."
- status: accepted
- destination (proposed): SECURITY

### A-63: Answers 56,59-68 accepted as recommended
- type: decision
- source: archive/2026-09-22/ANSWERS.md#56,59-68
- statement: Accepted as recommended: (56) receive's record carries each handle's kind; (62) badge-slot cost confirmed in K2 (legacy numbering); (63) the steward enforces MAX_LEASE while the kernel knows only deadlines.
- status: accepted
- destination (proposed): kernel

### A-64: Pinned open calls use abandoned-call notices, option (a)
- type: decision
- source: archive/2026-09-22/ANSWERS.md#81
- statement: Option (a): an abandoned-call notice (flag lives in the open call's own page; server replies to free it); at MAX_OPEN_CALLS, receive refuses only calls, still delivering sends and notices. Not option (b) letting a call's timeout lapse once taken, since a hostile server could pin a caller until the server dies. admit's caps sum to less than MAX_OPEN_CALLS with headroom; parked calls get a server-side deadline. This is the one kernel notice besides exit notices (with answer 69 accepted).
- status: accepted
- destination (proposed): kernel

### A-65: User work inside servers runs in the stride queue; steward residual noted
- type: decision
- source: archive/2026-09-22/ANSWERS.md#84
- statement: Servers doing work for users run in the stride queue with a manifest weight, bounding one request's work. The steward keeps strict system-first ordering (logout/lease-ending stay responsive) but also works for users, so it must bound per-request work and rely on per-account caps; CONTAINMENT.md states the residual.
- status: accepted
- destination (proposed): kernel

### A-66: Steward work is paid by the steward, not the requester
- type: residual-risk
- source: archive/2026-09-22/ANSWERS.md#84
- statement: "steward work is paid by the steward, not the requester" — a stated residual of CONTAINMENT.md.
- status: accepted
- destination (proposed): SECURITY

### A-67: Answers 69-83,85-101 accepted as recommended; 56/57/58/62/64 revisited
- type: decision
- source: archive/2026-09-22/ANSWERS.md#69-101
- statement: Accepted as recommended. Revisited: 56's revised recommendation accepted (check handle kinds by use; the table's kind is documentation); 57/58 replaced by 82 (serve(msg_id), no fallback); 62 moot with 69; 64 as recommended; 100's answer-14 order was a classification, the spec's positional order stands.
- status: accepted
- destination (proposed): kernel

### A-68: One stride queue for every budget; no first flag or priority (revised by A-10)
- type: decision
- source: archive/2026-09-22/ANSWERS.md#103
- statement: One stride queue for every budget; init, the steward, and drivers get large weights in the manifest instead of running first. A driver woken by an interrupt re-enters at the minimum pass (R12). Removes the first flag, its setting rules, and R12's two-tier ordering; class now means trust only (R1's label-check exemption, budget_usage), never scheduling. Stated cost: up to one SLICE of latency for drivers and the steward under load (this latency claim is later withdrawn by answer 166/A-10).
- status: superseded by A-10
- destination (proposed): kernel (rule-id: R12)

### A-69: Answers 102,104-115 accepted as recommended
- type: decision
- source: archive/2026-09-22/ANSWERS.md#102-115
- statement: Accepted as recommended, including (102) MAX_HANDLES = 4096 with TooLarge, and (111) charging table pages in use (the model follows).
- status: accepted
- destination (proposed): kernel

### A-70: Handle/quota/build-configuration decisions
- type: decision
- source: archive/2026-09-22/ANSWERS.md#116-119
- statement: (116) Handles that would exceed MAX_HANDLES for a receiver are refused to the sender (Refused); a reply's excess handles give the caller OutOfMemory and deliver without them. (117) new_connection gains quota: u64; ninep_common error table gains 3 refused (root missing, permission denied, cap reached, quota exceeded), leaving code 2 for not_yours; a self-minted connection counts in the share of the connection it came through. (118) Each server's manifest sizes its bucket count to the (account, label set)s it serves so the cap doesn't bind in normal use; CONTAINMENT.md states the residual channel for an undersized server; byte quotas live in fsd behind grant/disconnect hooks, quota field stays on the wire. (119) A debug-assertions/overflow-checks build is not a special build; the bench boots chosen cases with it; the shipped configuration is still what most cases boot.
- status: accepted
- destination (proposed): kernel

### A-71: Residual channel for an undersized server bucket count
- type: residual-risk
- source: archive/2026-09-22/ANSWERS.md#118
- statement: CONTAINMENT.md states the residual channel for a server sized smaller than the (account, label set)s it serves.
- status: accepted
- destination (proposed): SECURITY

### A-72: Bundle-container domain separation and its own signature domain
- type: decision
- source: archive/2026-09-22/ANSWERS.md#120
- statement: A domain for the package container is added in milestone 2 (legacy numbering); init refuses a manifest that gives keyd the key the loader verifies the bundle with, since keyd cannot see that itself. In addition, the bundle signature gets its own domain now in milestone 1: the loader verifies a signature over domain || length || tar rather than the bare archive, closing the cross-protocol signing hole from both sides.
- status: accepted
- destination (proposed): kernel

### A-73: Answers 121-126 accepted as recommended
- type: decision
- source: archive/2026-09-22/ANSWERS.md#121-126
- statement: (121) WIRE.md states once that a typed protocol minting a narrower capability names its grant/release operations; CONTAINMENT.md says a launcher releases a child's grants on the child's exit notice. (122) Manifest arguments are opaque strings each server's note defines; init validates only count/length/encoding. (123) bootfsd serves only manifest-marked-public entries, never the manifest itself; in milestone 1 seeds live in init's memory and the bundle image at bundle trust; milestone 2 seals them to the machine and generates at first boot. (124) No session or lease holds keys in milestone 1; milestone 2 gives a principal's key the one message shape it may sign. (125) keyd keeps the audit purpose; each audit record is signed; verification is an operator tool in milestone 2. (126) A server draws its first minted badge at random above 2^63 so a restarted server never reissues a badge a client still holds.
- status: accepted
- destination (proposed): kernel

### A-74: Bundle seeds trust residual before milestone 2 sealing
- type: residual-risk
- source: archive/2026-09-22/ANSWERS.md#123
- statement: "in milestone 1 the seeds live in init's memory and in the bundle image, at the same trust as the bundle" — until milestone 2 (legacy numbering) seals them to the machine.
- status: accepted
- destination (proposed): SECURITY

### A-75: Isolation unit is the label set, not the capability set
- type: decision
- source: archive/2026-09-22/ANSWERS.md#150
- statement: Capabilities bound authority, labels bound information flow, data moves only along labels. Two budgets with different handle sets but equal label sets are one trust domain (a handle passed between them is not a crossing); two budgets with differing label sets are the smallest domains the OS distinguishes (R1). No mechanism change: this is what R1 and check already implement.
- status: accepted
- destination (proposed): kernel (rule-id: R1)

### A-76: confined is a per-boot, whole-manifest flag with boot-refusal semantics
- type: decision
- source: archive/2026-09-22/ANSWERS.md#152
- statement: confined is one top-level boolean applied to the whole manifest, not per domain. init compares label sets and refuses the boot when differing sets share a servers entry, a volumes entry, an endpoint name in receives/handed, an ipd:*/netd instance, or a core; it also refuses a confined manifest where a labelled domain reads a shared unlabelled volume. Refusal is a boot failure, not a warning (TENETS.md 2). A system server such as the steward carries no labels and is a domain of its own.
- status: accepted
- destination (proposed): kernel

### A-77: The steward push (mirror of declassification)
- type: decision
- source: archive/2026-09-22/ANSWERS.md#153
- statement: One item per push: the target label's owner triggers it through the powerbox with out-of-band approval; the steward (unlabelled) reads the source and writes the item into the labelled volume through a short-lived writer budget carrying exactly the target label set (write needs equal labels, and the steward holds none); no standing path, queue, or batch; audited with the request's labels. The confined domain cannot trigger, name the item for, or pull a push — that closes the B-to-A channel. check is unchanged; the steward declines a labelled session's mount of a shared unlabelled volume and offers the push instead.
- status: accepted
- destination (proposed): kernel

### A-78: A push is one human action; input rate bound by approval rate
- type: security-caveat
- source: archive/2026-09-22/ANSWERS.md#153
- statement: "a push is one human action, so a confined domain's input rate is a human approval rate."
- status: accepted
- destination (proposed): SECURITY

### A-79: Covert-channel claim stated once canonically in TENETS
- type: decision
- source: archive/2026-09-22/ANSWERS.md#151
- statement: Covert communication is stated once, canonically, in TENETS.md (The adversary); every other note points at it. The design's exact and only claim is zero intentional (software-mediated) paths across a label boundary, by construction; the only zero is placement.
- status: accepted
- destination (proposed): TENETS

### A-80: WP-W3b dropped; answer 115 already satisfied by redoubt-rt's Record construction
- type: decision
- source: archive/2026-09-22/ANSWERS.md#154
- statement: redoubt-rt's Record<const N: usize>([u64; N]) is built as Record([0; N]), a written stack array, so every record it passes is already backed and the runtime owes no change. The "untouched page is InvalidArgument" assertion is kernel behaviour unreachable through the HostKernel fake and already lives in two real-boot cases (budget-syscall-attack, lend-untouched-page). No runtime change, no new case; WP-W3b deleted (legacy numbering).
- status: accepted
- destination (proposed): kernel

### A-64: fsd message copy renamed copy_file to avoid derive collision
- type: decision
- source: archive/2026-09-22/ANSWERS.md#155
- statement: copy camel-cases to Copy, which is in the wire generator's RESERVED_TYPES since every generated type derives Copy; the message is renamed copy_file (opcode 17, reply count: u64 unchanged) in NAMESPACES.md and USERLAND.md.
- status: accepted
- destination (proposed): servers/fsd

### A-82: Three-way Read enum restored for parking reads
- type: decision
- source: archive/2026-09-22/ANSWERS.md#156
- statement: The read path gets Read::Done(n)/Read::Wait back; answer_in_place returns Replied/Waiting/NoRoom; serve_parking is serve_with except a Read::Wait request is handed back unanswered with its T-message still in its lend, parked and served again later. A held request's handles are closed when handed back and its handle list emptied. Only read waits in milestone 1 (legacy numbering).
- status: accepted
- destination (proposed): servers/consoled

### A-83: WP-R1c owns the Parked/Admission join, not a server-port package
- type: decision
- source: archive/2026-09-22/ANSWERS.md#157
- statement: A new package (WP-R1c) owned by libs/rt, with its own review round, adds NineServer::admission_mut and NineServer::share_of; Parked<T> stops owning an Admission (Parked::new(longest), with &mut Admission passed to park/resume/resume_first/expired/abandoned), so fids and parked calls are charged in the same buckets and shares.
- status: accepted
- destination (proposed): servers/consoled

### A-65: consoled is the first user of the parking join
- type: decision
- source: archive/2026-09-22/ANSWERS.md#158
- statement: A read with no input parks rather than answering 0 (which looks like a closed console) or an error the client must poll; it's the smallest first user (one file, one wait condition), and ipd needs the same join.
- status: accepted
- destination (proposed): servers/consoled

### A-85: Two test-harness fixes (stale path; conformance runner)
- type: decision
- source: archive/2026-09-22/ANSWERS.md#159
- statement: (a) keyd's and ported servers' test helper include path was stale, causing cargo test -p redoubt-keyd to be red; fixed as its own one-line commit. (b) The ported servers need a server-side conformance runner (libs/rt/tests/common/vectors.rs, vectors::run) recovered as part of WP-R1c, driving the skeleton with a waiting count matching answer 156's new observable.
- status: accepted
- destination (proposed): servers/keyd

### A-86: console_size Option semantics and Redoubt's answer
- type: decision
- source: archive/2026-09-22/ANSWERS.md#162
- statement: The Platform trait's console_size returns Some((cols,rows)) when known, None when not, defaulting to None. On Redoubt the size comes from the console server via consol's size call (opcode 16), cached; a server not serving consol refuses the opcode as Malformed and the platform answers None. consoled takes its size as a manifest argument cols,rows defaulting to 80x24; sshd answers from the SSH pty-req instead. USERLAND-API.md owns the Redoubt side of the Platform contract.
- status: accepted
- destination (proposed): servers/consoled

### A-87: on_resize callback removed
- type: decision
- source: archive/2026-09-22/ANSWERS.md#162 note
- statement: The on_resize(callback) entry in Redoubt.Console is removed with answer 160's recommendation, since a callback delivered via GenServer handle_info is two contracts in one row and with no push there is nothing to deliver.
- status: accepted
- destination (proposed): userland

### A-88: libvterm is reference-only, never built or linked
- type: security-caveat
- source: archive/2026-09-22/ANSWERS.md#162 note
- statement: libvterm (an untracked C git clone at the repo root) is reference only — read for its terminal state-machine and key tables, never built, never linked (TENETS.md tenet 3, no C). Recorded in USERLAND-API.md's console section, ignored like other vendored reference trees.
- status: accepted
- destination (proposed): TENETS

### A-89: Server push via parked call, no endpoint handed over
- type: decision
- source: archive/2026-09-22/ANSWERS.md#160
- statement: IPC primitives are caller-initiated, so server-initiated delivery works by the client calling and waiting: a call meaning "tell me when this happens" that the server parks and answers when the event occurs. resize (consol opcode 17) is an instance: no fields, parked, answered cols,rows on change; a client re-calls after each reply to wait for the next; on a UART where nothing resizes, consoled parks a resize forever rather than refusing it. Redoubt.Console gets await_resize/1 as a message ({:console_resize, cols, rows}), not a callback. The owner overruled the earlier recommendation to drop resize from milestone 1 and chose to keep it, which revealed question 163 (A-43).
- status: accepted
- destination (proposed): servers/consoled

### A-90: Parked call cost — one open-call slot and one admission slot for its duration
- type: security-caveat
- source: archive/2026-09-22/ANSWERS.md#160
- statement: A parked call holds one of the caller's MAX_OPEN_CALLS and one of the server's admission slots (bucket and share) for as long as it waits, which is why parked calls are capped, reported abandoned, and may carry a deadline.
- status: accepted
- destination (proposed): SECURITY

### A-91: Caller ownership and reply validity independent of status; exact register encoding
- type: decision
- source: archive/2026-09-22/ANSWERS.md#167
- statement: Preserves R3, R4, R4b including abandonment consuming the lend, server death returning a live caller's lend, and R4 delivering partial replies on OutOfMemory. Every call return carries lend disposition and reply presence in registers even on error: a1 is 0 none/1 returned/2 consumed; a2 is 0 absent/1 present; a3..a7 zero. Before decoding a recognized call, initialize none for raw lend (0,0) and returned otherwise, with absent reply, so even an earlier bad argument cannot hide retention. present means the complete output record committed, validated readable/writable before delivery and re-checked at completion; the lend is restored before output so a record inside it stays supported. A failed output commit reclaims newly installed reply handles and their table pages, returns the lend, and reports InvalidArgument with absent, overriding an attempted partial reply's OutOfMemory.
- status: accepted
- destination (proposed): kernel

### A-92: CAPABILITIES.md owns the safe-runtime consuming-buffer contract
- type: decision
- source: archive/2026-09-22/ANSWERS.md#167
- statement: CAPABILITIES.md, IPC, owns the safe runtime's consuming-buffer contract: return ownership only when retained, disarm consumed buffers, expose or close every handle in a committed partial reply before translating or discarding an error; other facades must preserve this accounting.
- status: accepted
- destination (proposed): kernel

### A-93: Reply success reports delivery/discard and installed-handle mask
- type: decision
- source: archive/2026-09-22/ANSWERS.md#168
- statement: reply success is a0=0, a1=0 discarded/1 delivered, a2=installed-handle mask (positional bits 0..MAX_MSG_HANDLES-1, zero on discard), a3..a7=0; reply errors keep the existing error code and all-zero payload and leave the call open. A discarded reply successfully closes the call whether abandonment or failed caller-output commit caused it. A partial R4 reply is delivered if its complete record commits, with only its installed slots set in the mask. Delivery is kernel record commit, not application acknowledgement or a promise against later revocation.
- status: accepted
- destination (proposed): kernel

### A-94: CONTAINMENT.md owns the transaction rule for new grants/connections
- type: decision
- source: archive/2026-09-22/ANSWERS.md#168
- statement: CONTAINMENT.md, the shared server library, owns the transaction rule: retain new grants/connections provisionally, roll them and their admission charges back on discard, check required returned-handle slots on delivery; new_connection and keyd's grant need their returned capability, missing it rolls back that new resource; a multi-resource operation needs an explicit per-resource policy.
- status: accepted
- destination (proposed): kernel

### A-95: WP-IPC1 tracks the implementation work
- type: follow-up
- source: archive/2026-09-22/ANSWERS.md#167-168
- statement: WP-IPC1 updates redoubt-sys, kernel completion paths, executable model, redoubt-rt and its client/server wrappers, and existing grant/connection servers together, covering every lifecycle-table row, both widths' encoding, early errors, partial handle delivery, failed output commit/rollback, subsequent address reuse and destructor behavior, and real-kernel server cleanup. Model integration and real timer-based cancellation acceptance remain required before declaring the package complete; host substitutes alone do not establish these boundaries.
- status: open
- destination (proposed): plan (legacy numbering: WP-IPC1)

### A-96: These are spec changes awaiting implementation
- type: security-caveat
- source: archive/2026-09-22/ANSWERS.md#167-168
- statement: "These are specification changes awaiting implementation; no existing code is represented as conforming."
- status: accepted
- destination (proposed): SECURITY

### A-97: One kernel completion must protect mapping/lifecycle state too, not only handle tables
- type: decision
- source: archive/2026-09-22/ANSWERS.md#Architect clarification after R-IPC1-design
- statement: Answer 167's valid output commit and answer 168's single delivery/discard outcome require validation, copying, handle installation/rollback and outcome publication to be protected together against relevant mapping changes and teardown/abandonment; KERNEL-SPEC.md's Output validity and rollback states this explicitly. Equivalent validated-frame pinning still needs completion arbitration; it cannot permit a reply to commit while abandonment consumes the same lend. Post-commit concurrent changes remain outside the promise; no encoding, status, or ownership rule changes. WP-IPC1's concurrent-completion coverage verifies it.
- status: accepted
- destination (proposed): kernel

# DOC1 Inventory — QUESTIONS.md and ASTRA.md (B-series)

## Part 1: QUESTIONS.md (questions 1–168, one item each)

### A-98: Reply-owed ambiguity on receive
- type: decision
- source: QUESTIONS.md#1
- statement: A received message doesn't say whether it came via `call` (reply owed) or `send`, nor whether its buffer is a lend or transfer; recommendation was for `receive` to report the kind and for `reply` to a send's id to error.
- status: resolved
- destination (proposed): kernel

### A-99: Receiving while still owing a reply
- type: decision
- source: QUESTIONS.md#2
- statement: Whether a thread can `receive` again while a reply is owed; recommendation allowed one call per thread with `Busy` otherwise.
- status: resolved
- destination (proposed): kernel

### A-100: Message id uniqueness
- type: decision
- source: QUESTIONS.md#3
- statement: Message ids should be non-zero and never reused so a stale reply/mint can't hit a later message.
- status: resolved
- destination (proposed): kernel

### A-101: Which budget's label R1 checks
- type: decision
- source: QUESTIONS.md#4
- statement: When threads from different budgets wait on one endpoint, R1's label check should compare against the endpoint owner's budget, not the receiving thread's.
- status: resolved
- destination (proposed): kernel

### A-102: Transfer exceeding receiver's free pages
- type: decision
- source: QUESTIONS.md#5
- statement: A transfer within `max_transfer` but exceeding the receiver's free pages should give the sender `Refused` and the kernel moves on.
- status: resolved
- destination (proposed): kernel

### A-68: Server dies with calls queued
- type: decision
- source: QUESTIONS.md#6
- statement: Conflicting docs on whether blocked senders get `Dead` or the endpoint survives its receivers; recommendation follows KERNEL-SPEC.md (endpoint survives, queued senders wait for restart).
- status: resolved
- destination (proposed): kernel

### A-104: Unbounded exit notices when payer destroyed
- type: residual-risk
- source: QUESTIONS.md#7
- statement: When a budget is destroyed, exit notices for processes it killed are owed to creators elsewhere with nothing paying for them, so pending notices could grow unbounded; recommendation charges an exit slot to the creator at `process_create`.
- status: resolved
- destination (proposed): kernel

### A-105: System-class exemption for exit notices and budget_usage
- type: decision
- source: QUESTIONS.md#8
- statement: R1 exempts system-class receivers from the label check, but exit notices and `budget_usage` need receiver ⊇ target labels or `init`/steward never see a labelled agent exit; recommendation extends the same exemption.
- status: resolved
- destination (proposed): kernel

### A-106: System-class children created by user-class holders
- type: security-caveat
- source: QUESTIONS.md#9
- statement: A user-class process holding a system budget handle could create system-class children; recommendation requires the caller's own budget to also be system class (`ClassDenied`).
- status: resolved
- destination (proposed): kernel

### A-107: Handle slot 0 and unbounded start list
- type: decision
- source: QUESTIONS.md#10
- statement: Slot 0's meaning was undefined and the start handle list uncapped; recommendation makes 0 mean "no handle" and adds `MAX_START_HANDLES`=64.
- status: resolved
- destination (proposed): kernel

### A-108: budget_usage counters undefined
- type: open-question
- source: QUESTIONS.md#11
- statement: `budget_usage`'s "-> counters" was undefined; recommendation specifies page/process limit+usage and weight limit+carved weight.
- status: resolved
- destination (proposed): kernel

### A-109: Weight-0 budget holding a process
- type: decision
- source: QUESTIONS.md#12
- statement: R12 divides by weight and revocation scopes have weight 0; recommendation disallows a weight-0 budget from holding a process.
- status: resolved
- destination (proposed): kernel

### A-110: Undefined per-object memory cost
- type: open-question
- source: QUESTIONS.md#13
- statement: What objects cost in pages (including page tables) was unspecified, blocking model/kernel comparison; recommendation adds a cost table (1 page per object, 128 handles/page).
- status: resolved
- destination (proposed): kernel

### A-111: Errors and check ordering unspecified
- type: decision
- source: QUESTIONS.md#14
- statement: Most checks didn't name their error or order, blocking exact-error replay; recommendation adopts the model's table as normative with decoding errors first.
- status: resolved
- destination (proposed): kernel

### A-112: Small ABI clarifications bundle
- type: decision
- source: QUESTIONS.md#15
- statement: Bundle of ABI clarifications: 8-byte record alignment, absolute-µs deadlines with FOREVER=none, saturating relative timeouts, badge-0/W+X double-checking, `MAX_RANDOM`=64.
- status: resolved (partly superseded: MAX_RANDOM later removed, see A-174)
- destination (proposed): kernel

### A-113: u64 ABI argument register-pair layout
- type: decision
- source: QUESTIONS.md#16
- statement: A u64 argument always takes two 32-bit register halves on both widths, removing width cfgs from `redoubt-sys` at a small rv64 register cost.
- status: accepted
- destination (proposed): kernel

### A-114: Per-account caps as a covert channel out of a vault
- type: security-caveat
- source: QUESTIONS.md#17
- statement: A vault session and its owner's unlabelled session share one account, so they share steward pending-request caps and kernel `WAIT_CAP`, letting one fill show as `Busy` on the other (confirmed by the 10^6 model run); recommendation keys both caps by (account, label set).
- status: resolved
- destination (proposed): kernel

### A-115: Sequential steward session ids leak session counts
- type: security-caveat
- source: QUESTIONS.md#18
- statement: The model found sequential session ids leak how many sessions other principals start; recommendation makes every steward-issued id unpredictable (keyed random).
- status: resolved
- destination (proposed): servers/steward

### A-116: No typed-message tables exist
- type: decision
- source: QUESTIONS.md#19
- statement: No typed-message tables existed for blkd/fsd/steward/keyd/sshd/ipd; recommendation adopts WP-W1's generator/table format with `<!-- wire: NAME -->` markers.
- status: resolved
- destination (proposed): kernel

### A-117: Reply layout and buffer-shape rules undefined
- type: decision
- source: QUESTIONS.md#20
- statement: WIRE.md didn't define a reply's layout or how to decide inline vs buffer shape when only the request's fields were considered; recommendation adds a Reply column, buffer-shape-if-either rule, and status word 0.
- status: resolved
- destination (proposed): kernel

### A-118: Wire packing choices
- type: decision
- source: QUESTIONS.md#21
- statement: Packing choices from WP-W1: inline fields 4 bytes/word in words 1-3, buffer holds fields only (opcode in word 0), inline/buffer fixed per message type.
- status: resolved
- destination (proposed): kernel

### A-119: Compound fields (label sets, IP prefixes)
- type: decision
- source: QUESTIONS.md#22
- statement: No new wire types for milestone 1 compound fields; use a `bytes` field with inner layout stated in the table.
- status: resolved
- destination (proposed): kernel

### A-72: JSON integer representation ambiguity
- type: decision
- source: QUESTIONS.md#23
- statement: Small integers could be written as number or string ambiguously in manifest JSON; final rule (revised from initial "accept both") fixes each field's JSON type by schema, strings only above 2^53.
- status: resolved
- destination (proposed): kernel

### A-121: C toolchain in host-only test tooling vs Tenet 3
- type: decision
- source: QUESTIONS.md#24
- statement: libFuzzer and littlefs C reference are used in separate, non-workspace, host-only crates, conflicting with Tenet 3's "no C toolchain in the build"; recommendation amends tenet 3 to allow host-only test oracles/fuzz drivers.
- status: resolved
- destination (proposed): TENETS

### A-122: littlefs scope limits
- type: decision
- source: QUESTIONS.md#25
- statement: littlefs milestone-1 scope excludes wear levelling, superblock expansion, and atomic attribute+data commit; accepted with WP-D2 to revisit atomic commit if needed.
- status: accepted
- destination (proposed): servers/fsd

### A-123: Attack tests must not trust attacker's own output
- type: rule-id
- source: QUESTIONS.md#26
- statement: The console doesn't identify which process printed a line, so a hostile program could fake a PASSED line; rule requires attack success be asserted by the system, never the attacker's own output.
- status: resolved
- destination (proposed): plan (attack suite)

### A-124: Typed operations written into a 9P file (no opcode slot)
- type: decision
- source: QUESTIONS.md#27
- statement: `ipd`'s connect/listen/close as writes to `/net/tcp/N/ctl` have no wire opcode field; recommendation encodes contents as a u32 opcode followed by buffer-shape encoding.
- status: resolved
- destination (proposed): kernel

### A-125: Handle kinds in typed-message tables
- type: decision
- source: QUESTIONS.md#28
- statement: Tables named a handle's slot but not its expected object kind; recommendation names the kind in the table for docs/generated helper checks (later revised to check-by-use, see A-153).
- status: superseded by A-153
- destination (proposed): kernel

### A-126: Manifest name rules for arbitrary JSON strings
- type: security-caveat
- source: QUESTIONS.md#29
- statement: The manifest parser accepted any JSON string including empty, NUL, U+FEFF or C1 controls as names that become endpoint/volume/path names; recommendation restricts to 1-64 bytes of `[a-z0-9_:+-]` starting with a letter, plus byte-for-byte name comparison and reserved opcode 0.
- status: resolved
- destination (proposed): kernel

### A-4: Revocation doesn't reach in-flight messages
- type: security-caveat
- source: QUESTIONS.md#30
- statement: R10 closes handles in tables but a queued/taken message through a destroyed-budget-stamped handle was still delivered with its badge and replies (including new handles) still reached the sender; recommendation fails queued messages with `Dead` and discards replies whose stamp is destroyed.
- status: resolved
- destination (proposed): kernel

### A-128: Blame outlives a send
- type: security-caveat
- source: QUESTIONS.md#31
- statement: The served account was cleared only by `reply`, so a thread serving a `send` (which can't be replied to) that later faults would wrongly blame the sender; recommendation sets the served account only while a `call` is open.
- status: resolved
- destination (proposed): kernel

### A-129: What counts as blocked sender for WAIT_CAP
- type: decision
- source: QUESTIONS.md#32
- statement: Whether WAIT_CAP counts only queued messages or also callers whose message was taken; recommendation counts only queued messages.
- status: resolved
- destination (proposed): kernel

### A-130: Unbounded requester-chosen leases
- type: security-caveat
- source: QUESTIONS.md#33
- statement: A powerbox request could ask for `lease = u64::MAX` (saturating to FOREVER/no deadline), and sub-agents could outlive their parent and exhaust the sponsor's processes; recommendation adds `MAX_LEASE` (24h) and carves sub-agents from the agent's own budget with leases ending no later than the agent's.
- status: resolved
- destination (proposed): servers/steward

### A-131: Approval screen must show requester identity and reject bidi/format chars
- type: security-caveat
- source: QUESTIONS.md#34
- statement: The approval screen must show the session kind and steward-assigned name, and "control characters stripped" misses bidi/format characters (U+202E, U+2066, U+200B); recommendation whitelists printable ASCII for rendered fields.
- status: resolved
- destination (proposed): servers/steward

### A-132: Free text from labelled sessions reaching unlabelled approval screen
- type: security-caveat
- source: QUESTIONS.md#35
- statement: A vault session's free-text request reason/note (64 chars each) reached `approve@box` unchecked — a channel out of the vault; recommendation shows only fixed steward-generated text for labelled requests.
- status: resolved
- destination (proposed): servers/steward

### A-133: blkd/fsd power-loss safety contract undefined
- type: decision
- source: QUESTIONS.md#36
- statement: littlefs's power-loss safety depends on block-device torn-write behavior, which IO-ARCHITECTURE.md didn't state; recommendation defines blkd's contract (sector overwrite, in-order completion, torn write persists a prefix, sync waits for flush) and records littlefs's lack of data checksums as an accepted limit.
- status: resolved
- destination (proposed): servers/blkd

### A-134: Blame and mint ambiguity with multiple open calls per thread
- type: decision
- source: QUESTIONS.md#37
- statement: After answer 2 allowed several open calls per thread, "the account the thread is serving" and "the message a caller is serving" (mint) no longer said which call; recommendation was to blame every account with an open call (later revised to most-recent-open-call, one account).
- status: resolved (revised)
- destination (proposed): kernel

### A-135: Server library's admit(account) same shape flaw as A-114
- type: security-caveat
- source: QUESTIONS.md#38
- statement: The shared server library's `admit(account)` still admitted per account after answer 17 keyed kernel/steward caps by (account, label set), so a vault session could fill fsd's admission slots under its owner's unlabelled session.
- status: resolved
- destination (proposed): servers (shared library)

### A-136: Startup block format undefined
- type: decision
- source: QUESTIONS.md#39
- statement: INIT.md said what the startup block holds but not its tags/layouts; recommendation adopts R1's `SBlk`/`NmSp`/`Hndl`/`Argv` format (later replaced by a single typed wire message, see A-172).
- status: superseded by A-172
- destination (proposed): kernel

### A-137: How a process finds its startup block
- type: decision
- source: QUESTIONS.md#40
- statement: `process_start` gave no way for a child to learn its startup page address; recommendation carries the page-aligned address in the `arg` register (no fixed address).
- status: resolved
- destination (proposed): kernel

### A-138: 9P call word contents undefined
- type: decision
- source: QUESTIONS.md#41
- statement: WIRE.md said a 9P message travels in a lend but not what the call's four words hold; recommendation makes them all zero with status 1 ("not a 9P message") if not.
- status: resolved
- destination (proposed): kernel

### A-139: No common malformed-request error code across protocols
- type: decision
- source: QUESTIONS.md#42
- statement: WIRE.md had no cross-protocol error status for a malformed typed request; recommendation reserves code 1 as `Malformed` in every protocol's error table.
- status: resolved
- destination (proposed): kernel

### A-140: I10 wording after exit-slot creator-charging
- type: decision
- source: QUESTIONS.md#43
- statement: I10 needed rewording once exit slots were charged to the creator: destroying a child budget restores parent usage only once "killed" notices are received or dropped.
- status: resolved
- destination (proposed): kernel

### A-141: Send handles/page-tables when receiver can't pay
- type: decision
- source: QUESTIONS.md#44
- statement: Nothing said what happens to a send's handles/page-tables when the receiver can't pay; recommendation counted page tables in R4's check, and had the message stay queued for handle `OutOfMemory` (later reversed, see A-169).
- status: superseded by A-169
- destination (proposed): kernel

### A-142: R4a uncovered case at MAX_OPEN_CALLS
- type: decision
- source: QUESTIONS.md#45
- statement: A thread already waiting in `receive` when its process reaches `MAX_OPEN_CALLS` was uncovered; recommendation returns `Busy` and keeps the call queued.
- status: resolved
- destination (proposed): kernel

### A-143: I7 delegation of receive rights across budgets
- type: decision
- source: QUESTIONS.md#46
- statement: A receive right delegated to another budget receives messages checked only against the endpoint owner's labels; accepted as intended delegation, with misuse being a buggy server, not the kernel.
- status: accepted
- destination (proposed): kernel

### A-144: Page-table freeing and address placement unstated
- type: decision
- source: QUESTIONS.md#47
- statement: Page-table freeing and `map_anon` address placement were unstated, affecting exact usage replay; recommendation frees page tables mapping nothing and leaves `map_anon` placement to the kernel's choice.
- status: resolved
- destination (proposed): kernel

### A-145: Crash blame keyed by account alone leaks vault sessions
- type: security-caveat
- source: QUESTIONS.md#48
- statement: A vault session crashing a shared server three times would log out its owner's unlabelled sessions too — same leak class as A-114; recommendation keys blame, its limit, and the logout by (account, label set).
- status: resolved
- destination (proposed): kernel

### A-146: Lend disposition when R10 fails in-flight calls to a destroyed endpoint
- type: decision
- source: QUESTIONS.md#49
- statement: As in R3, a lend stays with the server, charged to it, until reply or death.
- status: resolved
- destination (proposed): kernel

### A-147: One badge shared by all of a client's processes (identity confusion)
- type: security-caveat
- source: QUESTIONS.md#50
- statement: Every handle copy carries the same badge and `process_start` copies handles into children, so all of a user's processes share one 9P connection/fid table — a hostile agent could read/close/wipe its owner's open files; recommendation has launchers mint a fresh connection per child and key fids by (badge, account, label set) as defense in depth.
- status: resolved
- destination (proposed): servers (shared library)

### A-148: Writing up destroys/reveals under no-write-down
- type: security-caveat
- source: QUESTIONS.md#51
- statement: `check`'s no-write-down let an unlabelled caller truncate/overwrite/remove labelled files it can't read, and `Tcreate`'s "file exists" error revealed names in an unlistable directory; recommendation requires equal labels for create/truncate/remove, blind write-up becomes append-only with one fixed error.
- status: resolved
- destination (proposed): servers (shared library)

### A-149: Labelled metadata flows down through walk/directory read
- type: security-caveat
- source: QUESTIONS.md#52
- statement: A walk returned a target's qid (version) with no read check, and directory reads returned every entry's stat — a covert channel out of the vault; recommendation treats qid/metadata as a read and filters directory listings.
- status: resolved
- destination (proposed): servers (shared library)

### A-75: Account-0 admission bucket and dead-client fid leaks
- type: security-caveat
- source: QUESTIONS.md#53
- statement: Account 0 ("none") let every system-class caller share one admission bucket enabling lockout, and nothing released a dead client's fids, letting a crashed/hostile agent exhaust quota; recommendation admits account 0 per badge and adds a kernel notice on last-handle-closed per badge.
- status: resolved
- destination (proposed): kernel

### A-79: System-class (steward) reader can't satisfy check for declassification
- type: decision
- source: QUESTIONS.md#54
- statement: `check` compares only label sets, so the unlabelled steward couldn't read a labelled item to snapshot/stat it for declassification; recommendation drives declassification reads through the label owner's own session (later clarified as a short-lived reader budget carrying exactly the item's labels).
- status: resolved
- destination (proposed): servers/steward

### A-76: Does a panic count toward crash blame
- type: decision
- source: QUESTIONS.md#55
- statement: A Rust panic exits via `process_exit` with cause `exited`, not `faulted`, so the most common hostile-input crash might never be blamed; recommendation blames an exit while open calls are held like a fault.
- status: resolved (changed: blamed per A-134/A-179's most-recent-open-call rule)
- destination (proposed): kernel

### A-77: Handle kinds can't be checked as originally specified
- type: decision
- source: QUESTIONS.md#56
- statement: No syscall reported a handle's kind so a generated helper had nothing to check; options were receive-record kinds, a query call, or check-by-use; recommendation initially chose receive-record kinds, later revised to check-by-use (`WrongObject` on first use, table kind is documentation).
- status: resolved (revised)
- destination (proposed): kernel

### A-80: Which open call a fault blames
- type: open-question
- source: QUESTIONS.md#57
- statement: Ambiguity between "most recently taken call still open" vs "most recently taken call even if replied to"; left open, then replaced by item 82's `serve`-tracked current call.
- status: superseded by A-179
- destination (proposed): kernel

### A-81: Panic in a thread with no open calls while sibling threads hold calls
- type: open-question
- source: QUESTIONS.md#58
- statement: Blame is per-thread, so a fault in a thread with no open calls would blame nobody; recommendation proposed falling back to the process's most recent open call, later changed to "blame nobody, no fallback" and kept by item 82.
- status: superseded by A-179
- destination (proposed): kernel

### A-82: Timing of Dead for a revoked call
- type: decision
- source: QUESTIONS.md#59
- statement: Confirms the caller gets `Dead` immediately on revocation, not when the server replies, matching answer 49.
- status: resolved
- destination (proposed): kernel

### A-83: Handles-vs-pages OutOfMemory/Refused split
- type: decision
- source: QUESTIONS.md#60
- statement: Editor's reading of answer 44: pages/page-tables get `Refused` (R4), handles get `OutOfMemory` with message queued; later reversed so every delivery failure is `Refused` (item 72).
- status: superseded by A-169
- destination (proposed): kernel

### A-84: blamed_labels field needed for keyed blame
- type: decision
- source: QUESTIONS.md#61
- statement: Keying blame by label set (answer 48) needs a `blamed_labels` field in the exit notice, not originally stated.
- status: resolved
- destination (proposed): kernel

### A-85: Badge notice mechanism details
- type: decision
- source: QUESTIONS.md#62
- statement: Badge slot costs 1 page/128 slots (pending kernel confirmation), re-mint withdraws a pending notice, unreceived-message handles count as held, label rule uses last holder's budget, notices precede messages; later made moot when item 69 removed badge notices from the kernel.
- status: superseded by A-266 (S1-style simplification, badge notices removed)
- destination (proposed): kernel

### A-89: Does the kernel enforce MAX_LEASE
- type: decision
- source: QUESTIONS.md#63
- statement: Only the steward enforces MAX_LEASE; the kernel knows deadlines, not leases (constant later moved to CAPABILITIES.md, see A-175).
- status: resolved
- destination (proposed): kernel

### A-161: Startup-block Hndl name rule inconsistency
- type: decision
- source: QUESTIONS.md#64
- statement: Startup-block `Hndl` names followed `startup.rs`'s looser rule (non-empty, no NUL) instead of INIT.md's manifest name rule; recommendation applies one rule in one place.
- status: resolved
- destination (proposed): kernel

### A-86: Where the loader stub finds the ELF image
- type: open-question
- source: QUESTIONS.md#65
- statement: PACKAGES.md left unspecified where the loader stub finds the ELF image; recommendation makes it a named startup-block entry pointing at parent-mapped pages.
- status: resolved
- destination (proposed): kernel

### A-163: R10's message-in-flight reach and badge slots belong in WP-K2
- type: decision
- source: QUESTIONS.md#66
- statement: Placement decision: R10's reach into in-flight messages and badge slots (later removed by item 69) belong to WP-K2, since WP-K1 has no endpoints/messages.
- status: accepted
- destination (proposed): plan

### A-164: INIT.md worked example missing vault read-access scenario
- type: open-question
- source: QUESTIONS.md#67
- statement: The worked example didn't show the vault session's read access to its owner's unlabelled volume, which answer 51 relies on.
- status: resolved
- destination (proposed): kernel

### A-165: KERNEL-SPEC.md miscounts object kinds
- type: decision
- source: QUESTIONS.md#68
- statement: Editorial: spec says "five kinds" of objects but lists four.
- status: resolved
- destination (proposed): kernel

### A-10: Remove badge notices from the kernel (simplification)
- type: decision
- source: QUESTIONS.md#69
- statement: Proposal to delete the badge slot, badge notice and old I15, replacing them with a server-issued random connection id and `disconnect(id)`; stated residual: a launcher dying without disconnecting leaks its children's connections until its own connection is freed, charged against its own (account, label set); saves an estimated 150-250 lines of error-prone kernel code.
- status: accepted
- destination (proposed): kernel

### A-91: Lend charged to both sides while call is open
- type: decision
- source: QUESTIONS.md#70
- statement: Taking a call charges lent pages (and their page tables) to the receiver as well as the caller, removing the "over page limit" budget state; cost: a server's budget must cover open lends up front (~4 MiB for 64 open 9P calls).
- status: accepted
- destination (proposed): kernel

### A-93: Define "abandoned call" once
- type: decision
- source: QUESTIONS.md#71
- statement: Editorial: define "abandoned call" once in R3 instead of restating it in R3/R4b/R10.
- status: resolved
- destination (proposed): kernel

### A-6: One delivery-failure outcome — Refused to sender
- type: decision
- source: QUESTIONS.md#72
- statement: A message is delivered only if the receiver's budget can pay for everything it brings; otherwise sender gets `Refused` and the kernel moves on — `receive` never fails for want of pages. Reverses answer 44's "stays queued" outcome.
- status: accepted
- destination (proposed): kernel

### A-8: Class is inherited; budget_create takes no class argument
- type: decision
- source: QUESTIONS.md#73
- statement: A class check on `budget_create` alone guards only one of three doors (process_create/budget_destroy also need it); what actually protects system budgets is never handing them to users (item 79).
- status: accepted
- destination (proposed): kernel

### A-171: Process object *is* the exit slot
- type: decision
- source: QUESTIONS.md#74
- statement: A process is charged to its creator, outlives death until its exit notice is received/dropped, and is freed with the creator's budget — removes the exit slot as a separate object, without changing answer 7's guarantee.
- status: accepted
- destination (proposed): kernel

### A-12: Startup block becomes one typed wire message
- type: decision
- source: QUESTIONS.md#75
- statement: The startup block becomes one WIRE.md message (`namespace`, `handles`, `argv` as `bytes` fields) decoded by `redoubt-wire`, replacing a second framing format with CRCs that protect nothing since the parent writes both sides; saves 200-300 lines in `redoubt-rt`.
- status: accepted
- destination (proposed): kernel

### A-14: A budget's own page always charged to its parent
- type: decision
- source: QUESTIONS.md#76
- statement: A v4 clause of R6 removes the revocation-scope special case in accounting.
- status: accepted
- destination (proposed): kernel

### A-18: random returns one u64
- type: decision
- source: QUESTIONS.md#77
- statement: Drops `MAX_RANDOM`, a buffer, and a range check; a 32-byte seed now takes four calls.
- status: accepted
- destination (proposed): kernel

### A-175: MAX_LEASE moved out of the kernel spec
- type: decision
- source: QUESTIONS.md#78
- statement: The kernel never reads MAX_LEASE (see A-160); the constant moves to CAPABILITIES.md and out of `redoubt-sys`; the steward still refuses leases over 24h.
- status: accepted
- destination (proposed): kernel

### A-176: Only init and the steward should hold system-class budget handles
- type: security-caveat
- source: QUESTIONS.md#79
- statement: Every server's startup block included its budget; a compromised `ipd` could then create system-class children with forged admission/blame and a labelled reader. Recommendation: server startup blocks omit `budget`, a manifest granting one is refused, with an attack test.
- status: resolved
- destination (proposed): kernel

### A-177: A narrowing handle is always a revocation scope
- type: security-caveat
- source: QUESTIONS.md#80
- statement: To mint a connection narrowed to a child's budget a server must hold that budget handle — a destroy right — so a compromised `fsd` could end every session; recommendation has the steward pass servers only purpose-built scopes, never a budget holding processes, with an attack test.
- status: resolved
- destination (proposed): kernel

### A-178: Open calls can be pinned; server never told a call was abandoned (High)
- type: security-caveat
- source: QUESTIONS.md#81
- statement: "High" — Bob can park 64 lent calls at `ipd` with tiny timeouts, filling `MAX_OPEN_CALLS` so `receive` returns `Busy` for everything including netd's frames, killing every SSH session; recommendation adds an abandoned-call notice, freeing at limit only calls (still delivering sends/notices), and requires admit caps to sum under `MAX_OPEN_CALLS` with headroom.
- status: resolved
- destination (proposed): kernel

### A-179: Blame can be steered in event-driven servers (High)
- type: security-caveat
- source: QUESTIONS.md#82
- statement: "High" — `ipd`/`sshd` park calls and later resume an old one on an interrupt/send, so "most recently taken" is a bystander's call; Bob can crash `ipd` on his own connection while Alice's call is newest, logging Alice out. Recommendation adds `serve(msg_id)` to name the thread's current call; event work with no current call blames nobody. Attack test: a crash triggered by a send while a bystander's call is parked blames nobody.
- status: resolved
- destination (proposed): kernel

### A-180: Fresh-connection operation protocol undefined
- type: decision
- source: QUESTIONS.md#83
- statement: Answer 50's fresh-connection mechanism had no protocol; recommendation makes every 9P endpoint serve typed operations (word 0 nonzero = opcode), with `new_connection` (opcode 2) minting a connection rooted at/below the caller's, carrying item 69's connection id.
- status: resolved
- destination (proposed): kernel

### A-181: User work at system priority inside servers (CPU amplification)
- type: security-caveat
- source: QUESTIONS.md#84
- statement: Bob could make `fsd`/`keyd`/`ipd` do expensive work with no user budget running meanwhile; recommendation restricts strict system-first ordering to `init`, the steward and drivers, with user-serving work running in the stride queue at manifest weight, bounding one request's work. Stated residual: that cost is paid by the server's weight, not the requester's.
- status: accepted (with note)
- destination (proposed): kernel

### A-182: Shared pools that aren't carved (volume fill, handle-table growth, connection-loop channel)
- type: security-caveat
- source: QUESTIONS.md#85
- statement: (a) Bob fills a shared volume so Alice's saves fail; (b) Bob floods `fsd` with handles growing its table until it can't pay; (c) a vault session looping on fresh connections consumes `fsd`'s budget — a channel. Recommendation: a byte quota per attach root, closing unrequested handles, and sizing per-client caps to fit the server's budget.
- status: resolved
- destination (proposed): servers/fsd

### A-183: Revocation must reach handles inside queued messages
- type: security-caveat
- source: QUESTIONS.md#86
- statement: A revoked handle arriving in an already-queued message becomes a zombie connection if badges are reused; recommendation sweeps handles in unreceived messages (arrive as 0) and forbids badge reuse.
- status: resolved
- destination (proposed): kernel

### A-184: System callers share one fairness group (DoS / vault channel)
- type: security-caveat
- source: QUESTIONS.md#87
- statement: R2 grouped every account-0 sender as `(0, {})`, so a busy `fsd:data` filling WAIT_CAP at `blkd` starves `fsd:alice-secrets` — a DoS and a vault channel; recommendation includes the sender's budget id in the fairness group key for account 0.
- status: resolved
- destination (proposed): kernel

### A-185: Global counters as a covert channel (message ids, PIDs)
- type: security-caveat
- source: QUESTIONS.md#88
- statement: Global message-id/PID counters let one process observe traffic-rate gaps of another, including a vault's; recommendation scopes message ids to the receiving process, draws PIDs at random from free ASIDs, and shows `ps`/`budget` only the caller's (account, label set).
- status: resolved
- destination (proposed): kernel

### A-186: Carving under one top budget is a channel
- type: security-caveat
- source: QUESTIONS.md#89
- statement: A vault session's leases change Alice's top budget's free limits, probeable by her unlabelled agent; recommendation splits each principal's top budget into fixed sub-budgets per (principal, label set) at boot.
- status: resolved
- destination (proposed): servers/steward

### A-187: An agent can lock out its sponsor
- type: security-caveat
- source: QUESTIONS.md#90
- statement: An agent sharing its sponsor's buckets can fill the steward's and fsd's caps for up to 24h, preventing even lease termination; recommendation adds a fair share per badge within a bucket and always accepts lease-ending from the sponsor ahead of admission. Attack test: agent floods steward/fsd; Alice still opens a file and ends the lease.
- status: resolved
- destination (proposed): servers/steward

### A-188: Logout isn't a lockout; agents survive it
- type: security-caveat
- source: QUESTIONS.md#91
- statement: Bob crashing `fsd:data` three times, being logged out, then logging back in (or agent carrying on) could reboot the box after three more crashes; recommendation makes the third blamed crash destroy every budget of that (account, label set), sessions and leases alike, refusing new sessions until the window passes.
- status: resolved
- destination (proposed): kernel

### A-189: Audit file and approval-waiting timing are unlabelled sinks
- type: security-caveat
- source: QUESTIONS.md#92
- statement: A labelled request's target landed in the audit file and notification timing reached the unlabelled session; recommendation carries request labels on audit records (read under `check`) and restricts notification reach to channels whose labels ⊇ the request's plus `approve@box`.
- status: resolved
- destination (proposed): servers/steward

### A-190: process_create accepts a badged exit endpoint (spray attack)
- type: security-caveat
- source: QUESTIONS.md#93
- statement: Anyone could spray exit notices at a server via a badged exit endpoint; recommendation requires badge 0 (`NotPermitted` otherwise).
- status: resolved
- destination (proposed): kernel

### A-191: Approval screen shares sshd with most hostile input
- type: security-caveat
- source: QUESTIONS.md#94
- statement: A `sunset`-type bug reached from Bob's channel could control the approval screen; network floods could delay approvals; and CONTAINMENT.md's "no owner exemption at any sink" seemed to contradict sshd carrying a vault channel to its owner. Recommendation states sshd as the one sink cleared for a label (only the owner-authenticated pty channel, no forwarding/subsystems/exec), stating the milestone-1 residual; milestone 2 gives `approve@` its own instance.
- status: resolved
- destination (proposed): servers/sshd

### A-192: keys in a lease is a signature oracle
- type: security-caveat
- source: QUESTIONS.md#95
- statement: A hijacked agent could sign relayed SSH user-auth blobs, letting a peer log in as the owner elsewhere; recommendation restricts a lease's `keys` to approval-named keys, with keyd badges naming one key and one purpose.
- status: resolved
- destination (proposed): servers/keyd

### A-193: Badge slots charged to init (moot with A-166)
- type: decision
- source: QUESTIONS.md#96
- statement: If badge notices are kept, charge badge slots to `init` (creator of every server's endpoint); moot once item 69 is accepted.
- status: superseded by A-166
- destination (proposed): kernel

### A-194: init's blame report to the steward has no message table
- type: open-question
- source: QUESTIONS.md#97
- statement: No typed message table existed for `init`'s blame report to the steward; recommendation adds one typed message (S2 writes the table), with the R3 case expecting the logout signal to name (account, label set).
- status: resolved
- destination (proposed): kernel

### A-195: A table can't mark a typed message as a send
- type: decision
- source: QUESTIONS.md#98
- statement: Every milestone-1 typed message is a `call`; a `kind` column is added when a protocol first needs a transfer.
- status: resolved
- destination (proposed): kernel

### A-196: Deliberate exit with open calls is a blamed fault
- type: decision
- source: QUESTIONS.md#99
- statement: A server that means to exit must reply to every open call first, per spec.
- status: resolved
- destination (proposed): kernel

### A-197: Decoding-order classification vs sequence confusion
- type: open-question
- source: QUESTIONS.md#100
- statement: Whether "BadHandle, then TooLarge, then InvalidArgument" in ANSWERS.md was a sequence or a classification; confirmed as a classification, with the spec's positional order standing.
- status: resolved
- destination (proposed): kernel

### A-198: Direction of steward/reader-budget communication
- type: decision
- source: QUESTIONS.md#101
- statement: The steward `call`s the reader budget, which fills the steward's lend with the snapshot, matching "labelled callers can only submit requests."
- status: resolved
- destination (proposed): servers/steward

### A-199: Per-process handle limit (MAX_HANDLES)
- type: decision
- source: QUESTIONS.md#102
- statement: K1 caps a process's handle table at 32 pages (4096 handles), unspecified in the design; recommendation adds constant `MAX_HANDLES`=4096 with `TooLarge` on overflow, distinguishable from OutOfMemory.
- status: resolved
- destination (proposed): kernel

### A-200: Budget "first" flag mechanism for answer 84 (later fully replaced)
- type: decision
- source: QUESTIONS.md#103
- statement: Editor's invented mechanism (a `first` flag settable only by first-class callers under a system-class parent, run ahead of the stride queue) was replaced entirely: no `first` flag, no strict priority — one stride queue for all budgets with large manifest weights for init/steward/drivers; class means trust only. Stated cost: up to one SLICE of latency for drivers/steward under load.
- status: resolved (replaced)
- destination (proposed): kernel

### A-201: Delivery point of the abandoned-call notice
- type: decision
- source: QUESTIONS.md#104
- statement: The notice is delivered once, on the holding thread's next receive on the endpoint the call came in on; server library ensures each serving thread keeps receiving.
- status: resolved
- destination (proposed): kernel

### A-202: MAX_OPEN_CALLS limit refuses only calls
- type: decision
- source: QUESTIONS.md#105
- statement: At the limit, calls stay queued (R2 skips them), sends/notices still arrive, and receive no longer returns Busy for the limit.
- status: resolved
- destination (proposed): kernel

### A-203: PID retention semantics under answer 74
- type: decision
- source: QUESTIONS.md#106
- statement: A finished process keeps its PID until its notice is received, but stops counting against its budget's process limit when it dies.
- status: resolved
- destination (proposed): kernel

### A-204: Reply side of answer 72 for undeliverable handles
- type: decision
- source: QUESTIONS.md#107
- statement: A reply whose handles don't fit the caller gives OutOfMemory (later folded into item 116: the reply is still delivered, handles dropped).
- status: resolved
- destination (proposed): kernel

### A-205: New editor-invented message layouts (startup, ninep_common)
- type: decision
- source: QUESTIONS.md#108
- statement: The `startup` message fields and `ninep_common` table (`disconnect` opcode 3, `root: string` in `new_connection`) are the editor's, fenced until WP-R1b generates them.
- status: resolved
- destination (proposed): kernel

### A-206: New invariant I15 (abandoned calls reported exactly once)
- type: rule-id
- source: QUESTIONS.md#109
- statement: Every abandoned call is reported exactly once and stays open until replied to, reusing the number of the old badge-notice invariant.
- status: resolved
- destination (proposed): kernel

### A-207: 58-under-82 fallback confirmation
- type: decision
- source: QUESTIONS.md#110
- statement: A thread with no current call blames nobody, with no fallback.
- status: resolved
- destination (proposed): kernel

### A-208: Handle-table cost accounting discrepancy (kernel vs model)
- type: decision
- source: QUESTIONS.md#111
- statement: The kernel charges by pages-in-use for a handle table with holes while the model charges `ceil(handles/128)`, causing WP-C1 replay divergence; recommendation adopts pages-in-use since handles are never moved to compact the table.
- status: resolved
- destination (proposed): kernel

### A-209: Startup message can't be decoded from its page as specified
- type: decision
- source: QUESTIONS.md#112
- statement: A typed message has no overall length and the decoder refuses trailing bytes, but INIT.md said the rest of the startup page isn't read; recommendation prefixes the page with a u32 byte length.
- status: resolved
- destination (proposed): kernel

### A-210: Opcode collision between ninep_common and a server's own protocol
- type: decision
- source: QUESTIONS.md#113
- statement: WIRE.md didn't say whether/how a 9P server's own protocol coexists with ninep_common on one endpoint; recommendation reserves opcodes 1-15 for ninep_common, server protocols use 16+, enforced by the generator.
- status: resolved
- destination (proposed): kernel

### A-211: disconnect naming a stranger's connection id
- type: decision
- source: QUESTIONS.md#114
- statement: ninep_common error table gains code 2 `not_yours` for a disconnect naming an id the caller didn't receive, indistinguishable from a nonexistent id so nothing is revealed.
- status: resolved
- destination (proposed): kernel

### A-212: OutOfMemory during record decoding, unlisted in error table
- type: decision
- source: QUESTIONS.md#115
- statement: Backing an untouched reserved page during record decoding could surface OutOfMemory at a stage where the error table doesn't list it; recommendation forbids decode-time allocation — an unbacked record page is InvalidArgument, and the runtime must touch record buffers first.
- status: resolved
- destination (proposed): kernel

### A-213: MAX_HANDLES enforcement at delivery
- type: decision
- source: QUESTIONS.md#116
- statement: A message or reply's handles pushing the receiver/caller past MAX_HANDLES: message is Refused to sender (per answer 72); reply's excess handles give the caller OutOfMemory with the reply still delivered minus those handles.
- status: resolved
- destination (proposed): kernel

### A-214: ninep_common additions found during implementation (quota, refused, fair-share minting)
- type: decision
- source: QUESTIONS.md#117
- statement: `new_connection` gains `quota: u64`; error table gains code 3 `refused` (root missing/denied/cap/quota); a connection a client mints for itself counts in the share of the connection it came through, closing a share-escape via re-minting.
- status: resolved
- destination (proposed): servers/fsd

### A-215: Bucket-slot exhaustion as a covert channel
- type: security-caveat
- source: QUESTIONS.md#118
- statement: A server tracking a fixed number of (account, label set) buckets could refuse a latecomer for want of a slot, revealing that others hold state — an undocumented channel; recommendation sizes bucket counts to the manifest's declared set and states the residual for undersized servers; moves byte quotas out of the shared library into fsd.
- status: resolved
- destination (proposed): servers/fsd

### A-216: Debug-assertion kernel builds conflict with Tenet 6
- type: decision
- source: QUESTIONS.md#119
- statement: Bench boots six cases with debug assertions/overflow checks on, which found real UB and two SMP bugs, but Tenet 6 forbade "special test builds"; recommendation amends the tenet to clarify this isn't a special build.
- status: resolved
- destination (proposed): TENETS

### A-217: Bundle signature domain separation (keyd raw-byte signing risk)
- type: security-caveat
- source: QUESTIONS.md#120
- statement: keyd signing only self-computed digests closes it from its own side, but the boot-bundle container (`signature || tar`) had no domain and 25 bytes of domain+length fit inside a ustar header name field; recommendation gives the package container its own domain (deferred to milestone 2) and, per owner addition, gives the *bundle* signature its own domain in milestone 1 too, plus `init` refusing a manifest that hands keyd the bundle-verification key.
- status: resolved
- destination (proposed): servers/keyd

### A-218: No ninep_common-equivalent pattern for typed-protocol grant/release
- type: decision
- source: QUESTIONS.md#121
- statement: keyd invented its own `grant`/`release` operations for freeing per-client state since typed servers had no equivalent of ninep_common; recommendation states the pattern once in WIRE.md, with launchers releasing grants on a child's exit notice.
- status: resolved
- destination (proposed): servers/keyd

### A-219: Manifest argument semantics unspecified
- type: decision
- source: QUESTIONS.md#122
- statement: INIT.md's manifest table didn't define what server "arguments" are; recommendation makes them opaque strings init passes through unchanged, each server's note defining its own, init validating only count/encoding.
- status: resolved
- destination (proposed): kernel

### A-220: keyd private seeds sit readable inside the unencrypted bundle
- type: security-caveat
- source: QUESTIONS.md#123
- statement: keyd's manifest-argument seeds sit inside the signed-but-unencrypted boot bundle served by bootfsd at `/boot`, reachable by any session — anyone who can read `/boot` can read the box's private keys; recommendation makes bootfsd serve only manifest-marked-public entries, never the manifest itself, with the residual stated (seeds trusted at bundle-level until milestone 2 seals them to the machine).
- status: resolved
- destination (proposed): servers/keyd

### A-221: Milestone-1 sessions/leases must not hold arbitrary signing keys
- type: security-caveat
- source: QUESTIONS.md#124
- statement: The worked example gave Alice's session "sign with Alice's keys" and CAPABILITIES.md gave a lease `keys`, but keyd's grant mints only the granter's own key/purpose (ssh_host, audit); recommendation removes `keys` from milestone-1 sessions/leases entirely, deferring to milestone 2 with key-to-message-shape binding.
- status: resolved
- destination (proposed): servers/keyd

### A-222: Signed audit records ownership
- type: decision
- source: QUESTIONS.md#125
- statement: keyd's `audit` purpose existed but no note said audit records are signed or verified; recommendation has WP-S2 sign each appended record, with an operator verification tool deferred to milestone 2.
- status: resolved
- destination (proposed): servers/steward

### A-223: Minted badges collide with stale handles after a server restart
- type: security-caveat
- source: QUESTIONS.md#126
- statement: Servers restart minted badges at a fixed starting value while clients still hold pre-restart handles, so post-restart grants could match stale handles; recommendation draws the first minted badge at random above 2^63.
- status: resolved
- destination (proposed): servers (shared library)

### A-224: Cost table silent on per-process kernel storage (PROCESS_IMPL_PAGES)
- type: open-question
- source: QUESTIONS.md#127
- statement: A process object costs one page in the table but saved thread contexts take PROCESS_IMPL_PAGES; recommendation states the true cost (own page + PROCESS_IMPL_PAGES-1 + one page/thread for IPC state) and has the kernel charge it once WP-K4 lands.
- status: open
- destination (proposed): kernel

### A-225: Endpoint cannot be destroyed (deliberate design gap)
- type: open-question
- source: QUESTIONS.md#128
- statement: Endpoints die only with their owner budget, so a process could spend budget pages on unreclaimable endpoints, bounded by the budget; recommendation states this in spec rather than adding `endpoint_destroy`, with revocation scopes as the reclaim mechanism.
- status: open
- destination (proposed): kernel

### A-226: Cross-directory rename impossible with plain 9P2000
- type: open-question
- source: QUESTIONS.md#129
- statement: Plain 9P2000 rename works only within one directory, blocking `File.rename("/a/x","/b/x")`; recommendation adds a typed `rename` operation atomic within a volume, cross-volume falling back to copy+remove (POSIX EXDEV-like).
- status: open
- destination (proposed): servers/fsd

### A-227: File.stat/chmod semantics with no mode/owner/atime fields
- type: open-question
- source: QUESTIONS.md#130
- statement: No mode/owner/atime exist below the capability model; recommendation synthesizes a fixed mode, reports mtime/size honestly, and makes chmod/chown no-ops rather than :enotsup, since Mix/escript tooling depends on chmod succeeding.
- status: open
- destination (proposed): userland

### A-228: Fid behavior across a remove
- type: open-question
- source: QUESTIONS.md#131
- statement: NAMESPACES.md allows remove to succeed while another connection holds a fid (avoiding an "in use" channel) but doesn't say what the holder then sees; recommendation keeps the fid serving the unlinked file until clunked (POSIX-like, cheap under littlefs COW).
- status: open
- destination (proposed): servers/fsd

### A-229: No descriptor inheritance means pipelines need distinct stdio names
- type: open-question
- source: QUESTIONS.md#132
- statement: Redoubt has no descriptor table/inheritance, so pipeline children need distinct per-child bound names or output collides with input; recommendation adds `/dev/stdin`, `/dev/stdout`, `/dev/stderr` namespace entries, bound to `/dev/cons` for interactive children.
- status: open
- destination (proposed): userland

### A-230: Who serves a pipe (no pipe object)
- type: open-question
- source: QUESTIONS.md#133
- statement: A pipe is just a 9P file somebody serves; recommendation has the session's shell VM serve it (needs serve/reply natives + server-side 9P codec in beamlet), reusing the same machinery `System.cmd` capture needs.
- status: open
- destination (proposed): userland

### A-231: Program image copied fresh on every launch (no shared text/demand paging)
- type: open-question
- source: QUESTIONS.md#134
- statement: The launcher copies the ELF into fresh child-charged pages every launch; accepted for milestone 1 as the reason the steward may cache a VM image, with a question for WP-R2 about read-only shared-cache mapping.
- status: accepted (residual noted)
- destination (proposed): plan (M1, legacy numbering)

### A-232: Launching native programs from Elixir needs three new beamlet natives
- type: open-question
- source: QUESTIONS.md#135
- statement: Milestone 1's shell must call process_create/process_map/process_start as beamlet natives plus a startup-block writer, with namespace/handle policy staying in Elixir and encoding in Rust.
- status: open
- destination (proposed): userland

### A-233: Where the Elixir OS-facade modules live
- type: open-question
- source: QUESTIONS.md#136
- statement: Whether `Redoubt.Namespace`/`Redoubt.Process`/`Redoubt.Budget` etc. are embedded in the VM or a separate Mix package; recommendation puts them in a Mix package in `redoubt/elixir/`, versioned separately from the VM.
- status: open
- destination (proposed): userland

### A-234: Two error vocabularies (POSIX atoms vs Redoubt errors)
- type: open-question
- source: QUESTIONS.md#137
- statement: `File` expects POSIX atoms while Redoubt has its own error vocabulary (`refused`, `not_yours`, label/budget errors); recommendation keeps Redoubt's own atoms everywhere and maps to POSIX only at the `File`/`prim_file` shim boundary.
- status: open
- destination (proposed): userland

### A-235: Transfer within one budget should be free of charge
- type: open-question
- source: QUESTIONS.md#138
- statement: R4 as written would refuse a same-budget transfer for want of free pages even though usage is unchanged; recommendation states a transfer costs the receiving budget only what it doesn't already pay for (free within a budget), while a lend is charged to both sides even within a budget.
- status: open
- destination (proposed): kernel

### A-236: Notices after an endpoint is destroyed
- type: open-question
- source: QUESTIONS.md#139
- statement: Destroying an endpoint abandons taken calls but there's no endpoint left to deliver the abandonment notice on; recommendation makes `Dead` from receive the server's cue to reply to every open call it took there, with no notice following.
- status: open
- destination (proposed): kernel

### A-237: Charging for lending untouched reserved pages
- type: open-question
- source: QUESTIONS.md#140
- statement: The ABI refuses an untouched record but a call's lend of untouched-but-reserved pages is backed/charged to the caller like map_anon, and a caller that can't pay gets InvalidArgument since call's row has no OutOfMemory; recommendation states this explicitly in R3/the call row for model/kernel agreement.
- status: open
- destination (proposed): kernel

### A-238: System caller can open a bucket per chained 9P connection
- type: security-caveat
- source: QUESTIONS.md#141
- statement: Admission keys account 0 by badge, folding a share into its parent only when client matches caller, so a system client minting connections for itself opens a fresh bucket each time and can exhaust the server's buckets; recommendation folds by (account, label set) match, ignoring badge, while admission still keys by badge — costing system-to-system delegation its own share (the conservative direction).
- status: open
- destination (proposed): servers (shared library)

### A-239: Device object cost and ownership undefined
- type: open-question
- source: QUESTIONS.md#142
- statement: The cost table had no Device row and no stated owner; recommendation charges one page to the owning budget (the loader-handout budget), revoked with that budget like an endpoint, with no `device_destroy`.
- status: open
- destination (proposed): kernel

### A-240: Loader device-classification rules undocumented and security-relevant
- type: security-caveat
- source: QUESTIONS.md#143
- statement: The loader's undocumented rules (DMA flag from `compatible`=virtio, interrupt controllers excluded from device list, boot refused if an entry names RAM or wraps) are security-relevant — an interrupt controller as a device object would let its holder mask anyone's interrupts; recommendation writes all four rules plus the Devs tag layout into BOOT.md. A follow-up adds: the kernel must also refuse a Devs entry naming the PLIC (currently only a loader heuristic keeps it out).
- status: open
- destination (proposed): kernel

### A-241: Two holders can map the same MMIO device handle; revocation doesn't unmap
- type: security-caveat
- source: QUESTIONS.md#144
- statement: A device handle is copyable and untracked, so two processes can both map_device the same range; and revoking a device handle doesn't unmap MMIO a holder already mapped, so a process in another budget keeps register access after the owner budget is destroyed. Recommendation documents the shared-handle model and requires either unmap-on-destroy or explicit "mapping outlives handle" wording.
- status: open
- destination (proposed): kernel

### A-242: Page tables never freed on unmap despite R11's claim
- type: security-caveat
- source: QUESTIONS.md#145
- statement: R11 claims "a page table is freed when it maps nothing" but `unmap` doesn't implement it, letting a process strand its own table pages (bounded, self-charged, but the rule as written is false); recommendation has WP-K3's unmap free empty tables or defers to WP-K5.
- status: open
- destination (proposed): kernel

### A-243: map_device gives no length or device identity
- type: open-question
- source: QUESTIONS.md#146
- statement: map_device returns only an address with no length, and nothing ties a handle to which device it is (WP-K3's own test had to probe with dma_alloc); recommendation changes map_device to return `addr, len` and has the manifest name handles by device.
- status: open
- destination (proposed): kernel

### A-244: Driver restart leaves device pointed at freed DMA frames
- type: security-caveat
- source: QUESTIONS.md#147
- statement: blkd's DMA pages return to the free pool on death, but nothing stops the still-programmed device from writing into them before restart resets it — the concrete form of K3's residual DMA-handle trust; recommendation has the kernel reset a device (virtio status register = 0) when its DMA pages are freed, in `destroy_device`, before the frames return to the pool. Alternative: keep a dead driver's DMA frames out of the pool until reset, at memory cost.
- status: open
- destination (proposed): servers/blkd

### A-245: How the boot bundle reaches bootfsd (bootfs push protocol)
- type: decision
- source: QUESTIONS.md#148
- statement: `bootfsd` never sees the raw bundle: `init` reads it and pushes public bytes via a two-message typed protocol (`add`/`seal`), sealed forever after setup, accepted only on init's founding handle — making answer 123's residual hold by construction.
- status: resolved
- destination (proposed): servers/bootfsd

### A-246: Naming convention for a device with both MMIO and interrupt
- type: decision
- source: QUESTIONS.md#149
- statement: Three packages invented inconsistent naming (`disk`/`disk-irq` vs `uart`/`uart:irq`); recommendation standardizes on `NAME` and `NAME-irq` under the manifest name rule.
- status: resolved
- destination (proposed): kernel

### A-247: "Isolation unit is the label set" left undefined
- type: decision
- source: QUESTIONS.md#150
- statement: The stated property had no formal definition anywhere, risking inconsistent per-package interpretation of "domain"; recommendation formally defines it: capabilities bound authority, labels bound information flow, a handle passed between equal-label budgets is not a crossing, R1 is the boundary, and equal-label budgets with different handle sets are one trust domain.
- status: resolved
- destination (proposed): TENETS

### A-248: Covert-channel non-claim restated/re-argued across docs
- type: decision
- source: QUESTIONS.md#151
- statement: CONTAINMENT.md restated TENETS.md's covert-channel non-claim in its own words, risking drift and misreading as an OS obligation; recommendation keeps TENETS.md as the single canonical statement ("zero intentional software-mediated paths across a label boundary; the only zero is placement") with CONTAINMENT.md pointing at it.
- status: resolved
- destination (proposed): TENETS

### A-249: confined flag semantics pinned (per-boot, label-set comparison, boot refusal)
- type: decision
- source: QUESTIONS.md#152
- statement: The `confined` manifest flag lacked definitions for "entry"/"differing"/"core"; recommendation pins it as one per-boot top-level boolean comparing declared label sets, refusing shared servers/volumes/endpoints/network instances/cores across differing sets (boot failure, not a warning), and refusing a confined manifest where a labelled domain reads a shared unlabelled volume.
- status: resolved
- destination (proposed): kernel

### A-250: Steward "push" mechanism undefined (labelled-domain input channel)
- type: decision
- source: QUESTIONS.md#153
- statement: "Audited push from the steward" was named but undefined, leaving read-down with no stated closure; recommendation defines push as declassification's mirror — one item per push, triggered only by the target label's owner via powerbox approval, written by the unlabelled steward through a short-lived writer budget carrying exactly the target labels, with no standing path/queue/batch. Stated residual: a confined domain's input rate is bounded by a human approval rate.
- status: resolved
- destination (proposed): CONTAINMENT

### A-251: WP-W3b package has nothing to build (already-satisfied requirement)
- type: decision
- source: QUESTIONS.md#154
- statement: WP-W3's "records already backed" deliverable turned out unimplementable/unnecessary because `Record<N>` is already a written stack array and the untouched-page assertion is kernel behavior unreachable via the HostKernel fake; recommendation drops WP-W3b, keeping the assertion in existing real-boot test cases.
- status: resolved
- destination (proposed): plan

### A-252: Wire message named "copy" collides with Rust's derived Copy trait
- type: decision
- source: QUESTIONS.md#155
- statement: A generated message named `copy` camel-cases to `Copy`, a reserved type name, breaking `cargo test -p redoubt-wire-gen`; recommendation renames the message to `copy_file`.
- status: resolved
- destination (proposed): servers/fsd

### A-253: 9P server that must wait needs a three-way read result and call-parking join
- type: decision
- source: QUESTIONS.md#156
- statement: `FileServer::read`'s `Result<usize,_>` can't express "wait for data with no EOF" (needed for console/ipd reads); recommendation adds `Read::Done`/`Read::Wait`, a three-way `answer_in_place` result (`Replied`/`Waiting`/`NoRoom`), and `serve_parking` to hand a call back unanswered — with held-request handles closed and the request re-read from the lend on resumption. Only `read` waits in milestone 1.
- status: resolved
- destination (proposed): kernel

### A-254: Ownership and scope of the park-join implementation (WP-R1c)
- type: decision
- source: QUESTIONS.md#157
- statement: The park-join change has a wide blast radius across the shared 9P skeleton and must not be bundled into a server port; recommendation creates WP-R1c owned by `libs/rt`, recovering an unmerged commit, adding `admission_mut`/`share_of` and making `Parked<T>` stop owning its own `Admission` (shared with fid accounting).
- status: resolved
- destination (proposed): kernel

### A-255: consoled's UART read chosen as the first parking user
- type: decision
- source: QUESTIONS.md#158
- statement: Recommendation builds the park join with `consoled` as first user (smallest possible: one file, one wait condition) rather than shipping a retry-on-error console read that would change /dev/cons's semantics or make a live console appear closed.
- status: resolved
- destination (proposed): servers/consoled

### A-256: Two test-harness breaks block ported servers
- type: decision
- source: QUESTIONS.md#159
- statement: (a) `servers/keyd/tests/keyd.rs` uses a stale `#[path]` that fails `cargo test -p redoubt-keyd` on redoubt today, independent of the R4 port; (b) the server-side 9P conformance vector runner referenced by ported servers' tests doesn't exist. Recommendation fixes (a) as its own one-line commit and recovers the server-side runner as part of WP-R1c so every 9P server runs the corpus.
- status: resolved (a: separate fix; b: resolved via WP-R1c)
- destination (proposed): kernel

### A-257: Server-to-client push mechanism for console resize (owner-decided general rule)
- type: decision
- source: QUESTIONS.md#160
- statement: WIRE.md's "every milestone-1 message is a call" rule conflicted with a proposed `resize` push, and no send/call reaches a client with no open call on a 9P connection (not an endpoint); owner decided the general rule: a server pushes an unprompted event by parking a call the client made and answering it later — no new primitive, no send — with `resize` as an instance, costing one MAX_OPEN_CALLS slot and one admission slot.
- status: resolved
- destination (proposed): kernel

### A-258: WP-B2 scope crept into three packages (IEx, console library, editor app)
- type: decision
- source: QUESTIONS.md#161
- statement: WP-B2 ("IEx on the UART", Size S) grew to include a full console library, keyboard state machine, and TUI editor, making its acceptance test test the editor rather than IEx; recommendation splits into WP-B2 (IEx), WP-B2a (Redoubt.Console + Key), WP-B2b (Redoubt.Ed + Shell.top), each with its own acceptance criteria.
- status: proposed
- destination (proposed): plan

### A-259: console_size contract ownership and default behavior
- type: decision
- source: QUESTIONS.md#162
- statement: `console_size` was implemented with conflicting promised contracts (`{:error,:unknown}` vs "UART defaults to 80x24") and no note owned the Platform contract's Redoubt side, nor how consoled learns a size. Recommendation: `Option` with `None` default (honest for headless platforms), Redoubt platform queries `/dev/cons` via the `consol` `size` opcode, `consoled` learns size from a `cols,rows` manifest argument (default 80x24), sshd answers from the SSH pty-req; USERLAND-API.md owns the Redoubt-side contract.
- status: resolved
- destination (proposed): userland

### A-260: A parked *typed* call is not yet possible (blocks resize)
- type: open-question
- source: QUESTIONS.md#163
- statement: The park mechanism (WP-R1c) reaches only the 9P `read` path, not typed dispatch, so `resize` as a "call that parks" cannot be built until typed dispatch supports parking; recommendation proposes WP-R1d to extend `TypedServer::handle` with a "hold this" outcome, routed through the same hand-back mechanism as the read path. Until it lands, WP-B2a implements only `size`, not `resize`.
- status: open
- destination (proposed): kernel

### A-261: Confined placement vs approved trusted-mediation paths (ASTRA D1 routing)
- type: open-question
- source: QUESTIONS.md#164
- statement: Answer 152's blanket confined-placement rule (no server/endpoint sharing across differing label sets, including the steward) conflicts with required labelled reader/writer helpers, declassification, push, and powerbox request paths — a missing permitted topology, not a question of whether push was approved. Recommendation names an explicit trusted control-plane mediation exception in TENETS/INIT/CONTAINMENT covering only the specified request/approval path, per-item reader/writer ops, and labelled lifecycle supervision, with one worked configuration; residual: named mediators are trusted across the labels they serve.
- status: open
- destination (proposed): TENETS

### A-262: Authority-closure scope under same-label delegation (ASTRA D2 routing)
- type: open-question
- source: QUESTIONS.md#165
- statement: Answer 150 makes equal-label budgets one trust domain where handle-passing isn't a crossing, letting a recipient acquire authority absent from its initial handle set with no new approval — apparently conflicting with TENETS/GAME's per-agent authority-expansion prohibition. Recommendation defines the closure claim over a trust domain's initial granted authority plus human-approved additions, closed under permitted delegation; alternative is an explicit recorded delegation/proxy graph giving a tighter per-agent bound.
- status: open
- destination (proposed): GAME

### A-263: One-slice wakeup promise doesn't follow from stride scheduling (ASTRA D3 routing)
- type: open-question
- source: QUESTIONS.md#166
- statement: Answer 103's promised "up to one SLICE" wakeup for drivers/steward doesn't follow from R12's `max(own pass, current minimum)` rule when several budgets tie at the minimum or a waking budget retains a larger prior pass. Recommendation keeps the single stride queue and `max` rule but replaces the universal one-slice claim with a measured, workload-qualified responsiveness target, with WP-K5 specifying deterministic tie handling.
- status: open
- destination (proposed): plan (M5, legacy numbering: WP-K5)

### A-264: Caller can't distinguish lend/reply disposition from a call's error result (ASTRA D4/C1)
- type: decision
- source: QUESTIONS.md#167
- statement: `Timeout`/`Dead` alone don't distinguish pre-delivery cancellation (buffer restored), post-delivery abandonment (buffer consumed), or server death while waiting (buffer returned), and partial-reply words/handles could be lost on error; recommendation adds an explicit outcome (`lend = none|returned|consumed`, `reply = absent|present`) returned out-of-band from the caller's output record, even on errors, with exact rules for each failure case. Owner approved.
- status: accepted
- destination (proposed): kernel

### A-265: Server can't distinguish delivered vs discarded reply (ASTRA D4/A1)
- type: decision
- source: QUESTIONS.md#168
- statement: R3/`reply` intentionally discard an abandoned reply while closing the open call, but a successful `reply` syscall doesn't tell the server whether the caller actually received a newly minted connection/grant id, so rollback based only on syscall error can strand admission state. Recommendation returns `delivered`/`discarded` plus the installed-handle-slot mask from a successful `reply`; the shared server library exposes this for transaction rollback. Owner approved.
- status: accepted
- destination (proposed): CONTAINMENT

## Part 2: ASTRA.md findings

### A-266: Abandoned lend leaves a stale, safely-accessible Buffer (C1)
- type: residual-risk
- source: ASTRA.md#C1
- statement: A taken call abandoned by timeout/revocation/caller-death leaves its lend with the server per R3, but the safe `libs/rt` `Buffer` wrapper's Deref/destructor assumed the mapping stays valid until drop, so a caller can retain a safe object claiming to own absent memory; if the freed virtual address is reused, the stale object's destructor can unmap the replacement allocation. "This breaks the runtime's memory-safety argument; no cross-budget memory escape was demonstrated." Host-reproduced (not a live-kernel exploit).
- status: resolved (design: A-264; implementation: WP-IPC1)
- destination (proposed): kernel

### A-267: Partial reply delivery loses surviving handle identities (C2)
- type: residual-risk
- source: ASTRA.md#C2
- statement: R4 permits a reply to arrive with some handles missing (successful slots delivered, `OutOfMemory` returned), but `libs/rt/src/ipc.rs`'s `nothing(syscall(&call))?` returned before decoding the reply record, so an installed-but-unexposed handle is neither returned to the caller nor closed — "repetition leaks caller handle slots." Host-reproduced.
- status: resolved (design: A-264/A-213; implementation: WP-IPC1)
- destination (proposed): kernel

### A-268: Output-record writability checked too late, after server-side effects (C3)
- type: residual-risk
- source: ASTRA.md#C3
- statement: KERNEL-SPEC.md requires output-record writability to be checked during decoding, before other effects, but the shared call/send path only read-validates at stage 1 and writability is checked only when the reply is written — so a valid call body in initially read-only backed RAM can reach the server and produce effects (handles possibly already installed) before returning `InvalidArgument`. Traced by source only; no QEMU reproduction.
- status: resolved (folded into WP-IPC1 per architect routing)
- destination (proposed): kernel

### A-269: Unsafe-code ratchet silently treats missing configured source roots as zero unsafe uses (C4)
- type: residual-risk
- source: ASTRA.md#C4
- statement: `tests/unsafe-budget.toml` names five stale paths (pre-move `redoubt/...` locations) that no longer exist; the checker returns success for a missing extensionless path (neither directory nor `.rs`), so the check reported PASS while reporting zero unsafe uses — and even suggesting zero budgets — for paging, sys, rt, keyd areas that visibly contain unsafe code. "This is a demonstrated false-green check, not merely stale prose."
- status: resolved (repaired to fail-closed; found real 11-vs-9 budget violation, later fixed)
- destination (proposed): plan

### A-270: Abandoned-reply success defeats grant/connection rollback (A1)
- type: residual-risk
- source: ASTRA.md#A1
- statement: A caller can abandon a taken call after the server creates a grant/connection but before its reply is delivered; the kernel discards the abandoned reply and returns `Ok(())`, but `keyd` and the ninep skeleton only undo the newly minted record if the reply *call itself* errors — so the record and its admission charge survive although the identifying reply never reached the caller. "Repetition can exhaust the affected share/bucket." No cross-account DoS demonstrated; test only exercises `forget_badge` directly, not actual abandoned-IPC invocation.
- status: resolved (design: A-265; implementation: WP-IPC1)
- destination (proposed): CONTAINMENT

### A-271: MMIO records may pass validation intended for owned RAM (A2)
- type: residual-risk
- source: ASTRA.md#A2
- statement: `user_frame` checks address range/permissions/validity/lent-out bit but never classifies a physical page as RAM or established physical ownership, and record frames obtained this way are passed to RAM-only accessors in `kframe.rs`. On RV32 this likely hits an assertion (panic); on RV64 the loader maps low physical range into the physmap so the access could reach MMIO instead of being rejected — behavior depends on device width/type. Requires an existing MMIO device mapping (not available to an ordinary agent). "No specific RV64 fault or escape is claimed."
- status: open
- destination (proposed): kernel

### A-272: Console rejects unknown requests without closing attached handles (A3)
- type: residual-risk
- source: ASTRA.md#A3
- statement: `consoled`'s unsupported-operation callback replies `MALFORMED` directly, bypassing the normal `finish` path that closes unused request handles, so handles attached to a rejected request remain open without an application owner — "repeated rejected requests can consume console handle-table capacity and its memory budget outside the server's normal admission accounting." Kernel receiver limits bound growth; not unbounded. No executed reproduction (interrupted by a service restriction).
- status: resolved (routed via answer 85 / WP-R1b cleanup requirement)
- destination (proposed): servers/consoled

### A-273: Confinement does not define its trusted-mediation exception (D1)
- type: open-question
- source: ASTRA.md#D1
- statement: INIT.md's `confined` blanket prohibition on cross-label sharing conflicts with CONTAINMENT.md's required steward-mediated declassification/push helpers and CAPABILITIES.md's labelled powerbox path, with no scoped exception or control-plane topology defined. "A strict reading can reject the intended workflow; an ad hoc exception can permit more sharing than the stated guarantee intends." (Same underlying issue as QUESTIONS.md#164.)
- status: open
- destination (proposed): TENETS

### A-274: Per-agent capability closure conflicts with permitted same-label delegation (D2)
- type: open-question
- source: ASTRA.md#D2
- statement: TENETS.md bounds an individual agent's authority to initial grants plus human-approved additions, but CAPABILITIES.md permits copying/narrowing every handle and ANSWERS.md treats equal-label budgets as one trust domain, so a same-label peer can delegate a handle absent from a recipient's initial set with no extra approval — "intended, correct delegation can violate the literal per-agent guarantee and the authority-expansion verdict in GAME.md." (Same underlying issue as QUESTIONS.md#165.)
- status: open
- destination (proposed): GAME

### A-275: Stride scheduling doesn't imply the stated wakeup latency bound (D3)
- type: open-question
- source: ASTRA.md#D3
- statement: RESOURCES.md claims drivers/steward wait at most ~one SLICE, but R12's `max(own pass, current minimum)` wake rule doesn't guarantee selection-next among tied budgets nor bound a waking budget's retained larger pass — "a scheduler can implement the prescribed algorithm and still fail the responsiveness acceptance criterion." (Same underlying issue as QUESTIONS.md#166.)
- status: open
- destination (proposed): plan (M5, legacy numbering)

### A-276: IPC contract doesn't expose ownership/completion outcomes as distinct states (D4)
- type: open-question
- source: ASTRA.md#D4
- statement: The design-level root of C1/A1 (and relevant to C2): the same call error (`Timeout`/`Dead`) can accompany different lend-ownership states, and a successful `reply` doesn't establish the caller received the result — components can implement plausible but incompatible lifetime rules while each individually following spec. Recommended fix is a normative lifecycle table covering: queued-call-cancelled, taken-call-abandoned, server-dies-while-waiting, normal-reply, partial-reply-handle-delivery, and reply-to-already-abandoned-call, each with its lend/status/cleanup disposition. (Root of A-264/A-265.)
- status: resolved (owner approved as A-264/A-265; WP-IPC1 implements)
- destination (proposed): kernel

### A-277: Same-process received-lend alias unmapped while original lender mapping still records the frame (kernel review blocker, R1)
- type: residual-risk
- source: ASTRA.md#section 6 (kernel review blocker) / section 7 R1
- statement: "Incoming loan aliases are insufficiently protected when caller and receiver share a PID" — an existing PID-ownership test permits a same-process received-lend alias to be unmapped/freed while the original lender's PTE still records that frame, so the lender PTE can later restore a stale frame. Source-confirmed via a saved-but-unexecuted regression draft at the time of the blocker; not a demonstrated cross-allocation exploit. Fix subsequently attempted (marking incoming aliases `VALID|S` and lender reservations invalid, validating both markers plus matching physical frames on return) and passed `ipc-outcomes`/`return-lent-unmapped` on both widths, but required three independent reviews before integration.
- status: resolved (fix integrated per section 9, three-review passed; native multi-hart concurrent execution and full process-teardown cases remain uncovered)
- destination (proposed): kernel

### A-278: Retire the legacy syscall/IPC surface after migration (S1)
- type: follow-up
- source: ASTRA.md#S1
- statement: Two syscall/IPC dispatch paths (new and legacy) remain active simultaneously in `kernel/src/arch/riscv/irq.rs`/`redoubt.rs`, comprising 4000+ lines; recommendation follows K4→K5→K6 with K6 as the gate for the first completed containment claim, then deletes the obsolete dispatcher, SID/CID machinery, callback interrupt interface, and unused ABI surfaces. "Do not remove the working bootstrap before replacements exist."
- status: resolved (K6 merged per repo history; verify legacy surface fully retired)
- destination (proposed): kernel

### A-279: Remove unused flatipc workspace crates (S2)
- type: follow-up
- source: ASTRA.md#S2
- statement: `libs/flatipc` and `libs/flatipc-derive` (~1232 lines) have no in-repository consumer besides each other; recommendation removes them after checking for any external compatibility commitment.
- status: open
- destination (proposed): todo

### A-280: Reconcile status documents with the SWARM Claims ledger (S3)
- type: follow-up
- source: ASTRA.md#S3
- statement: BUILD-PLAN.md, ARCHITECT-NOTES.md, and STATUS.md drifted from each other and from SWARM.md's Claims table (already the declared source of truth) — e.g. BUILD-PLAN.md called R4/D1 both "building" and "merged" in different lines, and STATUS.md claimed no virtio driver existed despite a substantial blkd implementation. Recommendation reconciles against Claims and keeps warm-start notes as pointers, not mutable progress copies.
- status: resolved (per architect disposition, editorial)
- destination (proposed): plan

### A-281: Add a smaller kernel-containment acceptance gate before the full product gate (S4)
- type: follow-up
- source: ASTRA.md#S4
- statement: Final milestone-1 acceptance depends on the whole stack (beamlet, init, storage, networking, steward, SSH, leased agent); recommendation adds an intermediate real-QEMU gate proving kernel primitives alone (hostile-code preemption/deadlines, subtree revocation with in-flight messages/lends, victim responsiveness) before layering the full SSH/BEAM milestone on top. "The smaller gate proves kernel primitives only; it must not be presented as validation of the future steward, approvals, networking, or complete user experience."
- status: proposed
- destination (proposed): plan

### A-282: Grow one client API (redoubt-os) from real integrated callers, not speculative facade (S5)
- type: follow-up
- source: ASTRA.md#S5
- statement: OS-API.md's nine-module `redoubt-os` facade draft is speculative against the existing capability-explicit `libs/rt` client/handle layer, and its rejection of a `std::fs` shim needs reconciling with PLAN.md's still-described future Rust `std` backend. Recommendation: keep the draft future-facing, add only operations needed by the next real callers, and resolve the `std` direction in one owning document. "C1/C2 make stabilizing the underlying ownership/error contract especially important first."
- status: open
- destination (proposed): plan

### A-283: Bench registration for ported D1/R4 servers is incomplete (recovered branches)
- type: follow-up
- source: ASTRA.md#section 11 ("Reuse and blockers", item 1)
- statement: `blkd-build.toml`, `blkd-host-tests.toml`, `bootfsd-build.toml`, `consoled-build.toml`, and `r4-host-tests.toml` exist on old branches but were never carried into current `tests/`, so current `tests/host-tests.toml` runs only signing and testbench — server host tests and both-width builds are not automatically run. Old per-server unsafe-budget entries (blkd=4, bootfsd+consoled combined=0) were also omitted from current config. "A fail-closed checker can reject a bad configured root, but cannot detect an entirely omitted component without a coverage inventory."
- status: open
- destination (proposed): todo

### A-284: K4's IPC baseline predates the R3/R4b outcome fixes; naive whole-file port would regress them
- type: residual-risk
- source: ASTRA.md#section 11 ("Reuse and blockers", items 2-4)
- statement: The recovered K4 branch's `mem.rs`/`message.rs`/`redoubt.rs` predate answers 167-168 and the same-PID borrowed-alias fix (A-277); whole-file replacement would lose protected borrower mappings, identity-checked return, and observable ownership/delivery/completion rollback. Recommendation: port only the process hooks into current code, preserving current mapping/IPC semantics, and test their composition with process exit. K4 is also not acceptance-complete: `tests/process.toml` doesn't cover the full planned hostile process-map/handle-list/weight-zero/badged-exit-endpoint/notice-exhaustion attack gates.
- status: open
- destination (proposed): plan (legacy numbering: WP-K4)

### A-285: Recovered host model is semantically obsolete against current answers
- type: residual-risk
- source: ASTRA.md#section 11 ("Reuse and blockers", item 5)
- statement: The recovered `wp-m0` model's `src/sched.rs` still implements two `first` scheduling tiers explicitly removed by accepted answer 103 (A-200), and its `src/syscall.rs`/`src/kernel.rs` represent IPC completion as bare `Result<Ret,Error>` with no ABI record buffers, exposing no committed partial replies, lend disposition, or delivery masks per answers 167-168 (A-264/A-265). "It is not a missing lifecycle model, but an older one. Historical mutation-test claims are not evidence that it conforms to today's contracts."
- status: open
- destination (proposed): plan (legacy numbering: WP-M0/model)
