# QA log inventory

Extracted from `.wash/QA.md` before DOC1 deletes it. One item per resolved decision, owner decision, residual risk, security caveat, or open follow-up found in the log. Milestone/WP numbers (WP-K5, WP-K5a, WP-K5b, WP-K6, WP-R2, WP-R3, WP-D3, WP-S2) are legacy numbering, noted inline where they occur.

### Q-1: One-slice wakeup claim was false; replaced with wake-first tie rule plus a measured target
- type: owner-decision
- source: QA thread G1-q166 (revises answer 103, recorded as answer 166)
- statement: The scheduler's advertised "a driver woken by an interrupt runs within about one SLICE" bound does not follow from R12's max(own pass, current minimum) wake rule; no tie or preemption-timing rule existed to back it. Owner chose recommendation A: keep the single stride queue and actual-runtime charging, withdraw the universal one-SLICE claim, and instead specify deterministic wake-first tie handling plus slice-end/deadline-only preemption; wakeup is prompt but not bounded, and no deadline follows from weight.
- status: resolved
- destination (proposed): kernel (scheduler docs), plan/M-legacy-WP-K5 (superseded numbering)

### Q-2: Residual — human-control/steward responsiveness rests on a measured target, not a proof
- type: residual-risk
- source: QA thread G1-q166 (answer 166)
- statement: TENETS guarantee 3 (human control via steward lease termination) now rests on a measured wake/lease-termination latency under a named workload, recorded in RESOURCES.md — not a proven bound. This was an explicit, accepted trade-off, not an oversight.
- status: resolved (accepted residual)
- destination (proposed): TENETS, plan/M-legacy-WP-K5

### Q-3: Unsafe-code coverage ratchet had a real inventory gap (kernel + libs/abi)
- type: decision
- source: QA thread G1-coverage
- statement: A manual scan (using the checker's own counting rule) found on-target unsafe code the budget tool never counted: 5 kernel files (cell.rs, arch/riscv/smp.rs, arch/riscv/physmap.rs, debug/shell.rs, debug/macros.rs) and ~52 uses across libs/abi (the legacy xous ABI), 44 undocumented. Decision: close the gap immediately (before K5 became sole kernel writer) by budgeting everything at honest counts; libs/abi's undocumented uses were recorded explicitly as K6 legacy debt rather than documented now, since K6 deletes that interface.
- status: resolved (merged cf5465b85; verified 0 undocumented outside redoubt-abi)
- destination (proposed): kernel, todo (K6 legacy-debt tracking)

### Q-4: Residual — libs/abi (legacy xous ABI) carries ~44 undocumented unsafe uses until K6
- type: residual-risk
- source: QA thread G1-coverage
- statement: redoubt-abi is budgeted at its honest count (52 uses, 44 undocumented) rather than fully documented, on the basis that K6 deletes the legacy interface entirely. This is real standing debt, not resolved unsafe.
- status: open (tracked as K6 debt; closed by Q-38/Q-3-batch3's final ratchet — see Q-107)
- destination (proposed): todo, beyond (K6)

### Q-5: Kernel allowed the reserved write-without-read PTE encoding via set_flags/process_map
- type: security-caveat
- source: QA thread G1-write-without-read
- statement: The kernel accepted the RISC-V reserved W-without-R page encoding through set_flags(page, W) and process_map(child, ..., W), even though map_anon already refused it and the conformance model refused it in all three paths. This was a kernel/model conformance gap, not a spec option: the privileged architecture marks that encoding reserved, so R11 was amended to state no user mapping is ever writable without being readable, and set_flags/process_map now refuse it with InvalidArgument, matching map_anon.
- status: resolved (merged cf5465b85; write-only-attack test passes rv32/rv64; model mutation R11AllowsWriteOnly caught)
- destination (proposed): kernel, SECURITY

### Q-6: Residual — mutation proof and process_map ordering are not fully symmetric
- type: residual-risk
- source: QA thread G1-write-without-read
- statement: The write-without-read fix's model mutation is proven caught only through set_flags; process_map's own refusal check runs before the mapping's flags refusal rather than being independently exercised the same way. Recorded as a residual in SWARM Review debt, not re-opened as a blocker.
- status: resolved (accepted residual)
- destination (proposed): kernel

### Q-7: Nightly rustfmt component was missing from the dev environment
- type: follow-up
- source: QA thread CT1-fmt
- statement: The build machine's nightly toolchain lacked the rustfmt component, so `cargo +nightly fmt --check` couldn't be verified; the owner approved installing the component (changes local toolchain only). Separately, the same machine lacks the riscv64imac-unknown-none-elf target, so `cargo testbench consoled-build` fails for rv64 there (rv32 passes) — left open as an environment gap.
- status: resolved (fmt); open (rv64 target gap)
- destination (proposed): todo

### Q-8: Startup block gained image_addr/image_len fields for the loader stub
- type: decision
- source: QA thread R2-image-fields (answer 65 precedent)
- statement: INIT.md's `startup` wire table gained `image_addr: u64` and `image_len: u64` (paired presence: both 0 means "no image"), so the R2 loader stub can locate the ELF image its parent mapped in. The stub's own fixed load address (STUB_ENTRY = 0x0020_0000, later 0x1FF0_0000 per MEMORY-LAYOUT) is an implementation constant in a shared crate, not a spec table entry.
- status: resolved (implemented, redoubt-wire-gen regenerated, R2 host side passed red review round 3)
- destination (proposed): servers (loader stub / R2), plan/M-legacy-WP-R2

### Q-9: No syscall let a running process map memory at a self-chosen address — new map_fixed syscall (owner decision, question 172)
- type: owner-decision
- source: QA thread R2-stub-self-map (answer 172)
- statement: The stub, running inside its own started child, had no way to place ELF segments at fixed p_vaddr addresses (map_anon picks the kernel's address; process_map only targets unstarted processes). Owner approved adding `map_fixed(addr, len, flags)`: maps zeroed pages at exactly `addr` in the caller's own space, charged to the caller, refusing (InvalidArgument, nothing mapped) on unaligned/empty/out-of-range/overlapping-existing-mapping; never replaces a mapping; same W^X/W-without-R flag rules as map_anon; other error OutOfMemory. Rejected alternative: parent-side segment mapping (would force init/steward to parse ELFs, violating the "stub is the only ELF parser" design) and static-PIE relocation (would diverge from every other Redoubt binary's fixed-address linking convention).
- status: resolved (owner "go with recommendation"; implemented as WP-K5a)
- destination (proposed): kernel, servers (loader stub), plan/M-legacy-WP-K5a

### Q-10: User space lower bound settled as page 0 included (model had an unapproved floor)
- type: decision
- source: QA thread K5a-addr0
- statement: MEMORY-LAYOUT.md never excluded page 0 from user space, and the kernel already accepted address 0; the conformance model's USER_BASE = PAGE_SIZE floor was an unapproved model-only rule, not a spec gap needing an owner decision. Settled: page 0 is in user space on both widths (map_fixed/process_map/unmap/set_flags all accept it); rationale is that sstatus.SUM stays clear so the kernel never dereferences user pointers, so the usual null-page kernel exploit does not apply — a mapped page 0 can only affect the process that mapped it (or its parent via process_map).
- status: resolved
- destination (proposed): kernel, SECURITY (note the SUM-based null-deref rationale)

### Q-11: map_fixed plan round 1 found a real address-allocator divergence (model vs kernel) — BLOCK
- type: decision
- source: QA thread K5a-plan (red-team round 1, P1-1)
- statement: The model's alloc_va only ever grows from the highest existing mapping, so map_fixed near USER_TOP followed by map_anon would make the model return OutOfMemory while the kernel (which scans a 256 MiB window from mem_default_base) succeeds — every alloc_va user, and any static ELF linked high, would trigger it. Fixed by making alloc_va fall back to first-fit in [KERNEL_CHOSEN_BASE, USER_TOP) when the space above the highest mapping doesn't fit.
- status: resolved (fixed in K5a plan v2, confirmed by red-team re-review)
- destination (proposed): kernel

### Q-12: map_fixed plan round 1 found an unbounded-cost DoS via huge-length occupancy walk — BLOCK
- type: security-caveat
- source: QA thread K5a-plan (red-team round 1, P1-2)
- statement: The initial plan walked the whole requested range page-by-page (and ran tables_needed over the whole range) before any budget check, so a single map_fixed(0x1000, ~1GiB range) call from an empty-budget process could cost roughly 2^26–2^27 kernel iterations, repeatably — unlike every other range-taking syscall (unmap, process_map, find_virtual_address), which are budget- or window-bounded. Fixed with a new `range_available_in`/`subtree_occupied` helper that walks the page-table tree top-down and skips absent subtrees entirely, plus splitting the OOM check into "pages alone" then "pages+tables" so a huge len that already fails on pages never reaches the expensive walk — preserving the spec's declared error order (overlap/InvalidArgument before OutOfMemory).
- status: resolved (bounded cost confirmed under mutation: unfixed version took 13.8s vs a would-be 30s timeout; fixed version ~133us)
- destination (proposed): kernel, SECURITY (DoS-bound rationale worth keeping visible)

### Q-13: map_fixed plan found a W+X/W-without-R check placed after charging, reachable as a kernel panic path
- type: security-caveat
- source: QA thread K5a-plan (red-team round 1, P2-3); confirmed in K5a-review-1
- statement: The original plan re-checked W+X only inside map_page_inner, after the budget charge and under an unconditional `.expect()`, so a bad flag combination reaching that point would panic the kernel rather than refuse cleanly. Fixed by adding map_fixed's own three-way flag check (empty / W+X / W-without-R), copied from process_map, running before any charge — so the post-charge `.expect()` can never observe bad flags.
- status: resolved
- destination (proposed): kernel

### Q-14: Real bug found while implementing map_fixed — tables_needed under-counted missing page tables across parent boundaries
- type: security-caveat
- source: QA thread K5a-review-1 / K5a-review-2 (red-team round 2, N1; "EXTRA" finding)
- statement: `tables_needed` deduplicated missing tables by index within their immediate parent table, so the same index recurring under a different parent (e.g. missing L0 at index 5 in two different GiB ranges) was counted once instead of twice. This under-count could pass the budget check and then hit the guaranteed-success `.expect()` after pages were already charged, panicking the kernel — reachable by any process with roughly 1 GiB+ free budget via map_fixed, process_map, or IPC. Fixed by naming tables by `addr / leaf_size(level)`; the model's table_keys logic was already correct. No boot test had covered this until a dedicated case (tests/map-fixed-tables.toml) was added specifically because it needs ≥1 GiB of budget to trigger.
- status: resolved (fixed in K5a fix round 2, commit 7526e8571; confirmed by mutation causing the exact prior panic)
- destination (proposed): kernel, SECURITY

### Q-15: map_fixed residual — kernel's own map_anon window can be exhausted by map_fixed (self-harm only)
- type: residual-risk
- source: QA thread K5a-plan (P3-6), reconfirmed K5a-review-2
- statement: The kernel's map_anon allocator only scans a 256 MiB window; a process can use map_fixed to fill that window, making its own later map_anon calls fail with OutOfMemory where the (unbounded) conformance model would still succeed. This is the same class of divergence that already existed once map_anon totals exceed 256 MiB, and it only harms the calling process itself — recorded as a residual, not fixed.
- status: resolved (accepted residual, not fixed)
- destination (proposed): kernel

### Q-16: R2 stub had a real build blocker (redoubt-wire needs a global allocator in no_std)
- type: decision
- source: QA thread R2-fix-round-1
- statement: redoubt-wire unconditionally declares `extern crate alloc`, so linking it into a no_std/no_main stub binary needed some `#[global_allocator]` even though typed decoding (what the stub uses) never allocates. Fixed with a zero-sized `NullAlloc` (SAFETY: alloc/dealloc unreachable), instantiated once per binary (stub + fixture-child).
- status: resolved
- destination (proposed): servers (loader stub)

### Q-17: R2 residual — cargo-fuzz was never actually run against the loader stub's ELF parser
- type: residual-risk
- source: QA thread R2-fix-round-1
- statement: cargo-fuzz is not installed in the build environment; the fuzz target (stub/fuzz/fuzz_targets/plan.rs) was only check-compiled against the new BadImage variants, never actually run as a fuzz campaign. This is a real gap in adversarial-input coverage for the ELF-parsing stub, which is the component that processes attacker-influenced image bytes.
- status: open
- destination (proposed): todo, SECURITY

### Q-18: R2 residual — on-target hostile-ELF boot acceptance was blocked on K5a, then completed
- type: follow-up
- source: QA threads R2-fix-round-1, R2-after-K5a
- statement: R2's host-side work (ELF parsing, bounds checks) passed review before map_fixed existed; the actual on-target hostile-ELF boot acceptance test could not run until K5a's real map_fixed landed. Once it did, R2's final round wired the real Call::MapFixed, fixed error-code mapping (OOM→112 "MAP_UNAVAILABLE", everything else→111 "BAD_IMAGE" fail-closed), added checked_add for overflow (→110 "BAD_STARTUP"), and ran the stub-launch bench (all hostile children exit/fault correctly, budget usage returns to empty after each).
- status: resolved
- destination (proposed): servers (loader stub), plan/M-legacy-WP-R2

### Q-19: R2 residual — no kernel-side instruction-cache fence on code installation (icache coherence gap)
- type: security-caveat
- source: QA thread R2-review-4 (P3-4), carried to R2-followups and picked up by K5
- statement: The stub itself adds a `fence.i` before jumping into loaded code, but the kernel has no `fence.i` anywhere: (a) when a parent writes a child's code via map_fixed/process_map and maps it executable, nothing fences before the child fetches it, and (b) multi-hart thread migration will need icache maintenance on the destination hart. Single-hart K5 committed to running `fence.i` after any call that installs/adds an executable mapping (process_map, map_anon, map_fixed, set_flags) and once at boot before first user dispatch; the multi-hart piece (fencing other harts, migration-time fencing) is explicitly deferred to post-M1 SMP work, not K5's.
- status: resolved (single-hart piece); open (multi-hart piece deferred)
- destination (proposed): kernel, plan/M-post-M1 (SMP)

### Q-20: R2 residual — kernel refusals mask the stub's own overlap/segment checks; no bench case proves them independently
- type: residual-risk
- source: QA thread R2-review-4 (P3-3), recorded in R2-followups
- statement: On target, dropping the stub's startup-page exclusion or its segment-vs-segment overlap check still produces the same refusal (111), because the kernel's own map_fixed overlap check masks it — so on-target tests can't distinguish "stub checks it" from "kernel refuses it anyway." Only a host-level unit test actually pins the stub's segment-vs-image check; nothing pins the segment-vs-segment check via a host test.
- status: open (recorded as non-blocking follow-up, not scheduled)
- destination (proposed): todo

### Q-21: R2 follow-ups list (five non-blocking test-coverage gaps from final red review)
- type: follow-up
- source: QA thread R2-followups
- statement: After R2 merged, five P3-level gaps were recorded and left unscheduled: (1) no bench case for the new OOM→112 mapping itself (huge-memsz case would catch a regression to 111); (2) nothing observes that the image's mapping is actually unmapped by the stub — removing the unmap silently passes every test; (3) no host test for the segment-vs-image overlap check (see Q-20); (4) tests/programs/build.rs doesn't watch libs/{sys,wire} Cargo.toml/Cargo.lock for rebuild triggers; (5) the kernel-wide fence.i/icache-maintenance gap (see Q-19), raised for K5/SMP planning.
- status: open
- destination (proposed): todo

### Q-22: R2 follow-up — no stack guard gap between a loaded segment and the stack (accepted as launcher rule)
- type: decision
- source: QA thread R2-review-4 (P3-5), closed via R2-followups item 6
- statement: A test showed a segment ending right at an inside-link-range stack bottom is accepted with no guard gap, and the child runs successfully — this can't happen if the launcher convention is followed. Resolved by writing a launcher rule into PACKAGES.md/MEMORY-LAYOUT.md and BUILD-PLAN WP-R3: launchers must place the startup page and stack outside the range 0x1_0000..0x1FF0_0000 (the stub checks no segment-to-stack gap under that convention).
- status: resolved
- destination (proposed): servers (loader/launchers), plan/M-legacy-WP-R3

### Q-23: K5 scheduler — owner-approved 11-item decision package (stride scheduler redesign)
- type: owner-decision
- source: QA thread K5-plan (OWNER DECISIONS 1-11, approved 2026-09-24)
- statement: The owner approved, as one package, the full WP-K5 scheduler redesign: (1) I13 reading (every blocked thread with a finite deadline gets its call completed by the first kernel entry at/after its deadline); (2) preemption only at slice end, block, exit, or a budget deadline — never on wake alone; (3) current-minimum/floor semantics; (4) deterministic wake-first tie handling; (5) tick charging with remainder (accounting moved to the trap boundary, entry/exit hooks); (6) pass inheritance on budget create/destroy (entry = max(floor, parent.pass); destroy folds work additively into the parent, normalized by weight, later amended to avoid double-counting the entry wait); (7) stride weight defined as free weight plus refusal accounting; (8) stand-in processes for R3 rerun; (9) ROOT_WEIGHT raised from 1,000 to 1,000,000 (root 1,000,000 fully carved, system 250,000, users 749,000, root keeps 1,000 free for init); (10) K5 owns the single-hart fence.i; (11) testbench scope extended to include an independent scheduling oracle (sched_oracle.rs) alongside the icount option.
- status: resolved (implemented across 8 commits, merged 33db4858e)
- destination (proposed): kernel, plan/M-legacy-WP-K5

### Q-24: K5 residual — legacy IRQ callbacks that never return can still hold the CPU (accepted until K6)
- type: residual-risk
- source: QA thread K5-plan (P2-1, "LEGACY CALLBACKS")
- statement: While a legacy direct-switch callback pair is active, budget-deadline destruction and slice-end preemption are deferred until the callback returns through the normal path — so a callback that never returns keeps the CPU indefinitely. Mitigated only by scope: only manifest-granted IRQs can be claimed, and after K5 the only claimants are refusal-test code. The earlier plan's claim that "a callback can't hold off slice ends" was explicitly withdrawn as false. This is a stated residual until K6 removes the legacy callback mechanism.
- status: resolved (accepted residual, not fixed; closed by K6 — see Q-30/Q-107)
- destination (proposed): SECURITY, todo (K6)

### Q-25: K5 residual — legacy direct switches are out-of-model until K6
- type: residual-risk
- source: QA thread K5-plan (P2-5, "LEGACY DIRECT SWITCHES")
- statement: Legacy direct-switch code paths (Hart.running tracked as "the budget actually running," with a budget change at exit deschedule) are explicitly stated as an out-of-model residual, to be closed only when K6 removes the legacy interface.
- status: resolved (accepted residual; closed by K6)
- destination (proposed): todo (K6)

### Q-26: K5 residual — rounding loss and bounded cross-subtree debt-lift delay
- type: residual-risk
- source: QA thread K5-plan (P1-2 churn proof, later amendments)
- statement: Pass-inheritance on budget destroy loses under 1 pass-unit of remainder per destroyed budget (safe, bounded, falls on the churner). Debt parked on a shared parent (e.g. users/system) can delay later siblings by a bounded amount — final measured/asserted bound is roughly ≤2×SLICE per lift, tightened through several rounds of review (the original "at most one slice at the child's weight" claim was found wrong and withdrawn, then re-derived and re-bounded under the corrected additive, entry-wait-aware rule).
- status: resolved (accepted, bounded residual; verified via sched-debt-lift boot case and model property with dedicated mutations R12LiftByMax, R12UnnormalizedLift, R12LiftCountsEntryWait)
- destination (proposed): kernel

### Q-27: K5 residual — R10 (process/budget destroy) cost dominates lease-termination latency, thin margin against target
- type: residual-risk
- source: QA thread K5-plan (final checkpoint, orchestrator's merge-gate note)
- statement: R10 (budget/process destroy sweep) costs roughly 21 ms of virtual time per 2-process destroy and is non-preemptible; it dominates steward lease-termination latency. Measured p99 was 26.3/27.3 ms (rv64/rv32) against a pinned ≤30 ms target — a thin margin explicitly flagged by the orchestrator at the merge gate, not re-optimized before merge. Tracked as a named follow-up (K5-r10-destroy-cost).
- status: resolved (merged with thin margin; follow-up tracked separately, see Q-53/Q-96)
- destination (proposed): kernel, todo

### Q-28: K5 named follow-ups opened at merge, not yet actioned
- type: follow-up
- source: QA thread K5-plan (final resolution note)
- statement: At K5's merge, several items were explicitly deferred and tracked as separate follow-ups rather than folded in: K5-r10-destroy-cost (R10 destroy latency, see Q-27); K5-carve-lead-rescale (weight-carve mid-syscall lead rescaling); K3-irq-level-latch (a goldfish RTC alarm raised while its IRQ object is masked between receives is never delivered on QEMU virt — a possible lost level-interrupt, flagged during K5 measurement as "R5 observation," not a K5 bug); K5-latency-flake and bench-process-flake (unspecified flakiness in latency/process benches). The model's "million-sequence" validation run for the final scheduler also had some property families still running when the merge checkpoint was sent and not yet recorded in model/VALIDATION.md at merge time (a later addendum confirmed all families, including steward_noninterference and a 1,000-iteration flood, passed).
- status: open
- destination (proposed): todo

### Q-29: K5 residual — a console-readable per-pick scheduling trace is a cross-principal information channel
- type: security-caveat
- source: QA thread K5-plan (orchestrator instruction on commit 6, sched-trace)
- statement: The orchestrator flagged that a console-dumpable scheduling trace (every budget's pick, pass, and id) is a cross-principal information channel not covered by the milestone's containment claims, and required it be compiled only under a test-only build feature/cfg, never in the production kernel default build. Implemented: default rv64/rv32 kernel builds contain no SCHED-TRACE string and no trace symbol; a grep/build check enforces this in the commit.
- status: resolved
- destination (proposed): kernel, SECURITY

### Q-30: I13/R12 preemption reading — timeout is a wake, budget deadline preempts
- type: decision
- source: QA thread K5-q-preempt
- statement: The KERNEL-SPEC budget table's `deadline` attribute is the only thing whose passing destroys a budget; R12's "preemption at slice end or at a deadline, never on wake alone" refers to budget deadlines. Timeout results are committed and the thread made runnable at the first kernel entry at or after the timeout (an always-armed timer bounds entry latency), but whether it then runs is R12's business — a timeout wake does not itself preempt.
- status: resolved
- destination (proposed): kernel (KERNEL-SPEC I13 clarification)

### Q-31: R12 "current minimum" and idle-time banking
- type: decision
- source: QA thread K5-q-minimum
- statement: The wake floor is the lowest pass among runnable budgets (including the running one, as last charged), held as a monotone virtual time across idle periods; wake pass = max(own, floor), preventing a sleeping budget from banking credit while idle (attack case: A sleeps with low pass while B runs alone and its pass climbs; without a floor A would resume with stale low pass and dominate).
- status: resolved
- destination (proposed): kernel (KERNEL-SPEC R12 clarification)

### Q-32: R12 tie-break rule among equal-pass wakers
- type: decision
- source: QA thread K5-q-ties
- statement: Later wakers rank ahead of equal-pass queued budgets (LIFO, no starvation since pass advances); simultaneous wakes are reconciled once per kernel entry under an explicit rank rule over kernel-assigned budget ids; a slice-end requeue goes FIFO behind equals. Mutations R12TieQueuedFirst and R12RequeueAhead approved as evidence.
- status: resolved
- destination (proposed): kernel (KERNEL-SPEC R12 clarification)

### Q-33: R12 charging unit — timebase ticks with exact remainder, not whole microseconds
- type: decision
- source: QA thread K5-q-units
- statement: Charging in whole microseconds creates a sub-microsecond gaming channel (a thread running under 1 µs per burst is charged 0). Fix: charge in timebase ticks, minimum 1 tick per deschedule, runtime capped at 2^40 ticks. A second, deeper hole exists for large-weight budgets (weight > 2^20 × ticks floors ticks×STRIDE/weight to 0), fixed by carrying the exact division remainder per budget (`rem = (rem + ticks×STRIDE) mod weight`). Timeouts/deadlines stay in µs per the ABI.
- status: resolved
- destination (proposed): kernel (KERNEL-SPEC R12 clarification)

### Q-34: rdtime frequency discovery — legacy timer ops removed, no replacement in K5
- type: decision
- source: QA thread K5-q-timebase
- statement: K5 removes both legacy timer ops (TIMER_SET_DEADLINE and TIMER_TIMEBASE) and adds no replacement call; rdtime is for relative high-resolution timing, time_now is the unit-bearing clock.
- status: resolved
- destination (proposed): kernel; follow-up recorded separately (Q-35)

### Q-35: follow-up — timebase-frequency consumer needs an R3/INIT startup-block field
- type: follow-up
- source: QA thread K5-q-timebase
- statement: If a future consumer (e.g. beamlet's monotonic clock under B1) needs the counter frequency, it should be added as a startup-block field, owned by the R3/INIT architect as a spec change — not reintroduced as a legacy op.
- status: open
- destination (proposed): todo / plan/M-R3 (INIT)

### Q-36: icache maintenance ownership for K5 (single-hart fence.i)
- type: decision
- source: QA thread K5-q-icache
- statement: K5 owns the single-hart fence.i; a physically-tagged icache holding a previous owner's code in a recycled frame is a correctness hazard (not a privilege one, since it runs in the new owner's context) and is resolved by the new owner's write-then-fence or specific kernel paths.
- status: resolved
- destination (proposed): kernel

### Q-37: named-workload latency bench uses stand-ins, with a mandatory rerun acceptance line
- type: decision
- source: QA thread K5-q-bench
- statement: Stand-ins (steward and driver processes) are acceptable for K5's acceptance, using INIT.md manifest weights, with both lease-termination paths (stand-in steward's budget_destroy and the kernel deadline) reported. Because this narrows acceptance, BUILD-PLAN WP-S2/WP-R3 acceptance must add: "rerun the WP-K5 named-workload latency bench with the real steward and drivers; the numbers must stay within the accepted target."
- status: resolved
- destination (proposed): plan/M-S2 (steward) and plan/M-R3

### Q-38: K5 plan review round 1 — three P1 blockers (charging choke point, budget-churn debt shedding, entry-time expiry panics)
- type: decision
- source: QA thread K5-plan-review (v1 review)
- statement: v1 was BLOCKed on: (P1-1) the plan charged only in `activate_process_thread`, leaving deschedule paths that never charge (a free-CPU channel); (P1-2) a new budget entering at the floor and being destroyed after one slice sheds its pass debt back to the parent, which narrows the approved "new budget enters at the floor" answer and needed an orchestrator rule (pass inheritance through the budget tree); (P1-3) entry-time expiry could kill or invalidate state the trap handler already captured, causing an `expect()` panic (I14). All three were fixed with an account-at-every-deschedule rule, additive pass inheritance, and removal of unsafe `expect()`s on possibly-killed pids.
- status: resolved
- destination (proposed): kernel

### Q-39: K5 plan review — residual: legacy direct-switch budgets are out-of-model
- type: residual-risk
- source: QA thread K5-plan-review (P2-5)
- statement: Legacy direct switches (borrowed-quantum `send_message`) run another budget with no scheduler pick, and the fairness model has no representation of them. `Hart.running` is defined as the budget actually running, and this is listed as an out-of-model residual until K6.
- status: resolved (accepted as residual, not fixed; closed by K6)
- destination (proposed): kernel; SECURITY (residual note); todo (K6 follow-up)

### Q-40: K5 plan review — residual: callback that never returns holds the CPU past its slice
- type: residual-risk
- source: QA thread K5-plan-review (P2-1)
- statement: The plan's claim "a callback can't hold off slice ends" contradicts the deferral of slice-end handling to `finish_isr`: a legacy callback that never returns keeps the CPU. Stated as a residual until K6, backed by the evidence that only manifest processes can claim IRQs.
- status: resolved (accepted as residual, not fixed in K5; closed by K6)
- destination (proposed): kernel; SECURITY residual; todo (K6)

### Q-41: K5 plan review round 2 (N1/N2) — carve weight and pass-inheritance rule needed orchestrator rulings
- type: decision
- source: QA thread K5-plan-review (v2 review)
- statement: v2 was BLOCKed on two new P1s: (N1) the plan's step formula used the budget's limit rather than the free/carved weight, unspecified and never updated on carving; (N2) `max(parent, child)` pass-lift on destroy absorbs an un-normalized descendant's debt onto the wrong ancestor, letting a lightweight grandchild's churn stall an unrelated sibling for tens of seconds under the named workload. Both required explicit orchestrator/architect rulings, not just implementer text.
- status: resolved
- destination (proposed): kernel

### Q-42: K5 plan review round 3 (M1) — additive, not max(), pass-lift propagation
- type: decision
- source: QA thread K5-plan-review (v3 review)
- statement: The v3 fix (`max(parent, candidate)`) still forgave a churned child's work whenever the parent had its own lead above the floor. Ruling: replace with additive propagation, `P.pass = max(P.pass, f) + W/w_P_new`, so a parent's own unpaid lead and a child's separate debt add rather than being merged by max. Verified against spinning-parent and deadline-timed variants plus mutation R12LiftByMax.
- status: resolved
- destination (proposed): kernel

### Q-43: K5 plan review round 4 (Q1) — double-counted entry wait in pass-lift formula
- type: decision
- source: QA thread K5-plan-review (v4 review)
- statement: OWNER DECISION 6 as first written over-charged honest parents because an inherited entry wait was counted again inside the lift. Fix: store entry position `e = max(floor, parent.pass)` at child creation, and on destroy use `W = (child.pass - max(e,f))+ × w_child + child.rem`, so a child's debt is only work done after its entry (create+destroy with no run moves nothing — verified via an idempotence property and mutation).
- status: resolved
- destination (proposed): kernel

### Q-44: K5 plan review round 5 (R1-R4) — boot weight split, loader-bundle interim behavior, latency-bench denominator
- type: decision
- source: QA thread K5-plan-review (v5 review, OK with notes)
- statement: Final ruling: root keeps free weight for init in both kernel and model (root 1,000,000; system 250,000; users 749,000; root keeps 1000 free). Noted residuals folded into acceptance: (R3) loader-bundle processes now run at system's high free weight instead of taking cooperative turns, so any busy-yielding bundle process will dominate until R3 — measured cases must ensure bundle processes block, checked by full-bench rerun, stated as interim; (R2) property (j) "parent pass/rem unchanged" is too strict since carve+return rescaling can drop rem by rounding, so it must assert pass unchanged and rem within a rounding bound.
- status: resolved
- destination (proposed): kernel; todo (R3 interim-behavior follow-up)

### Q-45: K5 plan approval — owner approved all 11 OWNER DECISIONS
- type: owner-decision
- source: QA thread K5-plan-review (final resolution)
- statement: Five plan review rounds (v1 BLOCK 3 P1 → v2 BLOCK N1/N2 → v3 BLOCK M1 → v4 BLOCK Q1 → v5 OK with notes). The owner approved the consolidated plan and all 11 OWNER DECISIONS in chat on 2026-09-24 ("approve all").
- status: resolved
- destination (proposed): kernel; plan/M-K5

### Q-46: sched-debt-lift bound corrected to one round, not a fixed 2 slices
- type: decision
- source: QA thread K5-debt-lift-bound
- statement: With discrete passes, spinners cluster exactly at the floor f or f+step; any nonzero pass-lift above zero loses the equal-pass wake-first tie to spinners still at f and must wait for the whole round. The test bound is corrected to S's first run ≤ (runnable budgets + 2) × SLICE, still decisive against R12UnnormalizedLift (~100 rounds wait). OWNER DECISION 6 itself is unchanged.
- status: resolved
- destination (proposed): kernel (test/spec bound only)

### Q-47: sched-debt-lift residual — one round of delay per destroyed lineage
- type: residual-risk
- source: QA thread K5-debt-lift-bound
- statement: A destroyed lineage can delay one sibling created under the same shared parent within the same round by at most one round, decaying once the floor passes the parent's pass. This is a stated, accepted residual, not a bug.
- status: resolved (accepted as residual)
- destination (proposed): kernel; SECURITY note

### Q-48: K5 code review checkpoint 1 — OK with notes, four P2s on test strength/placement
- type: decision
- source: QA thread K5-code-review-1
- statement: Model (ba154ca3f) and redoubt-stride (7aedd10d6) checkpoint reviewed clean (no P1; 19/19 mutations caught, meaningful differential against 8 planted bugs). Four P2s required fixup commits: shell property (k) didn't catch R12LiftCountsEntryWait; sched_rank property (b) was vacuous for mid-slice wakes; OWNER DECISION 5's "at least 1 tick per deschedule" rule was implemented in neither crate nor model; and the differential's "Kernel" harness was a copy of model logic, not the kernel's real wiring (fixed later to drive redoubt_stride::Cpu directly).
- status: resolved
- destination (proposed): kernel

### Q-49: K5 code review checkpoint 2 — BLOCK-for-merge P1: PREVIOUS_PAIR-keyed deferral lets any process defer all budget deadlines
- type: decision
- source: QA thread K5-code-review-2
- statement: Budget-deadline deferral was keyed on `PREVIOUS_PAIR` (i.e. `in_callback()`), but the legacy, ungated `ReturnToParent` syscall sets that pair outside any real callback for any user process. This let any process put the kernel into permanent "callback mode," deferring every budget deadline in the system indefinitely (defeating "a deadline destroys its budget"), refusing all Redoubt calls system-wide via `in_irq` (including the steward's own `budget_destroy`), and causing a later legacy `Yield` to panic the kernel. Fixed by keying deferral and `in_irq` on `HANDLING_IRQ` (a real callback) and refusing/removing `ReturnToParent` for user processes, with an attack boot case added.
- status: resolved
- destination (proposed): kernel; SECURITY (this was a real privilege/DoS defeat of R12, caught pre-merge)

### Q-50: K5 code review checkpoint 2 residual — discarded-reply race vs I15 "exactly once"
- type: residual-risk
- source: QA thread K5-code-review-2 (P3 item on reply race)
- statement: When a reply wins the race against a timeout, F_WAITING is already clear so the reply is discarded and the call closes with no notice sent — this is accepted as consistent with the caller-death race, but contradicts I15's "reported exactly once" wording unless the spec explicitly states the discard-is-the-report case. Docs commit was directed to reconcile this.
- status: resolved (documented, not changed behaviorally)
- destination (proposed): kernel (KERNEL-SPEC I15 clarification)

### Q-51: K5 code review checkpoint 2 residual — deferred deadlines during genuine callback are unbounded
- type: residual-risk
- source: QA thread K5-code-review-2 (P3 item 2)
- statement: Even after the PREVIOUS_PAIR fix, a genuine callback that never returns holds every budget deadline in the system, not just the CPU — an accepted residual until K6. Also flagged: grants are keyed by PID, so a Redoubt-created process reusing a granted boot process's PID must not inherit its legacy IRQ grant.
- status: resolved (accepted as residual, checked but not eliminated; closed by K6)
- destination (proposed): kernel; SECURITY residual; todo (K6)

### Q-52: R10 budget-destroy cost — non-preemptible, ~21ms, O(object frames), attacker-inflatable
- type: residual-risk
- source: QA thread K5-r10-destroy-cost (open, unassigned follow-up)
- statement: Destroying a budget takes ~21ms of virtual time in non-preemptible (SIE-clear) O(object-frames) sweeps (mark_dying/destroy_marked scan 0..high_frame; budgets_dying is O(E×F)); this cost scales with object frames that any budget can inflate by creating pages, delays every interrupt/timeout on the machine, and dominates driver-wake p99/max latency whenever a lease ends. Scheduled as a follow-up before S2 (the steward): make destroy proportional to the destroyed subtree, or make it preemptible and bounded. K5 records the number in its latency table as an interim measure.
- status: open
- destination (proposed): plan/M-S2 (steward); kernel; SECURITY (DoS amplification via object-frame inflation)

### Q-53: D3 owner decisions 1-11 — network stack scope, protocol, and capability design
- type: owner-decision
- source: QA thread D3-plan (OWNER DECISIONS list)
- statement: D3 approved to: pin smoltcp 0.14.0 (later changed by owner to VENDORED, not pinned-only) with a minimal feature set (alloc, medium-ethernet, proto-ipv4, socket-tcp — no IPv6/DHCP/DNS/UDP/ICMP/fragmentation/ping/log); IPv4-only, static config, TCP-only in D3 (deferring /net/udp against NAMESPACES); netd-to-ipd frames delivered as a `send`+transfer (needing WIRE's new `kind` column); TCP write backpressure via a new `Write::Wait` in libs/rt (amending NAMESPACES' "waiting write is a non-goal"); capability representation via root scopes from ipd's manifest arguments narrowed by a typed `grant(scope)` opcode and `mint_rooted`; the box's own addresses (interface addr, loopback, broadcast, multicast, manifest `self=` prefixes) always refused regardless of scope, with a stated residual that NAT-hairpin addresses outside the manifest's self set cannot be discovered/refused; the pre-R3 evidence path is a dedicated `tests/net` rig booting real netd/ipd through the R2 stub; QEMU virtio-mmio force-legacy=false must be added for [disk]/[net] cases (also affects D2/blkd).
- status: resolved
- destination (proposed): servers/netd; servers/ipd; plan/M-D3

### Q-54: D3 owner decision — stated residuals (ISN predictability, NIC-flood CPU cost, restart DMA hazard, SYN-flood socket hold)
- type: residual-risk
- source: QA thread D3-plan (OWNER DECISIONS item 11)
- statement: Four residuals accepted at plan approval: smoltcp's TCP ISNs originally came from one PCG32 seeded once from the kernel CSPRNG (predictable after a few observations, enabling off-path injection and a connection-rate side channel between unlabelled users — later mitigated, see Q-56); a NIC flood costs netd's weight-1000 CPU; the restart DMA hazard (Q147, same class as blkd — later escalated, see Q-58); a SYN flood can hold a listener's sockets in SYN-RECEIVED for up to a 10s timeout (later tightened to 3s + auto re-listen).
- status: resolved (accepted as residuals; some later mitigated)
- destination (proposed): servers/ipd; servers/netd; SECURITY

### Q-55: D3 plan review v1 — six P1 blockers (peer-count non-evidence, vacuous own-address cases, narrow self set, unauthenticated DMA address, NIC-brick-on-one-packet, understated Q147)
- type: decision
- source: QA thread D3-plan-review (v1 review)
- statement: v1 BLOCKed on: (P1-1) the testbench's guestfwd peer counted forwards opened at boot, not actual guest connections, making every count-based verdict non-evidence — fixed with a `guestfwd=...cmd:<helper>` spawning a process per real connection plus a must-fail "connects twice, expects 1" twin; (P1-2) own-address boot sub-cases passed vacuously under slirp's restrict=on — fixed with a scoped rig self-address plus host-side frame-capture assertions; (P1-3) the QEMU self set (`self=10.0.2.2/32`) was too narrow given libslirp maps the whole 10.0.2.0/24 vnet, plus the DNS resolver alias, to host loopback — fixed by declaring `self=10.0.2.0/24` in the manifest; (P1-4) the rx thread's startup message carrying a physical DMA address unauthenticated let a colliding badge inject an arbitrary DMA target — fixed by passing the address via state set before thread_create, never by message; (P1-5) any frame under 14 or over 1514 bytes (e.g. an honest 802.1Q or runt frame) was treated as a permanent device-breaking lie — fixed by distinguishing structural ring/descriptor violations (real lies) from oversize/runt content (dropped and counted, not broken); (P1-6) Q147 was understated as "same as blkd" — netd always has 16 rx buffers posted, so after netd's death the next network packet DMAs remote-controlled bytes into frames already returned to the pool (escalated as a kernel-level gap).
- status: resolved
- destination (proposed): servers/netd; servers/ipd; tools/testbench

### Q-56: D3 ISN mitigation — CSPRNG-seeded throwaway smoltcp Interface per connect/accept, no fork
- type: decision
- source: QA thread D3-plan / D3-plan-review (P2-2 disposition)
- statement: smoltcp 0.14 exposes no direct reseed hook, but its public `Context` API allows building a throwaway `Interface` seeded fresh from the kernel CSPRNG per active connect, and for passive opens processing the bare SYN through a separately-seeded throwaway before handing the SYN-ACK to the main interface's egress — verified against smoltcp source (tcp.rs:1063 connect, tcp.rs:1919 ingress SYN) and tested (each connect/SYN consumes exactly one seed; ISN equals seed output #2; no observed overlap with the main interface's seed stream).
- status: resolved
- destination (proposed): servers/ipd

### Q-57: D3 ISN residual — off-path injection remains a DoS, plus a cross-user connection-rate channel
- type: residual-risk
- source: QA thread D3-plan / D3-plan-review (P2-2 disposition)
- statement: Even with CSPRNG ISNs, blind off-path injection still needs only an in-window sequence guess (~2^19 tries at an 8KiB window, same as any random-ISN stack); worst case is a reset or garbage bytes that SSH's transport MAC rejects — a denial of service, not a compromise. Separately, admission-slot occupancy and CPU/queue timing remain an observable connection-rate channel between unlabelled users sharing one ipd (no label boundary crossed, since labelled callers are refused).
- status: resolved (accepted as residual)
- destination (proposed): servers/ipd; SECURITY

### Q-58: Q147 (DMA quarantine-on-death) escalated to full strength for netd — arbitrary physical read/write via freed DMA frames
- type: security-caveat
- source: QA thread D3-plan-review (R1, and D3-q-bench context)
- statement: The rx/tx descriptor tables and avail/used rings themselves live inside netd's freed DMA regions, and QEMU's virtio-net re-reads avail.idx and descriptors from guest memory on every packet with no notify required. So whoever next owns those freed frames (even an unprivileged process via ordinary map_anon reuse) can post descriptors pointing at any physical address; the next LAN or broadcast frame is then DMA'd there — an arbitrary physical write with attacker-chosen bytes (rx side), and a symmetric arbitrary physical read sent out to the wire if the device is kicked (tx side). This is a strictly worse restatement of the original "≤16 frames / 32 KiB" bound. The kernel-level quarantine-plus-reset fix is stated as a PREREQUISITE for any netd restart (R3) and for any off-bench use, added as a BUILD-PLAN WP-R3 gate. Interim mitigation: netd resets the device (STATUS=0, poll for 0) on every controlled exit including panic, and no rig case restarts netd.
- status: resolved (design escalated; kernel fix tracked separately, see Q-59 K5b-plan / answer 173)
- destination (proposed): kernel; SECURITY; plan/M-R3 gate

### Q-59: D3 plan review v1 — twelve P2 findings (TX stale-byte leak, crash-blame, labelled-refusal placement, bucket-visibility contradiction, listen-lineage hijack, SYN-flood re-listen, port/socket numbering, socket linger charging, sizing, lying-device boot-loop, rig placement, ack_delay)
- type: decision
- source: QA thread D3-plan-review (P2 list and disposition)
- statement: All twelve P2s were accepted and closed with concrete fixes: TX descriptor length set per-transmit to 12+frame_len (no stale-byte cross-user leak); `iface.poll()` only run when the thread has no current call (no crash-blame misattribution to a parked caller); labelled-refusal check moved into a shared lib dispatch function used by both bin and host tests; bucket-exhaustion testing replacing the vacuous unchanged-socket-count check; listen-port ownership keyed to the first-listening connection plus its clones (not the whole steward lineage) to prevent SYN hijack across principals; ipd sets its own SYN-RECEIVED timeout and re-listens automatically (smoltcp doesn't); socket numbers per-connection and ephemeral ports globally unique including TIME-WAIT; lingering sockets charged until smoltcp frees them with a bounded absolute linger; sizing limits made explicit per named root badge; a non-unicast/multicast MAC refused at bring-up rather than causing a smoltcp panic, and ipd never exits on link faults; rig startup page/stack placed outside the stub's link range per the R3 placement rule; ack_delay left at smoltcp's default (10ms), accepted as fine pre-K5.
- status: resolved
- destination (proposed): servers/netd; servers/ipd; tools/testbench

### Q-60: D3 plan — owner approved all 13 OWNER DECISIONS, changing decision 1 to vendor smoltcp
- type: owner-decision
- source: QA thread D3-plan-review (final resolution) / D3-plan
- statement: Two plan review rounds (v1 BLOCK 6 P1; v2 OK with notes, R1-R5 folded in). The owner approved all 13 OWNER DECISIONS in chat on 2026-09-24, with one change: VENDOR smoltcp (rather than merely pin it), noted as a tension against tenet 5 that the orchestrator flagged. Q147's kernel-side fix is covered separately by answer 173 / WP-K5b.
- status: resolved
- destination (proposed): servers/ipd; plan/M-D3; TENETS (note the tenet-5 tension re: vendoring a dependency)

### Q-61: netd made robust to a missed first-interrupt edge on QEMU virtio (defensive fix from K5 review 5)
- type: decision
- source: QA thread D3-plan (checkpoint/review-5 defensive-change note)
- statement: On QEMU virt, an interrupt raised while the IRQ object was masked (between receives) was observed lost once, specifically on a driver's first receive; no kernel defect was found for this and it is being followed up separately with K3 (see Q-83). netd was made robust regardless: after every IRQ receive returns, and once right after DRIVER_OK before the first receive, it drains the used ring until empty and re-checks the used-ring index before blocking again, so a lost first edge can't stall rx/tx. A host test simulates the fake device completing a buffer without raising the interrupt before the first receive.
- status: resolved
- destination (proposed): servers/netd; todo (K3 follow-up on the root-cause investigation)

### Q-62: D3 residual — full fuzz suite runs load-sensitive but unrelated to D3's own changes
- type: residual-risk
- source: QA thread D3-plan (checkpoint (c) follow-up)
- statement: A fuzz/test run looked load-sensitive under a full run, but wp-d3 changes nothing under kernel/, model/ or tests/programs relative to redoubt — flagged as possibly worth telling K5 about but explicitly stated as not blocking for D3.
- status: open (informational, not actioned within D3)
- destination (proposed): todo (cross-package flag to K5/testbench maintainers)

### Q-63: K5b DMA reset/quarantine trigger design (OD1–OD7 accepted)
- type: decision
- source: QA thread K5b-plan (owner recommendations, orchestrator ruling)
- statement: Frame release-at-process-end (not last-handle-count) is the reset trigger; DMA pages can never be lent/transferred/process_mapped; the reset set is the allocation device plus every device ever map_device'd (per-process bitmask); poll bound is 1000us/device non-preemptible; quarantined frames stay force-charged; quarantine flags a device by MMIO base and sweeps its handles; there is no reset-disable switch (must-fail evidence is a one-time recorded pre-K5b run).
- status: resolved (implemented and merged with K5b)
- destination (proposed): kernel (dma reset/quarantine); docs under legacy WP-K5b numbering

### Q-64: K5b P1-1 — a quarantined device must count as NOT reset, ever
- type: decision
- source: QA thread K5b-plan-review (red BLOCK, then closed in K5b-plan v2 re-review)
- statement: If any slot in the release set S is quarantined, every live run of the dying process is quarantined too, including runs through healthy slots in S — a quarantined slot never satisfies "confirmed" for pooling purposes, closing the arbitrary-DMA hazard (Q147) where a co-holder's frames could otherwise be pooled after a device is flagged.
- status: resolved (kernel + model mutation K5bQuarantinedSlotCountsAsReset, verified in K5b-code-review-3/final)
- destination (proposed): kernel

### Q-65: K5b OD5 amended — quarantine charge stays with the run's own budget
- type: decision
- source: QA thread K5b-plan-review / K5b-plan (P2-2)
- statement: The original OD5 ("force-charge to the grant holder, may exceed its limit") violated I5 (usage <= limit). Amended: the quarantine charge stays on the run's own already-paid budget and migrates to the destroyed top's parent at destroy time, AFTER return_carve returns the top's carve (N1 ordering fix), so no budget ever exceeds its limit.
- status: resolved
- destination (proposed): kernel; SECURITY (invariant I5 preserved)

### Q-66: K5b residual — quarantine does not unmap a flagged device from live co-holders
- type: residual-risk
- source: QA thread K5b-plan (P3-2), K5b-plan-review P3
- statement: A device quarantined because one holder's reset failed is NOT unmapped from other live processes that still hold a mapping to it (Q144: mappings outlive handles); such a co-holder can still program the flagged device until its own death triggers quarantine of its own runs.
- status: open (accepted residual, not fixed)
- destination (proposed): SECURITY / kernel residuals

### Q-67: K5b residual — non-virtio DMA devices always quarantine on driver death
- type: residual-risk
- source: QA thread K5b-plan-review (P2-5), stated platform residual in IO-ARCHITECTURE
- statement: reset() unconditionally returns false for non-virtio DMA devices, so any process death with such a device in its reach set quarantines all its runs and flags the device permanently (no restart) — even if that device never actually held the process's frames. There is no driver restart path for such hardware; the answer is physical confinement (e.g. an FPGA DMA channel), never a software fix.
- status: open (accepted as permanent platform limitation, not fixed)
- destination (proposed): SECURITY; servers (IO-ARCHITECTURE)

### Q-68: K5b OD6 — quarantine destroys the device object outright (Architect ruling)
- type: owner-decision
- source: QA thread K5b-od6-sweep (Architect ruling)
- statement: "A device that fails the reset of a process's end is destroyed the same way [R10] does, at that end: every handle naming it closes, in every table and every unreceived message, and its page goes back to its owner. Its base stays flagged until reboot, and a live co-holder keeps the mapping it already has (question 144)." map_device/dma_alloc NotPermitted guards can no longer be reached via handle, since no handle to a quarantined device survives; they remain only as fail-closed assertions.
- status: resolved
- destination (proposed): kernel; KERNEL-SPEC (R10, Device)

### Q-69: K5b legacy MapMemory bypass of DMA reset/quarantine (P2-1)
- type: decision
- source: QA thread K5b-plan-review (P2-1)
- statement: Legacy MapMemory of any page-rounded range overlapping a DMA registry slot's MMIO pages is refused with AccessDenied, for any grant, so a legacy grant-holder cannot bypass the reset set or quarantine flag. Non-DMA device MMIO (e.g. UART) stays grant-reachable only until K6 deletes legacy MapMemory entirely.
- status: resolved (superseded by K6's outright deletion of legacy MapMemory)
- destination (proposed): kernel (superseded by K6)

### Q-70: D3 vendoring — SHA256SUMS-based integrity is not provenance (fail-open bug)
- type: residual-risk / decision
- source: QA thread D3-code-review-1 / D3-code-review-4 (P2)
- statement: vendor-check's checksums are regenerated from the tree itself, so it proves internal integrity since vendoring, not provenance against upstream crates.io; a subsequent review found the provenance.sh script itself fail-open (a renamed README header or missing row silently skipped 0 crates and exited 0). Fixed to fail closed: offline structure check requires the table and vendor/ name the same crate set with equal counts, and the checked-count must equal vendored-directory count.
- status: resolved (40af0efc1, 3d5093263)
- destination (proposed): todo / SECURITY (supply chain)

### Q-71: D3 netd — rx thread silent exit on IRQ error leaves device armed but unreported
- type: decision (fixed regression)
- source: QA thread D3-code-review-2 (P2-1)
- statement: A `wait_irq` error in netd's rx thread previously returned silently, leaving the device DRIVER_OK with rx buffers armed while ipd was never told and transmit kept succeeding — violating the "either thread leaving its loop resets" rule. Fixed so every exit from receive_frames resets the device and reports BROKEN.
- status: resolved (7ca3af285)
- destination (proposed): servers/netd

### Q-72: D3 netd panic hook wiring (was claimed before it existed)
- type: decision
- source: QA thread D3-code-review-2 (P2-2)
- statement: netd's message claimed a panic-hook/reset wiring that did not yet exist in libs/rt; until wired, a netd panic left the device armed. Wired in the libs/rt commit with a test that a panic resets the device.
- status: resolved
- destination (proposed): servers/netd; libs/rt

### Q-73: D3 Admission override sizing undercounted worst case (P1, checkpoint 3)
- type: decision
- source: QA thread D3-code-review-3 (P1)
- statement: Admission::with_overrides/fits computed worst-case open slots as sum(overrides) + rest×default, but an override set BELOW the default could sit idle while a default-sized bucket took its slot, breaking the documented open-call headroom guarantee (49 admitted > 48 in the milestone numbers). Fixed to use max(override, default) per resource per slot; NAMESPACES milestone numbers re-derived.
- status: resolved (ddb5c4732)
- destination (proposed): servers/ipd; servers (libs/rt Admission)

### Q-74: D3/rt residual — libs/rt/tests/parked.rs hung once under heavy parallel load
- type: residual-risk / follow-up
- source: QA thread D3-rt-parked-hang, D3-code-review-3
- statement: Under six parallel heavy test binaries, the parked.rs test binary stopped making progress at 0% CPU and had to be killed; diagnosed as a TEST race (an unbounded WAKE-poll loop after the waiter's 200ms parked deadline expired under load — expected runtime behavior, TIMED_OUT correctly delivered) rather than a runtime bug. Fixed by bounding the test loop and raising LONGEST, but this is stated plainly as an observed hang under load, not softened.
- status: resolved (test fix ab45ef0c6), but the observed hang itself stands as evidence of fragility under load
- destination (proposed): todo (test hygiene note)

### Q-75: D3 ipd — martian drop was incomplete (non-TCP IPv4 could reach main interface)
- type: decision
- source: QA thread D3-code-review-5 (P2-1)
- statement: classify() dropped martian sources only for TCP; UDP/ICMP/unknown-protocol IPv4 from spoofed self-set sources reached the main interface, which replied with ICMP protocol-unreachable — causing ipd to ARP for its own address, or reflect 1:1 to a spoofed LAN victim. Fixed: apply the martian test before the protocol test, and drop every non-TCP IPv4 packet.
- status: resolved (8b1004921)
- destination (proposed): servers/ipd; SECURITY

### Q-76: D3 ipd — sockets bypassed admission rules (worst-case sizing, lingering, fair share)
- type: decision
- source: QA thread D3-code-review-5 (P2-2)
- statement: ipd's socket accounting repeated the override worst-case flaw (true milestone worst case was 60 not 52), let lingering disconnected sockets outlive their admission-bucket slot, and had no per-badge fair share — an agent sharing its sponsor's account could take all 8 sockets and lock the sponsor out (answer 90, CONTAINMENT). Fixed by charging sockets through the shared Admission library as State units (milestone worst case became 100).
- status: resolved (3d8743617)
- destination (proposed): servers/ipd

### Q-77: D3 ipd — connect-timeout case never actually tested a timeout (merge blocker)
- type: decision
- source: QA thread D3-code-review-final (B1)
- statement: bench-net-peer's "connect where nobody answers" case actually observed slirp's immediate RST refusal, not a timeout; with the ctl-deadline mechanism (60s parked deadline) planted off, the case still passed on both host and target, meaning ipd's ctl parked deadline was completely untested. Fixed by correcting the case's claim and adding a real listener ctl-read deadline test (host + target) that fails when the deadline is disabled.
- status: resolved (dc3c8aab1)
- destination (proposed): servers/ipd

### Q-78: K5 scheduler — three merge-gating test insensitivities (T1/T2/T4)
- type: decision
- source: QA thread K5-code-review-3, K5-code-review-4, K5-code-review-5, K5-code-review-final
- statement: Boot cases sched-exit-churn (T1) and sched-budget-churn (T2) did not actually exercise the exit-accounting and lift-wiring bug classes they existed for (planted "no accounting on exit" and "lift by max" bugs both passed undetected across three review rounds), and no boot case (T4) proved a scheduling timeout never preempts. All three gated K5's merge and were fixed (restructured exit-churn to end the spin directly by exit/fault/thread_exit; added sched-trace oracle lift checks for budget-churn; added a boot check for timeout non-preemption) — verified by planting each bug again at final review and confirming it now fails.
- status: resolved
- destination (proposed): kernel (scheduler); plan (legacy WP-K5)

### Q-79: K5 scheduler — carved-down budget lead is over-charged, not rescaled on weight restore
- type: residual-risk / follow-up
- source: QA thread K5-code-review-3 (D1), K5-code-review-4 (D1 worsened), K5-carve-lead-rescale
- statement: A budget that runs while most of its weight is carved away accrues its pass "lead" at the small carved-down weight, and that lead is never rescaled when the weight returns — this only ever over-charges the carving budget (not a gaming/security channel), but an honest shell or steward that carves heavily while running can be starved well past its restored fair share. Accepted for K5 merge and documented as stated behavior (R12 clarification); a classic-stride rescale (lead × w_old/w_new) is an explicit unfunded follow-up pending real carve-pattern measurements.
- status: open (accepted residual; rescale follow-up not done — see Q-104 explicit deferral)
- destination (proposed): kernel; todo (K5-carve-lead-rescale follow-up)

### Q-80: K5 scheduler — R10 destruction work must bill at the parent's RESTORED weight (D1 ruling)
- type: decision
- source: QA thread K5-code-review-4 (D1 ruling)
- statement: Moving the top budget's carve-return to its own lift step (matching an earlier plan wording) meant R10 destruction work (~21ms) was billed to the parent budget at its carved-down (not yet restored) weight, causing severe starvation (example: an honest parent destroying a large subtree gaining a huge inflated lead). Ruling: the TOP's carve must be returned at mark time, BEFORE any destruction work is billed, restoring the parent's real weight first. Model, redoubt-stride crate, and kernel all changed together; verified the reverted fix fails sched-destroy-billing at 419ms.
- status: resolved
- destination (proposed): kernel

### Q-81: K3 follow-up — an IRQ raised while its object is masked, before first receive, can be lost
- type: follow-up
- source: QA thread K3-irq-level-latch (from K5-code-review-5, R5 diagnostic)
- statement: A local diagnostic (goldfish RTC alarm) on QEMU virt reproduced, on trial 0 only (the driver's first receive after handle handover), an interrupt raised while the IRQ object was masked NOT being delivered to the next receive; all later masked-raises (trials 1-5) and fresh edges (6/6) delivered correctly. This is a state-dependent or first-time loss, not "never delivered" — no kernel defect was found by reading (candidates: QEMU PLIC pending semantics for a disabled level line, or stale state from other IRQ-handle probing). Explicitly assigned to K3 (IRQ objects, R5) as a follow-up requiring a dedicated boot case (irq-level-latch); mitigation meanwhile is that netd must drain used rings after every receive regardless of interrupt delivery.
- status: open
- destination (proposed): kernel (K3 IRQ objects); todo

### Q-82: K5 latency target — AMENDED sample size and target decomposition (K5-code-review-5)
- type: decision
- source: QA thread K5-code-review-5
- statement: The originally proposed latency target used only K=40 wakes (making p99 == max) and 8 destroys (supporting no percentile at all) and a single unified 50ms bound for "decision-to-dead." Amended and re-measured with K>=200 wakes/N and >=50 destroys per kind on both widths: wake p99<=50ms AND p50<=15ms; deadline notice p99<=30ms; NEW steward-termination targets split into destroy-kernel-time p99<=30ms and decision-to-dead (wake+R10) p99<=80ms (asserted as a sum, not a single-stat check), with call-to-return including CPU wait recorded but not asserted (~220ms one-round bound at N=16). Pinned and met on both widths at final review.
- status: resolved
- destination (proposed): kernel; plan/M-legacy(K5) — latency targets

### Q-83: K5 latency flake — sched-latency intermittently misses steward decision-to-dead target (open, root-caused, not fixed)
- type: residual-risk
- source: QA thread K5-latency-flake, K5b-code-review-0
- statement: A full bench run failed sched-latency's decision-to-dead check; an initial "fix" (63b3632bc) was found by the red team to actually WEAKEN the pinned target (asserting 80ms on decision-wake ALONE instead of the correct decomposition 50ms wake + 30ms R10 = 80ms combined), and was reverted. Root cause found: nondeterminism comes from the QEMU boot RNG seed (`/chosen/rng-seed`), which shifts PID allocation and hence instruction-count phase; under stride rank, at N=16 the steward can land behind up to ~5-6 weight-100 spinner slices purely by chance, producing a genuine, intermittent, non-artifactual miss of the approved 50ms decision-wake p99 target (measured up to 100702us). This is stated plainly as a real, reproducible miss of the pinned target under this workload — not a measurement bug — and remains OPEN for a ruling (re-pin with evidence, assert the true composite sum, or change the steward stand-in to avoid the debt) plus a recommendation to pin the guest RNG seed for reproducibility.
- status: open
- destination (proposed): kernel (scheduler); todo (latency re-pin decision)

### Q-84: bench-process-flake — process [rv64] boot case intermittent, root-caused and fixed
- type: follow-up (resolved)
- source: QA thread bench-process-flake
- statement: proc-test.rs's blame() scenario only awaited the server's exit notice, not each caller's own exit notice; a caller's process/PID slot could still be held (via process_ended not yet run) when the next scenario started, occasionally producing OutOfProcesses (reproduced 1-2/20 runs). Fixed by waiting for each caller's own exit notice and failing loudly (not silently truncating) on a missing notice; confirmed deterministic 20/20 on both widths.
- status: resolved
- destination (proposed): kernel (tests/programs); plan (bench hygiene)

### Q-85: K5b I-DMA blocker — ghost check disarmed live co-holders' frames on another process's death (B1)
- type: decision
- source: QA thread K5b-code-review-1
- statement: The model's ghost-arming check cleared a reset device from EVERY frame's armed set when any process died, including frames held by a still-live co-holder that still had the device mapped — meaning if that co-holder's OWN later death failed to reset the device, the I-DMA invariant would miss the reuse (proven with a planted kernel bug that passed all per-step Checker assertions and the full mutation suite; the default boot topology couldn't even generate this shape). Fixed: a dying process's reset disarms only ITS OWN frames; live holders' frames stay armed until their own death; a second healthy DMA device was added to make the co-holder shape reachable by the generator.
- status: resolved (2bb280d8b)
- destination (proposed): kernel; model

### Q-86: D3/K5 bench hygiene — testbench run once had an orphaned test process after a killed timeout
- type: residual-risk (minor, not reproduced)
- source: QA thread D3-code-review-6
- statement: One combined test run was killed at 600s right after a planted-bug run whose own `timeout 300` may have left an orphaned test process; it did not reproduce in four subsequent clean runs and no orphan remained. Recorded as likely harness artifact, not code, but left as a stated observation rather than dismissed.
- status: open (unexplained, not reproduced)
- destination (proposed): todo (bench hygiene note)

### Q-87: D3 residual — netd CPU cost under a flood is unbounded pacing-only
- type: residual-risk
- source: QA thread D3-code-review-6
- statement: netd drains the used ring until empty before every IRQ wait so a lost/coalesced edge cannot strand frames, but under a flood it loops without waiting at all, consuming CPU; this is only bounded/paced by ipd's own 50ms send cadence, stated as a residual, not eliminated.
- status: open (accepted residual)
- destination (proposed): servers/netd; SECURITY

### Q-88: D3 residual — one ARP broadcast per SYN under a full backlog during a SYN flood
- type: residual-risk
- source: QA thread D3-code-review-5 (P3), D3-code-review-final (carried residual)
- statement: When ipd's SYN backlog is full, a SYN answered with an RST via the "fresh" ephemeral interface, combined with an empty neighbour cache, produces one ARP broadcast per SYN under flood conditions. Stated as a residual, not mitigated.
- status: open (accepted residual)
- destination (proposed): servers/ipd; SECURITY

### Q-89: D3 residual — miri never run on heapless/smoltcp unsafe code (tooling gap)
- type: residual-risk
- source: QA thread D3-code-review-5, D3-code-review-final
- statement: cargo-miri was never installed during D3's review cycle, so the vendored heapless/smoltcp unsafe code (justifying the unsafe-ratchet exclusion) was never verified under Miri; stated repeatedly as an open tooling gap, not closed.
- status: open
- destination (proposed): todo (tooling gap)

### Q-90: D3/ipd residual — NAT hairpin rests on sshd refusing keyd keys, not on the network model
- type: residual-risk
- source: QA thread D3-code-review-final (carried residuals)
- statement: The outbound-gateway self-set exemption creates a hairpin path back to the box only through an outside connection via hostfwd; the actual security boundary preventing exploitation of this rests on sshd's own refusal of keyd-issued keys, not on ipd's network isolation model itself — stated as a carried residual, not eliminated.
- status: open (accepted residual)
- destination (proposed): SECURITY

### Q-91: D3/K5b residual — manifest boot of netd/ipd:lan and netd restart wait on R3 and K5b
- type: residual-risk / follow-up
- source: QA thread D3-code-review-1, D3-code-review-6, D3-code-review-final
- statement: netd's restart-after-crash and any off-bench (manifest-launched) use of netd/ipd are explicitly gated behind WP-K5b (DMA reset/quarantine) landing and R3 (init-owned launch); until then these remain retained, stated gates, not yet exercised in production configuration.
- status: open (gated follow-up, legacy WP-K5b/R3 numbering)
- destination (proposed): plan/M-legacy(R3); servers

### Q-92: K6 plan — deletion of the entire legacy (Xous-derived) syscall interface (OD1-OD10)
- type: owner-decision
- source: QA thread K6-plan (ten OWNER DECISIONS, owner-approved 2026-09-24)
- statement: The owner approved deleting the hosted/legacy architecture and syscall interface wholesale (~7,000 kernel lines, ~7,000 of libs/abi down to ~120 as redoubt-layout, flatipc entirely) rather than repairing it, under a "squeaky clean, no cruft, one definition each" directive: no dead code, no compatibility shims, no legacy/Xous names, features must have a cfg user, and a permanent no-cruft grep/build gate is added to the bench. This closes two residuals K5 had left open: a callback that never returns holding the CPU/deadlines forever (Q-24/Q-40/Q-51), and legacy direct-switch "borrowed quanta" running budgets the scheduler never picked, invisible to the model (Q-25/Q-39).
- status: resolved (implementation completed and red-teamed through k6-r1 through k6-r6, all MERGE)
- destination (proposed): kernel; userland; plan/M-legacy(K6)

### Q-93: K6 Rule F — test fixture rendezvous after SIDs (log-server as sole trusted party; DONE folded in)
- type: decision
- source: QA thread K6-plan-review (P1-1/P1-2 BLOCK, then resolved in v2)
- statement: After deleting well-known-address server lookup (SIDs), a fresh test-fixture design was required: the first program is the case's trusted tester and owns the console/Reset, serving a folded-in DONE op (replacing the deleted attack-checker) via the interim kernel log endpoint; an attacker is never first, only ever second-and-alone. A per-case table (§7, categories A/B/C) enumerates program order and handle assignment for every existing multi-party test case, including K5b's device-first cases.
- status: resolved
- destination (proposed): kernel; todo (legacy WP-K6 test fixture)

### Q-94: K6 badge-forgery gap — minted badges could collide with bundle PIDs, forging verdict lines
- type: decision
- source: QA thread K6-plan-review (R1, re-review)
- statement: logsrv relayed minted badges as "[pid N]"; if a program minted a badge equal to a low PID number (e.g. 2), an adversarial child could forge byte-identical "[pid 2]" verdict lines as the trusted first program. Fixed: minted badges must be >=0x100 (disjoint from PIDs <=64) AND relayed with a distinct "[badge N]" prefix; a forgery boot case added.
- status: resolved
- destination (proposed): kernel/test fixture (WP-K6)

### Q-95: K6 — the Error type fold into redoubt_sys::Error is boundary-only (N1, must-adopt)
- type: decision
- source: QA thread K6-plan-review (N1)
- statement: Folding the kernel's internal legacy Error type fully into redoubt_sys::Error was found unsafe at two boundaries — it would make address_available's BadAddress/MemoryInUse/ShareViolation distinction collapse (silently turning a lend-in-progress into a spurious OutOfMemory), and it would let a bare `?` compile across boundaries where an explicit spec-mapped error was previously required, risking leaking the wrong ABI error code. Ruling: keep a module-private page-layer error enum; redoubt_sys::Error is used only at/above the syscall boundary, with every mapping site kept explicit and reviewed against KERNEL-SPEC's Errors rows; no new sys::Error variant, no user-visible error value/order/meaning change.
- status: resolved
- destination (proposed): kernel; KERNEL-SPEC

### Q-96: K6 — the no-cruft static gate must not trip on K6's own intentional code (N2, must-adopt)
- type: decision
- source: QA thread K6-plan-review (N2)
- statement: The literal grep-based "no legacy identifier survives" gate would itself fail on K6's necessary code (the kernel must contain "Grnt" to refuse it; legacy-gone.rs must name the old call numbers to test them; OD9 legitimately defines USER_AREA_END twice, once per width). Required an explicit allowlist for these self-referential patterns, per-width constant pairs counted as one definition, and case-sensitivity limited to only the "xous" pattern to avoid false hits on "SysCall" appearing in unrelated redoubt_sys::syscall names.
- status: resolved
- destination (proposed): plan (legacy WP-K6); todo (tooling)

### Q-97: K6 — TID numbering fixed to spec (OD10): initial TID was silently wrong
- type: decision
- source: QA thread K6-plan v2/final (OD10), k6-r5-red
- statement: The kernel handed out only 29 usable threads (TIDs 2..=30) despite KERNEL-SPEC/model/redoubt-sys all stating MAX_THREADS=31; fixed by setting INITIAL_TID=1 so TIDs run 1..=31, stated explicitly as a user-visible numbering change (not silent), with literal TIDs in tests/model traces updated and a thread-limit boot case added.
- status: resolved
- destination (proposed): kernel; KERNEL-SPEC; GLOSSARY (TID numbering)

### Q-98: K6 gap — TAKE_GIFTS originally could hand attackers DMA-capable devices
- type: decision
- source: QA thread K6-plan-review re-review (R2)
- statement: The fixture's TAKE_GIFTS op initially handed the attacker every non-console device it held, including DMA-capable virtio slots — more privilege than any budget-attack case needed. Fixed: TAKE_GIFTS gives only budgets plus the single non-DMA MMIO device a specific case actually needs.
- status: resolved
- destination (proposed): kernel/test fixture

### Q-99: K6 ordering gap — legacy code paths remain live and untested between test-removal and decoder-deletion commits
- type: residual-risk (transient, resolved by later commits)
- source: QA thread k6-r2-red (P2-1), k6-r3-red (P2-1)
- statement: The red team explicitly flagged that intermediate K6 commits (test rows removed before the legacy decoder itself was deleted) left live, exercised-by-nothing legacy code paths (physical-address mapping, legacy IRQ claims, update_memory_flags, MapMemory grants, SwitchTo, Shutdown, ORIGINAL_PID/borrowed-quantum bookkeeping) — explicitly stating "K6 must not merge or be cut" at those intermediate points. Confirmed closed only once the final decoder-deletion commit (7d) landed and legacy-gone/thread-limit/no-cruft all passed.
- status: resolved (closed at commit 7d, verified k6-r4-red)
- destination (proposed): plan (legacy WP-K6, commit sequencing)

### Q-100: K6 final ratchet numbers — kernel unsafe count and layout crate
- type: decision
- source: QA thread k6-r6-red (final round)
- statement: Final measured kernel unsafe count is 13+12+19 = 44 (target was <=50, down from a baseline of 54); redoubt-layout (formerly libs/abi) is 0/0 unsafe under #![forbid(unsafe_code)], down from redoubt-abi's 52 on-target sites (44 undocumented, see Q-4). Verified: one syscall decoder, no legacy path/grant/PID-keyed authority, both K5 residuals (callback holding CPU, borrowed-quantum switches) closed, all planted attack-case regressions caught.
- status: resolved
- destination (proposed): kernel; SECURITY (unsafe ratchet accounting)

### Q-101: K5 review — carve-lead-rescale follow-up must NOT be done as part of K5 (explicit deferral)
- type: owner-decision
- source: QA thread K5-code-review-4 (D1 ruling)
- statement: "Rescaling the lead on every weight increase stays a follow-up (QA K5-carve-lead-rescale); don't do it now." — an explicit deferral, distinguishing the accepted-and-documented over-charge behavior (see Q-79) from a future fix that requires real shell/steward carve-pattern measurements first.
- status: open (deferred by design)
- destination (proposed): todo (K5-carve-lead-rescale)

### Q-102: K5-r10-destroy-cost — tightening dependency named but not yet a thread of its own
- type: open-question
- source: QA thread K5-code-review-5, K5-code-review-final
- statement: R10's destroy cost (~21-27ms, close to its 30ms bound) dominates the deadline-notice and steward-termination latency margins and is explicitly tied to the number of objects/frames present; the deadline target is stated as acceptable "for this workload only" and gated to tighten once R10 is indexed (a named but not-yet-tracked follow-up, K5-r10-destroy-cost — see also Q-52).
- status: open
- destination (proposed): todo / kernel

### Q-103: K5 residual — R5 IRQ latency and R10 non-preemptible poll worst case belong in the latency table
- type: decision
- source: QA thread K5b-plan, K5-code-review-final
- statement: K5b's non-preemptible reset poll adds a worst case of |S|×1ms (|S|<=16) on top of R10's ~21ms, explicitly folded into K5's sched-latency notes/docs rather than left implicit.
- status: resolved (documented)
- destination (proposed): kernel; plan (legacy WP-K5 latency notes)

### Q-104: K6 residual — EXCEPTION_TID/IRQ_TID reservation still owed to OD10 bookkeeping
- type: open-question (minor, closed by final round)
- source: QA thread k6-r4-red
- statement: After the legacy decoder deletion, EXCEPTION_TID and IRQ_TID slots were still reserved pending OD10's TID renumbering commit — flagged as an open item for that specific commit, not a standing gap.
- status: resolved (closed with OD10/commit 9)
- destination (proposed): kernel

### Q-105: D3/K5 residual — process[rv64] proc-test panic recorded and diagnosed only after the K5b restart
- type: follow-up
- source: QA thread D3-code-review-final, bench-process-flake
- statement: The D3 final-review full bench observed a rv64-only proc-test panic (proc-test.rs:454) with a byte-identical image on both base and tip, indicating a pre-existing K5-era race unrelated to D3; explicitly handed to K5's owner as a follow-up rather than silently absorbed into D3's merge, and only root-caused/fixed after the restart under K5b (see Q-84).
- status: resolved (root cause: caller exit-notice race; fixed 2e8f3d735/6d3154240)
- destination (proposed): kernel (tests/programs)

### Q-106: K6 residual — a sibling program (not the attacker) may receive TAKE_GIFTS first (P3-2)
- type: residual-risk
- source: QA thread k6-r1-red
- statement: TAKE_GIFTS in the interim log-server fixture goes to whichever program calls first; in an attack case where a benign sibling happens to call first, the attacker then fails loudly on Refused (no silent pass) — stated as acceptable only because the fixture is explicitly INTERIM until R3, not a permanent design.
- status: open (accepted, temporary)
- destination (proposed): plan/M-legacy(R3); todo

### Q-107: K5b residual — dma-reset-reuse case proves ordering only up to dma_alloc's return, not the internal reset-then-pool critical section
- type: residual-risk
- source: QA thread K5b-code-review-final-red (P3-1)
- statement: The reset-before-reuse boot evidence (dma-reset-reuse) reads device STATUS only after dma_alloc has already returned the frame to a new holder, so it proves the reset preceded the new holder's *use*, not that the reset strictly preceded the internal pool operation inside dma_release — that stricter in-call ordering rests only on the kernel's own P1-1b assert and the model's I-DMA invariant, not on independent boot-level proof. Explicitly accepted under the owner's "no-steering" decision, not closed by further testing.
- status: open (accepted residual)
- destination (proposed): SECURITY; kernel

### Q-108: K5b residual — dma-reset-deaf/quarantine attack cases are rv64-only; rv32 coverage is build-only
- type: residual-risk
- source: QA thread K5b-code-review-3 (P3-1), K5b-code-review-final-red (P3-2)
- statement: Both the dma-reset-attack (must-fail) evidence and the final dma-reset-reuse/dma-reset-quarantine cases run only on rv64; rv32 is verified only by successfully building the dma-reset-deaf feature, not by running the actual attack/reset scenarios — stated plainly as a coverage gap on the 32-bit target, not fixed.
- status: open (accepted residual)
- destination (proposed): SECURITY; kernel; todo
