# Open decisions

Open IDs: **128–149, 163–166, 171**. Next unused ID: **172**. Recommendations below are not approvals.
The owning specification carries each accepted rule and its rationale; [ANSWERS.md](ANSWERS.md)
indexes approval provenance. [Earlier questions](archive/2026-09-22/QUESTIONS.md) are historical.
Do not renumber or reopen settled questions. Record a new decision once, then update its owner.

Questions 164–166 qualify the confinement, authority-closure and wakeup-latency claims.
Questions 143/146 remain open even though BOOT documents the current device implementation.

### 128. An endpoint cannot be destroyed.

It dies only with its owner budget (R10), so a process
can spend its budget's pages on endpoints it can never reclaim. That is bounded by the budget,
and a revocation scope gives a way to reclaim them, so it may be deliberate.
*Rec:* state it in KERNEL-SPEC.md rather than adding a call: an endpoint lives until its
owner budget dies, and a process that wants to reclaim one creates it in a scope it can
destroy. No `endpoint_destroy`.

### 129. Rename across directories.

Plain 9P2000 renames only within one directory (`wstat` with a
new name), so `File.rename("/a/x", "/b/x")` cannot be expressed, and build tools lean on it.
*Rec:* `fsd` serves a typed `rename(dir_fid, name, dir_fid, name)` alongside 9P (every 9P
endpoint already serves typed operations), atomic within a volume; across volumes stays copy
and remove, reported like POSIX `EXDEV`.
*Alt:* copy and remove everywhere, non-atomic, and a crash mid-rename leaves both or neither.

### 130. What `File.stat` reports, and what `chmod` does.

There are no mode, owner or atime fields
anywhere below (access is by capability; littlefs keeps mtime and qid version).
*Rec:* synthesise a fixed mode for `stat`, report mtime and size honestly, and let `chmod` and
`chown` succeed as no-ops: Mix and escript tooling call `chmod` on scripts, and failing there
breaks tools for no security gain, since the bits mean nothing.
*Alt:* `:enotsup` for both, which is honest and breaks tools.

### 131. A fid held across a remove.

NAMESPACES.md says a remove succeeds while another connection
holds a fid, because an "in use" refusal would be a channel between connections. It does not
say what the holder then sees.
*Rec:* the fid keeps serving the unlinked file until clunked (POSIX behaviour, and littlefs's
copy-on-write makes it cheap); a fresh walk gets `:enoent`.
*Alt:* subsequent operations on the fid fail, which is simpler in `fsd` but surprising.

### 132. Names for standard input, output and error.

Plan 9 sends all three to `/dev/cons` and
redirects by duplicating file descriptors; Redoubt has no descriptor table and no inheritance,
so a pipeline needs distinct names bound per child, or `b`'s output goes into the pipe it is
reading.
*Rec:* `/dev/stdin`, `/dev/stdout`, `/dev/stderr` as namespace entries, with `/dev/cons` bound
to all three for an interactive child.
*Alt:* `/fd/0`, `/fd/1`, `/fd/2` (closer to Plan 9's `/fd`), same mechanism.

### 133. Who serves a pipe.

A pipe is a 9P file somebody serves (there is no pipe object).
*Rec:* the session's shell VM serves it, which needs `serve`/`reply` natives and the
server-side 9P codec in beamlet; `System.cmd` capturing output needs the same machinery, so it
is paid for once.
*Alt:* a tiny `piped` server per session: bulk bytes stay out of the shell VM, at the cost of
another server package and its startup block.

### 134. Copying the program image on every launch.

The launcher copies the ELF into fresh pages
charged to the child (PACKAGES.md); there is no shared text and no demand paging. A pipeline of
small tools does not care; beamlet VMs are megabytes and we start one per session and per
agent.
*Rec:* accept it for milestone 1, and record it as the reason the steward may cache a VM image;
ask WP-R2 whether the stub can map image pages read-only from a shared cache instead.

### 135. Launching from Elixir.

Milestone 1's shell must start native programs, which needs
`process_create`, `process_map` and `process_start` as beamlet natives, and the startup block
written from Elixir.
*Rec:* natives for the three calls plus a `startup` block writer in Rust (the encoder exists in
`redoubt-wire`); the namespace and handles come from the Elixir call, so the policy stays in
Elixir and the encoding stays in Rust.

### 136. Where the Elixir side lives.

The natives are in beamlet; the modules over them
(`Redoubt.Namespace`, `Redoubt.Process`, `Redoubt.Budget`, ...) could be embedded in the VM
(`redoubt/beamlet/vm/lib`, always present) or a Mix package loaded from the boot bundle.
*Rec:* a Mix package in `redoubt/elixir/`, so it versions with the system and not with the VM;
only what the VM needs at boot stays embedded.

### 137. Two error vocabularies.

`File` expects POSIX atoms (`:enoent`, `:eacces`); Redoubt has
`refused`, `not_yours`, label refusals and budget errors.
*Rec:* Redoubt errors keep their own atoms everywhere, and the `File`/`prim_file` shim maps
them to POSIX atoms at that boundary only, so OTP code sees what it expects and new code sees
the truth.

### 138. A transfer within one budget.

R4 counts a message's transferred pages among what the
receiving budget must be able to pay. But a transfer between two processes of one budget moves
nothing between budgets: usage is unchanged, so the kernel charges nothing and delivers even
with no free pages, while the spec as written would refuse it. WP-C1's replay would catch the
difference.
*Rec:* say so in R4: a transfer costs the receiving budget only what it does not already pay
for, so one within a budget is free; a lend is charged to both sides even within a budget (R3).

### 139. Notices after an endpoint is destroyed.

Destroying an endpoint abandons the calls taken
through it, but the notice can never be offered there, and the server's `receive` gets `Dead`.
R3 says the holding thread gets a notice, and I15 says exactly once.
*Rec:* KERNEL-SPEC.md says `Dead` from `receive` is the server's cue to reply to every open
call it took there; no notice follows, because the endpoint it would arrive on is gone.

### 140. Lending pages the caller reserved but never touched.

The ABI refuses an untouched
*record*, but a `call`'s lend of untouched pages is backed and charged to the caller first, as
`map_anon` would. A caller that cannot pay gets `InvalidArgument`, since `call`'s row has no
`OutOfMemory`.
*Rec:* state it in R3 or the `call` row, so the model and the kernel agree for WP-C1.

### 141. A system caller can open a bucket per chained connection in the 9P skeleton.

Admission
keys account 0 by badge, and a share folds into its parent's only when the requester's client
matches the caller's. So a system client of `fsd` or `blkd` minting connections for itself
opens a fresh bucket each time and can spend every bucket the server has, after which nobody
gets a new connection. keyd closed its version by letting only a root badge grant, but in 9P
minting a connection for a child *is* the attenuation the design wants.
*Rec:* fold when the requester's (account, label set) matches the caller's, ignoring the
badge, while admission keys by badge as now. That keeps answer 117 (the steward minting for a
lease's agent is a share of its own) and closes the chain. It costs system-to-system
delegation its own share, which is the conservative direction.

### 142. A device object's cost and owner.

The cost table has no Device row, and nothing says which
budget owns one. WP-K3 charges one page to the owning budget and revokes it with that budget
(R10), like an endpoint.
*Rec:* the cost table gains `| device | 1 | its owner |`, the Device object says its owner is
the budget that held the handle when the loader handed it out (`init`'s, in practice), and, as
for endpoints (question 128), there is no `device_destroy`.

### 143. What the loader decides about devices, and what it may not.

WP-K3's loader sets the DMA
flag from a node whose `compatible` names virtio, keeps interrupt controllers out of the
device list entirely, and refuses the boot on an entry that names RAM or wraps. BOOT.md now
documents that implementation; accepting its target policy remains open: an interrupt controller as a device
object would let its holder mask anyone's interrupts.
*Rec:* BOOT.md states all four rules, plus the `Devs` tag's layout, as part of what the loader
does.

### 144. Two holders of one MMIO handle.

A device handle is copyable, so two processes can both
`map_device` the same range. WP-K3 treats the handle as the authority and doesn't track
mappings.
*Rec:* say so in KERNEL-SPEC.md. A device is shared by whoever was given a handle, exactly
like an endpoint; a driver that must be alone is the only holder because `init` gave it out
once.

### 145. "A page table is freed when it maps nothing" (R11) is not implemented, and no package owns it.

`unmap` returns the pages but not their tables, so a process can strand its own table
pages. Charged to itself, so it is bounded, but the rule as written is false.
*Rec:* WP-K3's `unmap` frees an empty table, and the cost table's page-table row says when
the pages come back. If that is more than a small change, give it to WP-K5 and say so in the
rule.

### 146. Device mapping result and named handles.

The target syscall row still returns only an address; the implemented ABI returns `addr, len`
(BOOT.md). Approval is needed to reconcile that row and define how drivers identify device
handles. Positional discovery currently probes `dma_alloc`; the proposed manifest supplies names.
*Rec:* `map_device -> addr, len`. Which device a handle names comes from the boot manifest,
which gives each driver its handles by name (WP-R3), so the kernel says nothing about it.

Two additions to earlier questions, from the same review:
- **143:** the kernel refuses a `Devs` entry that names RAM, but leaves the interrupt
  controller to a loader heuristic, although the kernel holds the controller's range in the
  `Plic` tag. A `Devs` entry naming the PLIC would be mapped into userspace with nothing in
  the way, and its holder would own every interrupt source. *Rec:* the kernel checks that too.
- **144:** revoking a device handle does not unmap the MMIO a holder already mapped, and a
  device handle is copyable, so a process in another budget keeps register access after the
  owner budget is destroyed. *Rec:* either unmap on destroy, or say plainly in KERNEL-SPEC.md
  that a device mapping outlives its handle.

### 147. A driver's restart leaves its device pointed at freed frames.

blkd's DMA pages go back to
the free pool when it dies, and nothing stops a device already programmed with their physical
addresses from writing into them. The restarted blkd resets the device at bring-up, but only
after those frames may already belong to someone else. This is the concrete form of the
residual K3 recorded when it said a DMA handle is kernel-level trust.
*Rec:* the kernel resets a device whose DMA pages are freed. When a budget holding a DMA
device object is destroyed, or a process holding one exits, the kernel writes 0 to that
device's status register (a virtio reset) before the frames return to the pool. That is a few
lines in `destroy_device`, it needs no device knowledge beyond virtio's reset, and it closes
the window without hardware confinement. If you would rather not have the kernel touch a
device register, the alternative is to keep a dead driver's DMA frames out of the pool until
its successor resets the device, which costs memory instead.

### 148. How the bundle reaches `bootfsd`.

INIT.md says only that `init` passes the public list to
`bootfsd` as its arguments. WP-R4 chose that **`bootfsd` never sees the bundle**: `init` reads
it and pushes the public bytes over a two-message typed protocol (`bootfs`, opcodes 16 and 17:
`add` appends to a name already in its argument list at exactly the offset reached so far,
`seal` ends setup and refuses both for ever). Before `seal` the directory is empty; after it
nothing can be added, and neither message is accepted on a minted connection, so only `init`'s
founding handle can fill `/boot`.
*Rec:* accept. Answer 123 then holds by construction rather than by a filter that could be
wrong, and the bundle — which carries keyd's seeds until milestone 2 seals them — never enters
a server that answers user requests. The table is in NAMESPACES.md, which owns `bootfsd`.

### 149. How a device with both an MMIO region and an interrupt is named.

The manifest's `devices`
list gives one entry per device object, but a UART or a disk is two objects, and a driver has
to find each in its startup block. Three packages have now chosen their own convention: blkd
wants `disk` and `disk-irq`, consoled takes `uart` and `uart:irq`.
*Rec:* one rule in INIT.md: an MMIO region and its interrupt are separate manifest entries and
separate named handles, `NAME` and `NAME-irq`, both under the manifest's name rule (which
allows `-` but not `:`). WP-R3 enforces it, and blkd and consoled follow.

### 163. A parked *typed* call is not possible: only the 9P `read` path parks.

Answer 160 makes
`consol`'s opcode 17 `resize` a **`call` that parks** — the server holds the client's call and
answers it when the window changes. But the park mechanism WP-R1c landed reaches only one path:
`NineServer::serve_parking` hands a request back **only** when `answer_in_place` returns
`Answer::Waiting`, and `Answer::Waiting` is produced **only** by `FileServer::read` returning
`Read::Wait`. A **typed** opcode (`words[0]` not 0 and not in `NINEP_COMMON_OPCODES`) goes to the
server's own dispatch — `serve_parking`'s `own(self, request)`, which is
`Result<(), Error>` and must reply — and the typed `Answer<R>` has no "wait" variant, only
`reply`, `handles` and `close_after_reply` (`libs/rt/src/server/typed.rs`). So there is no way
for a typed request to be held, and `resize` as specified cannot be built until there is.
*Rec:* **extend the typed dispatch to park the same way the read path does, in its own `libs/rt`
package (WP-R1d), owned by the round that first needs it (WP-B2a).** The minimal shape, keeping
one rule for both paths: `TypedServer::handle` gains a way to answer "hold this" — an
`Answer::Wait`-style variant, or the dispatch returns `Option<Request>` as `serve_parking` does —
and `serve_parking` routes a held **typed** request through the same hand-back (close the handles
it brought, empty its list, return the request) that the read path already uses. Nothing else
changes: the parked call is still charged to the server's `Admission`, still reported abandoned,
still resumed and re-read. One mechanism, two entry points. Until it lands, WP-B2a implements
opcode 16 `size` and **not** 17 `resize`; `consol`'s table carries both, with `resize` marked as
depending on this answer.
*Alt:* make `resize` a 9P **file** instead of a typed opcode — a `/dev/cons-size` file whose
`read` parks and answers `cols,rows`. It needs no `libs/rt` change (the read path already parks)
and fits the "everything user-facing is 9P" rule (NAMESPACES.md, Decisions). Out: it adds a
second file to the console contract for one operation, and a size is not a stream — the `size`
query is already a typed opcode, so `resize` as a file would split one concern across two
mechanisms.
*Alt:* build a `send`-based push after all (a per-channel endpoint the client receives on), which
answer 160 rejected as heavier than milestone 1 needs.
**Open:** the owner's or the orchestrator's to schedule; the mechanism is `libs/rt`'s, the same
owner as WP-R1c.

### 164. Confined placement versus the approved steward mediation paths (ASTRA D1).

Answer 152
and INIT.md, The boot manifest, forbid differing label sets sharing any server or endpoint,
including the unlabelled steward's domain. Answers 101 and 153 nevertheless require labelled
reader/writer helpers and owner-approved declassification/push; CAPABILITIES.md also requires
labelled requests to reach the powerbox. The missing piece is a permitted topology and its
post-boot preservation, not whether push was approved. Guessing an exemption changes the
confined guarantee in TENETS.md and the boot refusal WP-R3 must implement.
*Rec:* retain ordinary per-label placement and explicitly name the trusted control-plane
mediation exception in TENETS.md, INIT.md and CONTAINMENT.md. Permit only the specified
request/owner-approval path, per-item reader/writer operations, and labelled lifecycle
supervision needed to end leases; give each edge its caller labels, allowed data, authority
and lifetime in one worked configuration. Do not exempt shared data servers, devices or cores.
`init` validates the declared graph at boot; the steward enforces it for dynamic budgets and
grants, and system servers enforce their own handoffs. Retain exact-label helpers, no standing
data path and answer 153's owner-triggered one-item push. Residual: the named mediators are
trusted across the labels they serve; the confinement claim must say so explicitly.
*Alt:* retain answer 152 literally, with a separate control-plane instance per label set and
an external owner-mediated transfer between them. That removes the shared mediator exception
but requires a replacement for the currently specified single-steward approval/helper path.
**Open:** owner decision; do not change the placement validator or add a general `system`
exemption while this is open. Follow-ups: WP-R3 and WP-S2, then the confined worked scenario.

### 165. What authority set is closed under permitted same-label delegation (ASTRA D2).

Answer
150 already says that equal-label budgets are one trust domain and that a handle passed between
them is not a crossing. CAPABILITIES.md permits copying handles. A recipient can therefore
acquire authority absent from its own initial handle set without a new approval, while
TENETS.md, Purpose and threat model, and GAME.md, Authority expansion, read as forbidding that increase.
Neither forbidding transfers nor reopening the isolation-unit decision is a clarification.
*Rec:* define the closure claim over a trust domain's initial granted authority plus its
human-approved additions, closed under the permitted delegation and service paths. State
separately that a process exercises only its currently held grants and may receive legitimate
attenuated delegations; the union is a bound, not permission to mint a handle it cannot reach.
Keep R9, stamps, lease revocation and answer 150 unchanged. Align TENETS.md, CAPABILITIES.md
and GAME.md so lawful delegation is not scored as escape. Residual: different handle sets
within one label set do not supply a per-agent non-collusion guarantee.
*Alt:* define per-agent reachable authority as the transitive closure of an explicitly
recorded delegation/proxy graph. This can state a tighter bound than the whole label domain,
but the setup must record its actual edges and the verdict must include newly authorized
delegations; an initial handle list alone cannot define that bound. No new nontransferable
handle mechanism is implied by either choice.
**Open:** owner decision on the claim's scope; answer 150 remains binding.

### 166. The one-slice wakeup promise does not follow from the chosen queue (ASTRA D3).

Answer
103 explicitly promises up to one `SLICE` for drivers and the steward, and both RESOURCES.md
and KERNEL-SPEC.md R12 repeat it. R12 actually wakes at `max(own pass, current minimum)`;
retaining a larger pass or several budgets tied at the minimum defeats an unconditional
next-turn bound. This is a proposed revision of answer 103's latency claim, not a correction
an editor may make silently.
*Rec:* retain the single stride queue, actual-runtime charging and the `max` wake rule, and
replace the universal one-slice claim with a measured responsiveness target under a named
workload. WP-K5 specifies deterministic tie handling and records weights, runnable budgets,
prior passes and measured wake/lease-termination latency in its real-boot acceptance. No
universal deadline is inferred from large weight. Update R12, RESOURCES.md and affected
acceptance text together; preserve share/fairness and human-control requirements.
*Alt:* retain a hard one-slice requirement and design a scheduling/admission rule with a
proof under explicit load assumptions. That changes answer 103's mechanism and must be
reviewed for starvation and sleeping-to-gain-priority before WP-K5 implements it.
**Open:** owner decision; neither strict priority nor a weaker guarantee is accepted here.

### 171. A receive output record becomes invalid while its thread waits.

KERNEL-SPEC.md, ABI Records and Errors, requires initial output-record validation. Its IPC
completion rule and late-output `InvalidArgument` explicitly describe `call`/`reply`; the
`receive` error row lists only `Timeout` and `Dead` after decoding. Neither answers 167–168
nor their archived proposals settle whether a failed receive-output commit consumes its
message, notice or interrupt and associated resources.

The current kernel revalidates before message delivery (`kernel/src/message.rs`, `deliver`),
leaving a message queued on failure, and before taking an exit notice. However, `answer_record`
documents a later write failure after consumption as the receiver's loss. Current code is
evidence of behavior, not approval for the model's independent oracle.

*Rec:* every successful `receive` output commits transactionally. Revalidate the output record
and protect validation, copying and delivery commit together against relevant mapping changes
and teardown. If it cannot commit, return `InvalidArgument` to the receiver with no valid
record and no delivery effects: leave a queued sender and its message unchanged; do not
consume an exit notice or release its object/PID, mark an abandoned-call notice delivered, or
consume a pending interrupt. Install no message handles or buffer mappings and retain no
provisional delivery charges. This does not undo receive setup already performed, such as
clearing the current call or unmasking an IRQ, and does not require a valid output record to
return an error such as `Timeout`. Record bytes on failure are unspecified and must not be
decoded. No new return-register encoding is needed.

*Alt:* defer this extension and explicitly exclude late-invalid receive outputs from the
model's conformance claims and acceptance counts pending a decision; continue initial record
validation and the settled call/reply outcome tests. A different mechanism, such as pinning
the initial output frames throughout the wait, would need its own mapping and teardown rules
and is not implicitly approved here.
**Open:** owner decision. If approved, apply the receive error/output contract to KERNEL-SPEC.md
and the model, then record native receive completion/rollback tests and implementation review
as remaining work. This host-model package does not change the kernel or establish native
receive conformance.
