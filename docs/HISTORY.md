# History

What was done, and what we learned. Current state: STATUS.md. Next: PLAN.md.

## Fork and first boot (2026-09-15 to 09-18)
- Hard fork of betrusted-io/xous-core at `c025441`.
- New S-mode loader for SBI and device-tree platforms (`global_asm!` entry, no C toolchain); the
  boot bundle became a signed ustar of ELFs, replacing Redoubt's `create-image` tool and its MiniELF format.
- rv64 kernel: widening 11 `target_arch = "riscv32"` gates left one compile error; every other rv32
  assumption was invisible to the type checker. Sv39 through a direct physmap (MEMORY-LAYOUT.md),
  trap entry as `global_asm!`, upstream `riscv` crate for CSRs, SBI console, PLIC backend
  (rustsbi `plic` crate), hart timer as IRQ 0 (BOOT.md), RNG keyed from `/chosen/rng-seed`.
- Milestone: boots to userspace on QEMU `virt` and passes every IPC message type.
- Test bench (`cargo testbench`): declarative TOML cases, bundles injected, console assertions,
  forbidden patterns, ELF corruption, firmware selection, `--run` and `--debug`.

## Bugs found
- **64-bit ABI:** syscall results were returned by reading the `repr(C)` `Result` enum as registers,
  which works only if every field is one word; now serialized with `to_args()`. The same class
  (memory punned as register arrays, `u32` fields in ABI types) remains to audit in `libs/abi` and
  `redoubt-ipc`.
- **`scause` on rv64:** interrupt causes were matched on bit 31; the flag is the top bit.
- **Upstream: `FreeInterrupt` off by one.** Any process could panic the kernel with
  `FreeInterrupt(32)`. Guarded by `irq-attack`. Worth reporting upstream.
- **Upstream: cross-process use-after-free of lent pages.** A process that died while a page was
  lent had the frame freed and reused under the borrower: `release_all_memory_for_process` passed a
  physical address where a virtual one was required. Fixed by walking the dying process's page
  table and reparenting lent frames. Guarded by `uaf-lent-page`. Worth reporting upstream.
  Remaining: a borrower returning a page to a dead lender may leak the frame (not a safety hole).
- **`MapMemory` on physical RAM (found by review round 1's red team):** an explicit physical
  address inside RAM skipped the device-grant check and was not zeroed, so any process could map a
  free frame and read a dead process's memory. Fixed in `c36c220`; guarded by `mem-attack`.
- **`libs/abi` `Result` marshalling:** `Unimplemented` encoded to another variant's opcode, and
  `UnknownResult` decoded shifted by one slot; both invisible to the compiler. Fixed in `8c21f32`,
  with a round-trip unit test over every variant.
- **Device tree parser:** the `fdt` 0.1.5 crate mis-parsed RustSBI's valid, re-serialized tree and
  panicked. Moving the loader to `fdt-rs` made it boot under RustSBI; RustSBI had been blamed first.

## Hardening pass (2026-09-18)
Typed page tables (the `paging` crate) with W^X by construction and a boot-time self-check;
`KernelCell` for globals; RNG and verified boot fail closed; default-deny device grants; the `unsafe`
ratchet (211 undocumented uses at the fork, 0 now); ARM, x86 and the in-kernel gdb stub deleted.

## rv32 re-homed (2026-09-18)
rv32 and rv64 now share one loader, one SBI/PLIC/timer platform and one physmap design, differing
only in width; both boot on QEMU under RustSBI (QEMU ships no rv32 OpenSBI). Precursor, bao1x,
VexRiscv, the Sv32 window scheme, swap and the prebuilt assembly blobs were deleted.
Width bugs found at first rv32 boot, all "assumed rv64":
- `PHYSMAP_BASE + phys` is right only when the physmap starts at physical 0; use `physmap_virt()`.
- The loader writes 64-bit `XArg` and `MREx` fields on both widths; parsing them through `usize`
  overflowed a `<< 32` on rv32.
- The PLIC sits at its own rv32 root entries, so the loader must pre-create shared intermediate
  tables before user address spaces copy the kernel roots.
- Reading the 64-bit `time` CSR on rv32 needs `rdtimeh`/`rdtime` with a wrap-retry loop.

## Interrupts (2026-09-18)
The interrupt path carried pending IRQs as a `usize` bitmask capped at 32 entries, though the
controller only ever reports one at a time. `cf101f8` dispatches one interrupt and sizes the handler
table to the PLIC's 1024 sources. The hart timer was delivered as IRQ 0 with `PlatformSpecific`
deadline calls (BOOT.md), pending the kernel-owned design.

## beamlet (sibling repository)
In parallel, beamlet reached: OTP's `crypto`, `public_key` and `ssl` running unmodified on Rust
natives; Elixir's compiler and its own test suite; console input and IEx; mailbox overflow killing the
receiver.

## Design v1 (tag `redoubt-design-v1`)
Tenets, then capabilities and principals (agents as first-class principals), containment (labels),
resources (budgets, scheduling), init and the steward, 9P namespaces, packages, I/O architecture.
Decisions made along the way: no dynamic linking; the BEAM is not in the TCB; littlefs over RedoxFS;
9P for everything user-facing; revocation by budget rather than a derivation tree.

## Review round 1 (tag `redoubt-design-v2`)
Red team, simplifier and editor. Thirteen decisions:
1. Revocation by budgets only; stamps inherited from the requesting capability; leases in the kernel.
2. No rights bits on handles.
3. The kernel owns the hart timer; receive takes a timeout.
4. No overcommit; a 1 ms user clock (dropped in round 2).
5. Device authority becomes handles; `redoubt-names` deleted; RAM never nameable; pages zeroed.
6. Labels static per budget; reads above them fail; dynamic taint deferred.
7. Approvals only out of band, like 2FA; made a tenet.
8. Two scheduling classes and runtime-charged stride; donation and CPU quotas deferred.
9. Native user code stays; trust lists and profiles become steward state.
10. launcherd into the steward; auditd deferred; one restart rule; `keyd` stays separate.
11. Admission per budget; crash quarantine (replaced in round 2).
12. Browser GUI, link layer and router, Linux partition to Later; Linux there is TCB.
13. Corrections: IP-prefix sockets, no persisted authority, M-of-N, per-item declassification,
    closed options.

## Review round 2 (tag `redoubt-design-v3`)
The same three roles, attacking the v2 fixes. Fourteen decisions:
1. Calls between user budgets need equal labels, sends may go up; system servers enforce no read up
   and no write down from the caller's label set.
2. Labels per volume, not per file; declassification copies an item.
3. More labels than the parent only via the steward; budget usage and exit messages obey labels.
4. Normal and vault sessions; a terminal is cleared for its authenticated owner's labels only.
5. Mint takes a budget handle (the stamp or a descendant); 64-bit never-reused budget ids.
6. Approval rendering stripped and steward-named; requests bound by id and hash; `approve-hs` with a
   FIDO key.
7. Budgets are charged; admission per principal; crash blame after 3 consecutive crashes means logout.
8. The loader loads only the kernel and `init`; every process maps its own ELF via the loader stub.
9. IPC is `call` and `send` with lend or transfer, zero-copy, no kernel queue; transfers need receiver
   opt-in.
10. Budgets: pages, processes, weight, class, labels, deadline; one flat stride queue.
11. Shared package store deferred (per-principal packages, A/B system updates); projects as
    principals; `blkd` merged, disk encryption deferred.
12. The 1 ms clock dropped: assume the attacker has a perfect clock.
13. North star adds Alice's leased agent and a scripted hostile agent; the shell is IEx.
14. Exit messages and kill-by-budget; FPGA host and bitstream assumptions; steward state on a system
    volume; small deletions and editorial fixes.

## SMP spike (2026-09-19)
With the `smp` feature, `KernelCell` became a spinlock (host stress test), then a second hart started
through SBI HSM ran kernel code and contended on it without losing updates, on both widths (the
`smp-spike` case). The bug that cost the debugging: `virt_to_phys` returns the frame base, so the
trampoline addresses lacked their page offset.

## Review round 3 (tag `redoubt-design-v4`; frozen for milestone 1)
The same three roles, attacking v3 and pinning interfaces for a swarm build. Fourteen decisions:
1. Keys that authenticate a person to the box never live in `keyd`; `sshd` rejects keys `keyd`
   holds; sessions cannot connect to the box's own addresses (closes loopback self-approval).
2. Lent pages stay with the server, charged to it, until it replies, if the lender dies or times
   out; lends are size-capped; `call`, `send` and `receive` all take timeouts.
3. Budgets carry an `account` (replacing "principal id"); endpoints serve waiting senders
   round-robin by account, with a per-account cap.
4. Crash blame is the account of the message the faulting thread was serving; three blamed crashes
   log that account out.
5. Minting keeps the stamp by default; a budget handle only narrows; zero-limit revocation scopes.
6. Parent usage counts children's limits; steward no-write-down, random request ids, pending caps;
   declassification snapshots and shows the whole item; calls and sends between user budgets need
   equal labels.
7. The terminal rule became ordinary no-write-down on labelled SSH channels.
8. Interrupts are received on IRQ handles; no handler context, no `ClaimInterrupt`.
9. `call` lends one writable buffer; `send` transfers; read-only lends and transfer-in-call dropped.
10. The loader stub is a flat binary; launchers copy ELF bytes and never parse them. Corrected claim:
    a hijacked agent can run code it wrote, never with more authority than it holds.
11. Milestone 1 trimmed (bundle-only programs, stateless steward, one approval path, no sharing);
    milestone 2 defined to carry the rest; FIDO approvals deferred further.
12. The milestone 1 attack suite: a scripted hostile agent and a scripted hostile user; a real-agent
    harness right after milestone 1; milestone 3 defined (self-hosted development).
13. Wire format: 9P's encoding for every typed message, strict JSON for human-written files; vault
    names select one label; packages are signed as a whole; interface pins (KERNEL-SPEC.md).
14. One exit slot per process and one fired flag per IRQ; agents share their sponsor's account;
    volume labels never come from the medium; the shared server library is `admit` and `check`;
    A/B details; the kernel rules became KERNEL-SPEC.md.

## Milestones (defined in round 3)
1. Separation and containment: Alice and Bob over SSH, Alice's leased agent contained.
2. Install, share, persist: packages, projects, reboot memory, A/B updates.
3. Self-hosted development: a real agent harness, compilers on the box, the server APIs.

## After the freeze (spec changes, each with its reason)
- **`random` system call added** (2026-09-19): user processes had no source of randomness, but
  beamlet, `keyd` and `sshd` need one. Chosen over a per-process seed in the startup block, which
  would spread randomness across every launcher and risk a parent reusing its seed for a child.
- **rv32 deferred** (2026-09-19): milestones 1 to 3 are claimed and booted on rv64 only, to keep the
  bench fast. rv32 returns as a small goal after milestone 3. Width-specific code stays confined to
  paging geometry, trap entry and saved context, and the ABI's register encoding; 64-bit values are
  `u64`, never `usize`. rv32 keeps compiling (a build check, no boots) so the abstraction cannot rot.
- **IPC semantics pinned** (2026-09-19, owner answers 1-6, QUESTIONS.md): `receive` says whether a
  message is a `call` or a `send`, and `reply` to a send is refused, because a server that guessed
  wrong would strand a caller. A thread may hold several **open calls**, up to `MAX_OPEN_CALLS` = 64
  per process, each charged a page, `Busy` beyond (changed from one per thread: `consoled` and
  `ipd` hold a request open per terminal or socket, and 31 threads would cap them at 31 clients);
  I5's R3 bound is per open call. Message ids are non-zero and never reused. R1 checks against the
  endpoint's owner, so the check no longer depends on which thread takes the message. A transfer
  the receiver's budget cannot hold is `Refused`, like one over `max_transfer`. A dying server
  fails only the calls it had taken; queued senders wait for the restart (INIT.md had said
  otherwise; KERNEL-SPEC.md was right, since endpoints outlive servers).
- **Objects and costs pinned** (2026-09-19, answers 7-13, 15, 16): a cost table in KERNEL-SPEC.md
  (one page per budget, process, thread, endpoint, open call and exit slot; page tables per page;
  handle tables one page per 128 handles, changed from the guessed 256 since a handle is 24-32
  bytes; WP-K1 confirms), so `budget_usage` and `OutOfMemory` compare between model and kernel.
  The exit slot is a page charged to the creator at `process_create`: `killed` notices outlive the
  budget that died, and before this nothing paid for them. System-class receivers see labelled
  exit notices and usage (else `init` and the steward could not see agents crash); creating a
  system-class budget needs a system-class caller, like adding labels. Handle 0 is "none";
  `MAX_START_HANDLES` = 64 and `MAX_RANDOM` = 64 are named constants. `budget_usage` returns the
  weight limit and carved weight (R7 carves weight). A weight-0 budget holds no process (R12
  divides by weight). Records are 8-byte aligned; deadlines absolute, timeouts saturating. A 64-bit
  argument always takes two 32-bit registers on both widths: one layout, and no width `cfg` in
  `redoubt-sys`.
- **Errors and the order of checks in KERNEL-SPEC.md** (2026-09-19, answer 14, changed): WP-C1
  compares exact errors, so the order is normative. Copied from the model's README into the spec
  (the spec owns it; the model conforms), with corrections where the model disagreed with the ABI
  or the answers: W+X, a badge of 0, a malformed page range and a misaligned record are decoding
  errors (`InvalidArgument`); an over-long `random` or `process_start` list is `TooLarge`;
  `LabelDenied` comes before `Busy` in `call` (R1 is now decided at send time); `receive` checks
  the object's kind before its badge; `mint`'s and `process_map`'s checks follow the general
  stages; a too-deep budget is `TooLarge` at the argument stage.
- **Caps keyed by (account, label set); steward ids unpredictable** (2026-09-19, answers 17, 18):
  a vault session and its owner's unlabelled session share an account, so a per-account cap (the
  steward's pending requests, `WAIT_CAP`, R2's turns) was a channel out of the vault; the model's
  10^6 run found it reaching the owner's budget usage. Every id the steward hands out is keyed
  random, not only request ids: sequential session ids told every principal how many sessions the
  others started.
- **Typed-message tables, replies and JSON types** (2026-09-19, answers 19-23, 27): WIRE.md adopts
  WP-W1's table format with a `Reply` column and a per-protocol error table; word 0 of a reply is a
  status (so opcode 0 is reserved); a message is buffer-shaped if its request or its reply needs a
  buffer, because `reply` carries only words and reply data can travel only in the lend (a `blkd`
  read's 12-byte request would otherwise go inline and have no lend to answer in). Inline fields
  pack four bytes per word, the same on both widths; compound values are `bytes`; typed operations
  written into a 9P file are the opcode then the buffer encoding. JSON member names compare byte for
  byte. Each server package writes its own table. JSON: no "either" (changed): the schema fixes
  each field's type, 64-bit quantities strings and small counts numbers, so one value has one
  spelling; INIT.md's example fixed.
- **Tenet 3 amended** (2026-09-19, answer 24): host-only test oracles and fuzz drivers (the
  littlefs C reference, libFuzzer) may be C or C++, in crates outside the workspace build, never
  linked into anything that runs on the machine. Differential testing against the reference
  implementation is what makes a from-scratch littlefs trustworthy. littlefs's milestone 1 limits
  accepted (answer 25, NAMESPACES.md).
- **Attacks asserted by the system** (2026-09-19, answer 26): every attack case, not only WP-E1's,
  asserts its outcome through the kernel, the victim or a clean power-off, never the attacker's
  own output: the console does not say who wrote a line, so a hostile program could print its own
  PASSED line.
- **Kernel IPC and revocation** (2026-09-19, owner answers 30-32, 43-45, 47, 49, 53; QUESTIONS.md):
  R10 now reaches messages already sent, because a queued or taken message through a revoked handle
  was still delivered and its reply's handles still reached the sender: a queued one fails with
  `Dead`, a taken call's caller gets `Dead` at once, and its reply is discarded (lend kept as in R3,
  as for calls in flight to a destroyed endpoint). `WAIT_CAP` counts queued messages only; a taken
  call is bounded by open calls. R4 counts the page tables to map a transfer; a message whose
  handles the receiver cannot pay for stays queued (`OutOfMemory`), for sends as for calls. A
  receiver already waiting when its process reaches `MAX_OPEN_CALLS` gets `Busy`. Page tables are
  freed when they map nothing and the kernel chooses addresses, so WP-C1 compares usage in the
  model's placement profile. **Badge notices** (new): the kernel tells an endpoint when the last
  handle with a badge is gone, so a server can free a dead client's fids and quota, which until
  then only a restart released; one pending slot per badge, charged to the endpoint's owner at the
  `mint` that creates the badge (so the notice never allocates), the exit notices' label rule, new
  I15. I10 now holds once the destroyed budget's exit notices are received or dropped, since the
  creator paid their slots (answer 43).
- **Blame by the most recent open call, and per (account, label set)** (2026-09-19, answers 31, 37,
  48, 55; 37 and 55 changed): after answer 2 a thread can hold many open calls, and blaming every
  one of them would blame everyone waiting on a `consoled` thread when Bob's request crashes it (the
  bystander problem of round 2). A thread's serving account is its most recently taken call still
  open; a `send` never sets it, since it cannot be replied to and an idle thread faulting an hour
  later must not blame its sender. `mint` accepts any open call of the caller's thread. A
  `process_exit` while holding open calls (a Rust panic, the commonest crash from hostile input) is
  reported `faulted` and blamed the same way. Exit notices carry the blamed call's labels, and
  blame, its limit and the logout are keyed by (account, label set), as caps are (answer 17): a
  vault session crashing a shared server must not log out its owner's unlabelled sessions.
- **Leases and approvals** (2026-09-19, answers 33-35; 33 clarified): `MAX_LEASE` = 24 h, a new
  KERNEL-SPEC.md constant the steward applies (a lease of `u64::MAX` meant no deadline); longer
  requests are refused, not clamped. Sub-agents are budgets inside their agent's budget: an agent
  holds only its own budget handle, so it cannot start siblings that outlive it and use up its
  sponsor's processes. The approval screen renders a printable-ASCII whitelist (stripping control
  characters missed bidi and format characters) and shows the requester's kind and steward-assigned
  name. A labelled requester's request shows only steward-generated text, since its free text
  reached the unlabelled screen unchecked: a channel out of the vault.
- **Containment: reads and writes** (2026-09-19, answers 46, 51, 52, 54; 51 changed, 54
  clarified): every write needs equal labels, and `check` is read ⇒ object ⊆ caller, write ⇒
  object = caller. Blind write-up let an unlabelled caller truncate or remove labelled files it
  could not read, and `Tcreate`'s "exists" revealed names; it is not needed, since data enters a
  vault by the vault session reading it down. A qid or `stat` is a read, and directory reads list
  only readable entries, since labelled metadata changing under an unlabelled observer was a
  covert channel. The steward reads labelled items (declassification) through a short-lived
  reader budget carrying exactly the item's labels, never a standing universal reader. A receive
  right is never handed across label sets (I7 states that R1 compares with the endpoint's owner).
  Account 0 is admitted per badge, so one daemon cannot lock the steward out.
- **Startup block and launching** (2026-09-19, answers 39, 40, 50): INIT.md adopts WP-R1's block
  format (`SBlk`, `NmSp`, `Hndl`, `Argv`; `redoubt-rt` implements it), since it is the contract
  between every parent and child. `process_start` gains `arg`, which carries the startup page's
  address to the first thread (no fixed address in the layout). A launcher never passes its own
  connection to a child, but a fresh one: every copy of a handle is the same badge, so Alice's
  hostile agent shared her 9P fid table. Stated as a rule, not a convention.
- **Wire** (2026-09-19, answers 28, 29, 41, 42): typed-message tables name each handle's kind
  (`handle[0] endpoint`), and the generator checks it. Manifest names are 1-64 bytes of
  `[a-z0-9_:+-]` starting with a letter, since the parser accepted empty names and NUL, U+FEFF or
  C1 controls, which became endpoint names and 9P paths. Status 1 is `Malformed` in every protocol
  and in a 9P call's reply, one rule, reserved by the generator; a 9P call's words are all zero in
  the request and in a successful reply.
- **Storage** (2026-09-19, answer 36): IO-ARCHITECTURE.md states `blkd`'s contract (whole-sector
  overwrites, in-order completion, torn writes persist a prefix, `sync` after virtio-blk's flush),
  which littlefs's power-loss safety depends on and which WP-L1's red team found unstated. littlefs
  has no data checksums (NAMESPACES.md, accepted limits).
- **Answers 56-68** (2026-09-19; 57 decided, 58 changed): `receive`'s record carries each handle's
  kind, since no call reported one and the generated helper (answer 28) had nothing to check
  against. A fault blames the most recently taken call still open (57); a thread that fails with
  no open call blames nobody even when other threads hold some (58, changed from a fallback to the
  process's newest open call, which would blame a bystander): such a crash counts only toward the
  restart limit and the reboot, and corruption left by a replied call can crash an idle thread
  later unblamed (stated residual). The steward alone enforces `MAX_LEASE`; the kernel knows only
  deadlines. `Hndl` names follow the manifest's name rule, one rule in one place. The startup block
  names the program image, with its tag defined by the loader stub's package (WP-R2). INIT.md's
  worked example shows the vault session reading its owner's unlabelled volume. KERNEL-SPEC.md
  counts four object kinds. 59-62 and 66 confirm the notes as written (the badge-slot cost is
  confirmed in WP-K2).
- **Design review round 4** (2026-09-19; Fable red team, simplifier, editor over answers 1-55):
  editorial fixes applied; owner questions 69-101.
- **Round 4, the kernel made smaller** (2026-09-19, answers 69-78, 93, 99, 100; the simplifier):
  badge slots, badge notices and the old I15 are gone (an estimated 150-250 lines of the most
  error-prone new kernel code); a server frees a client when the launcher, holding the random
  connection id `new_connection` returned, `disconnect`s it (stated residual: a launcher that dies
  leaks its children's connections until its own is freed, against its own (account, label set)).
  A lend is charged to both sides while its call is open, so no budget is ever over its limit and
  I5 is unconditional; "abandoned call" is defined once (R3). A delivery the receiver cannot pay
  for is `Refused` to the sender (reversing answer 44's "stays queued"), so `receive` never fails
  for want of pages. Class is inherited and `budget_create` takes no class: a class check on one
  door of three guarded nothing. The process object is the exit slot, charged to its creator. A
  budget's own page is its parent's. `random` returns one `u64`. `MAX_LEASE` is a steward
  constant in CAPABILITIES.md. The exit endpoint must be badge 0, or anyone could spray notices at
  a server. A server that means to exit replies to its open calls first. The positional order of
  checks stands (answer 14's list was a classification). Handle kinds are checked by use
  (revising answer 56): the kernel reports none, and the table's kind is documentation.
- **Round 4, holes closed** (2026-09-19, answers 79-92, 94, 95; the red team): open calls could be
  pinned (Bob parking 64 lent calls at `ipd` stopped `netd`'s frames), so a server is told when a
  call is abandoned (the flag in the open call's page; it replies to free it), and at
  `MAX_OPEN_CALLS` it takes no calls but still receives sends and notices (option (a); not letting
  a taken call's timeout lapse, which would let a hostile server pin its caller, the hole round 3
  closed with timeouts). Blame in event-driven servers could be steered to a bystander, so a
  thread's current call is set by `receive` and by a new `serve(msg_id)`, event work blames
  nobody, and 57/58 are replaced. Only `init` and the steward hold system-class budgets; servers
  get narrowing handles only as revocation scopes. Revocation sweeps handles inside queued
  messages; system callers are grouped by budget in R2; message ids are per receiving process and
  PIDs random, since global counters were channels. `init`, the steward and the drivers run in
  `first` budgets and servers working for users in the stride queue (84; the steward stays first
  but bounds each request's work: the `first` flag in `budget_create` is the editor's mechanism
  for it). Shared pools are carved (a byte quota per attach root, unasked handles closed, caps that
  fit the server's budget); each principal's budget is split into fixed sub-budgets per label set;
  an agent gets a fair share per badge and its sponsor can always end its lease; the third blamed
  crash ends every budget of that (account, label set) and blocks new sessions for the window;
  audit records and approval notifications carry labels; `sshd` is stated as the one sink cleared
  for a label, with `approve@`'s shared `sshd` a milestone 1 residual; `keyd` badges name one key
  and one purpose.
- **Round 4, formats** (2026-09-19, answers 75, 83, 97, 98, 101): the startup block is one typed
  message (`startup`) decoded by `redoubt-wire`, replacing a second framing format whose CRCs
  protected nothing; every 9P endpoint serves `ninep_common` (`new_connection`, `disconnect`);
  both tables are shown fenced until WP-R1b generates them, so the drift test stays green. `init`
  reports blame in one typed message whose table WP-S2 writes; every milestone 1 typed message is
  a `call`; the steward `call`s a reader budget, which fills its lend.
- **One stride queue, no priority at all** (2026-09-19, answer 103, replacing the editor's `first`
  flag): the flag is gone, with the rules for setting it, `budget_create`'s argument for it, R12's
  two-tier ordering and I8's clause about it. Every budget shares one queue by weight; `init`, the
  steward and the drivers get large manifest weights (1000 against a session's 100) instead of
  running ahead of everyone. Strict priority bought little that weight does not: a budget woken by
  an interrupt re-enters at the minimum pass, so a driver runs within about one `SLICE`, and
  priority would matter only for a driver that spins while others are runnable, which is a bug for
  the bench to find, not a mode to support. The steward's large weight is what keeps logout and
  ending a lease prompt (answers 84 and 90); answer 84's text, which gave servers manifest weights,
  now stands alone. **Stated cost:** under load a driver or the steward waits up to one `SLICE`
  before it runs, and the bench tests that bound. Class now means trust only — R1's exemption,
  `budget_usage` across labels, and who may add labels — and never scheduling; KERNEL-SPEC.md
  (Budget, R12, `budget_create` and its rows, I8), RESOURCES.md, INIT.md, CONTAINMENT.md,
  README.md and PLAN.md say so.
- **Answers 102 and 104-111: the kernel's remaining edges** (2026-09-19): `MAX_HANDLES` = 4096 is a
  constant, and a call that would pass it gets `TooLarge`, which a caller can tell from its budget
  running out of pages (K1 already caps the table at 32 pages; the spec named no limit). A handle
  table costs one page per table page holding a handle, holes and all, because that is what the
  memory costs and handles are never moved to compact the table; the executable model follows (111,
  which WP-C1's replay would otherwise have caught as a difference). A finished process stops
  counting against its budget's process limit at once but keeps its PID until its exit notice is
  received (106). An abandoned-call notice reaches only the thread holding the call, on the
  endpoint the call came in on, so every serving thread keeps receiving there (104). At
  `MAX_OPEN_CALLS`, `receive` refuses calls only — they stay queued, R2 skips them, sends,
  interrupts and notices still arrive, and no `Busy` is returned for the limit (105). The message
  layouts (108) and I15 (109) stand as written, and a thread with no current call blames nobody,
  with no fallback (110).
- **Answers 107 and 116: handles at delivery** (2026-09-19): handles that would take a receiver
  past `MAX_HANDLES` are a cost it cannot pay like any other, so the message is `Refused` to its
  sender (answer 72). A reply is never refused, since its caller is blocked and has nowhere to put
  the error: handles that do not fit the caller, by its pages or by `MAX_HANDLES`, are dropped (0
  in their slots, as a revoked handle is), the reply is delivered without them, and the `call`
  returns `OutOfMemory`.
- **Answers 112-115: formats and decoding** (2026-09-19): the startup page starts with a `u32` byte
  length, then the `startup` message, because a typed message carries no overall length and the
  decoder refuses trailing bytes, so the block could not otherwise be read out of its page (112).
  `ninep_common` reserves opcodes 1-15 on a 9P endpoint and a server's own protocol there starts at
  16, marked `<!-- wire: NAME ninep -->`; the generator refuses a marked table using a lower opcode
  (113). `not_yours` is code 2 for a `disconnect` naming an id the caller did not receive, so it
  looks like an id that does not exist (114). Decoding never allocates: a record in a page the
  caller reserved but never touched is `InvalidArgument`, not a page the kernel backs and charges
  mid-decode, so `OutOfMemory` cannot appear at the decoding stage and the error rows stand; the
  runtime touches its record buffers first (115).
- **Answers 117-119: quotas, buckets and the checked build** (2026-09-19): `new_connection` carries
  `quota`, the `ninep_common` error table has `3 refused` (root missing, permission denied, a cap
  reached, quota exceeded), and a connection a client mints for itself counts in the share of the
  connection it came through, so minting badges cannot escape a fair share (117; NAMESPACES.md and
  CONTAINMENT.md, from WP-R1b, completed). Each server's manifest sizes its bucket count to the
  (account, label set)s it serves, so the cap does not bind in normal use, and CONTAINMENT.md
  states the residual for a server sized smaller; byte quotas live in `fsd` behind the shared
  library's grant and disconnect hooks, not in the library, while `quota` stays on the wire, since
  only `fsd` meters bytes (118; BUILD-PLAN.md WP-D2 says so). Tenet 6's amendment, already made
  with WP-K0b, is confirmed as the owner worded it: a build of the same sources with debug
  assertions and overflow checks on is not a special build (119).
- **BUILD-PLAN.md follows answers 102-119** (2026-09-19): WP-A3 added (the ABI drops
  `budget_create`'s `first` flag, adds `MAX_HANDLES`'s `TooLarge` and the reply's `OutOfMemory`);
  WP-W3 added (the generator's `ninep` marker and opcode floor, and the runtime touching record
  buffers); WP-K2 removes the flag from the kernel and carries the delivery rules; WP-K5 delivers
  one queue and tests the `SLICE` of latency; WP-M1, WP-R3 (manifest weights), WP-D2 (the byte
  quotas) and WP-S2 (the steward's weight) follow; the merged packages and the order are updated,
  and A3 must land before K2.
- **The boot bundle's signature gets its own domain** (2026-09-19, answer 120; **a change to what
  ships**, not only to the notes): the loader now verifies, and the signing tool builds, the
  preimage `"redoubt.bundle.v1\0" || u64_le(len) || tar` — 18 bytes of NUL-terminated domain, the
  archive's length as a little-endian `u64`, then the archive — instead of the bare archive.
  Reason: a signature over a bare archive can be made to cover something else's bytes. A ustar
  header's name field is 100 bytes of arbitrary bytes, so another protocol's domain and length fit
  inside the first header, and that protocol's preimage is then a well-formed bundle; separating
  `keyd`'s domains closes the hole from one side only. And it is cheap now and awkward later: the
  loader and the signer are small, and the only bundles in existence are the bench's, signed with
  the public development seed, so no production key has to be re-signed. Every Redoubt signing
  domain is now a NUL-terminated name plus a `u64_le` length, and so prefix-free
  (`"redoubt.audit.v1\0"` for `keyd`'s audit records, `"redoubt.pkg.v1\0"` for packages in
  milestone 2). VERIFIED-BOOT.md states the container and the rule; PACKAGES.md the package
  domain; BUILD-PLAN.md WP-V1 changes the loader and the bench's signing path together, with a
  case that a signature over the bare archive is refused.
- **Answers 120-126: keys, arguments and what `/boot` shows** (2026-09-19): `init` refuses a
  manifest that gives `keyd` the key the loader verifies the bundle with, beside the login and
  approval-key check it already owned, asking `keyd` through `holds` so that `init` never derives a
  public key from a seed (120; INIT.md). A typed protocol that mints a narrower capability names
  its `grant` and `release` operations, the typed counterpart of `new_connection` and `disconnect`:
  `grant` mints no wider than the caller, stamped like the request's handle, and returns a random
  id; `release(id)` frees it and everything under it, for the holder of the id alone; a launcher
  releases a child's grants on its exit notice, as it disconnects its connections (121; WIRE.md,
  CAPABILITIES.md, INIT.md). Manifest arguments are opaque strings `init` passes through unchanged,
  each server's note defining its own, with `init` validating only their count, length and encoding
  (122; INIT.md). `bootfsd` serves only the bundle entries the manifest's new `public` list names,
  matched byte for byte, never the manifest itself, and the residual is stated: in milestone 1 the
  seeds live in `init`'s memory and in the bundle image, at the bundle's trust, and milestone 2
  seals them to the machine and generates them at first boot (123; INIT.md, NAMESPACES.md). No
  session and no lease holds `keys` in milestone 1 — the worked example's row is gone — and a
  lease carries `keys` from milestone 2, where a principal's key comes with the one message shape
  it may sign (124, answer 95; CAPABILITIES.md, INIT.md). The steward signs each audit record through
  `keyd`'s `audit` purpose and the file carries the signatures, with verification an operator tool
  in milestone 2 and the stated limit that per-record signatures catch edits, not wholesale
  deletion (125; CONTAINMENT.md). A server draws its first minted badge at random above 2^63 and
  counts up, refusing to mint rather than wrapping, so a restarted server on an endpoint that
  outlived it never reissues a badge a client still holds (126; CONTAINMENT.md).
- **BUILD-PLAN.md follows answers 120-126** (2026-09-19): WP-V1 added (the bundle domain in the
  loader and the bench's signing path, with a bare-archive case; it changes what ships, so it lands
  on its own, and both halves are hotspots until it does); WP-R3 gains the manifest refusals (the
  bundle key through `holds`, a `public` list naming the manifest or a missing entry), arguments
  passed through unchanged, and the `public` list handed to `bootfsd`; WP-R4 serves only public
  entries, with a case that a walk to the manifest's name is refused like a name the bundle never
  held; WP-S1 gains `grant`, `release`, `holds`, its manifest arguments, the random first badge, and
  the note that no lease carries `keys` in milestone 1; WP-S2 signs each audit record through a
  `keyd` grant. The Order still holds: V1 depends on nothing and is off the critical path, which
  stays K2 to K5, then R3, S2 and S3.

## Milestone 1 build (from 2026-09-19)
One line per merged work package (SWARM.md). Open owner questions: QUESTIONS.md.
- **WP-T1 bench extensions** (`987bacbed`): SSH sessions driven through host OpenSSH (checked-in
  test keys, pinned host keys, real exit statuses), virtio disk and net per case, bundle data
  entries, `poweroff`, `allow_panic`, `must_fail` self-checks. The red team found seven ways to
  make the bench pass wrongly (among them a loopback sshd that gave any local user a shell, and
  the console no longer being read after the last expect); all fixed, each with a `must_fail` case.
- **KERNEL-SPEC.md corrections from WP-A1** (2026-09-19, editorial, following answers 3, 11 and
  14): message id 0 is a decoding error in `reply` and `mint`; `budget_usage` returns its six
  counters in a record (they do not fit the result registers); stage 1 checks each register in
  full when it reaches it, a list count included, and unused registers last.
- **WP-A1 `redoubt-sys`** (`44f1780a1`): the system call ABI for KERNEL-SPEC.md, one register
  layout on both widths (a 64-bit value is two 32-bit halves), no width `cfg`, one `unsafe` (the
  `ecall` stub), no dependencies; host round-trip and malformed-input tests and a fuzz target
  (about 1.7 billion runs, no findings). Its three gaps in the spec were corrected (above).
- **WP-W1 wire codecs** (`d52896bee`): `redoubt-wire` (9P2000 from one message table, typed
  messages with replies and error tables, strict JSON with schema-fixed types) and a generator from
  the notes' tables to Rust and Elixir codecs; vectors run on the BEAM and on beamlet; fuzzed.
  Reviews found uncompilable generated names, silently dropped table rows and half-written
  directory entries; all fixed. No server tables exist yet (each server package writes its own).
- **WP-T1b attack verdicts** (`6cd067a39`): every attack case takes its verdict from the system
  (answer 26): `log-server` prints client bytes only through one sink that prefixes each line with
  the sender's PID from the kernel, victims and an `attack-checker` give the verdicts, and
  `bench-attack-forgery` keeps forgery closed. The review found the first version still printed
  moved pages raw, so any client could forge a victim's verdict; fixed and re-checked. The audit
  also found the lend-untouched-page kernel panic (WP-K0).
- **WP-L1 littlefs** (`25ab39296`): littlefs 2.1 in pure Rust (`libs/littlefs`: `no_std`, no
  dependencies, no `unsafe`) for `fsd`, without wear levelling or relocation; model-based,
  crash-at-every-write (torn writes, torn erases, crashes during repair), hostile-image and fuzz
  tests, and differential tests against the C reference v2.11.3 in a host-only crate. The review
  found a real bug: removing a file while a write handle was open let the handle later write into
  blocks another file had taken (fixed, with the model now holding handles across removes and
  renames, which found a second bug, a zero-byte write committing a stale copy). The C reference's
  wear-levelling path fails its own asserts and loses operations on several seeds (recorded in the
  diff suite; keep `block_cycles` off in C tooling that touches these volumes). The block-device
  contract the crate relies on is QUESTIONS.md 36.
- **CONTAINMENT.md: `admit` counted per (account, label set)** (2026-09-19, editorial, following
  answer 17): the shared server library's admission limits are caps, and answer 17 counts caps per
  (account, label set); the library's description now says so (question 38).
- **WP-K0 kernel memory panics** (`f7b9fdd16`): three kernel panics reachable from any
  unprivileged process, and the out-of-memory `expect`s beside them, fixed; the lend and move paths
  back, check ownership of, and prepare the destination for a whole range before any page moves;
  lent (S-bit) entries can no longer be unmapped, remapped or reserved over. Found by WP-T1b's
  audit and K0's own red team. Cases `lend-untouched-page`, `move-borrowed-page`,
  `return-lent-unmapped`, `syscall-attack`, `touch-beyond-ram`; the bench gained `memory_mib`.
  Bugs found (the owner decides what goes upstream, privately):
  - *Lending an untouched page re-entered the memory manager*: `lend_memory`/`send_memory` held the
    memory manager and called lazy backing, which borrowed it again (a panic on one hart, a
    deadlock with `smp`); backing also `expect`ed on out-of-memory. Upstream has the same shape as
    two live `&mut` to one static (undefined behaviour) rather than a panic.
  - *A server could panic the kernel by moving a page it was only lent*: `send_memory` remapped
    before learning the frame was the lender's, then panicked. Identical upstream.
  - *A lender could unmap its own lent page, and the server's return then panicked*
    (`return_page_inner`'s assert). Identical upstream.
- **Coherence pass over answers 1-55** (2026-09-19, editorial): every note re-read so the two
  tranches read as one design. Consequences written out where a note still stated the old one: a
  vault session may read unlabelled volumes (how data enters it, answer 51); `process_exit`'s row
  and R4b name the panic and revocation cases; the loader stub passes `arg` on (PACKAGES.md);
  admission, blame and the approval cap per (account, label set) wherever they were still "per
  account"; I10's caveat in RESOURCES.md. BUILD-PLAN.md gains the merged WP-K0 and WP-T1b and the
  follow-ups the answers require (WP-A2 ABI, WP-W2 generator, WP-M1 model, WP-R1b runtime), with
  Needs and Order updated; README.md's glossary and "Built today" follow.
- **WP-R1 `redoubt-rt`** (`8298608af`): the native runtime (startup block, typed handles, IPC
  helpers, heap, panic handler) and the shared server library (`admit` per (account, label set)
  and per badge for account 0, `check` with write needing equal labels, a 9P skeleton keyed by
  (badge, account, label set) that keeps `..` inside the root and treats qids and listings as
  reads, typed dispatch with status 1 = `Malformed`); host tests against a fake kernel, an rv32
  and rv64 build case, two fuzz targets. The review found fid tables shared by every copy of a
  handle, server work done before admission, allocation failure killing the server, and a 32-bit
  overflow in the startup parser; all fixed. Runs on the kernel after WP-K2.
- **`ninep-common` renamed `ninep_common`** (2026-09-19, editorial, from WP-W2): protocol names are
  snake_case identifiers (WIRE.md, Tables); the table's marker must be one.
- **Call numbers from `NUMBER_BASE` = 0x100** (2026-09-19, WP-K1, encoding only): until WP-K6
  deletes the legacy Redoubt calls, the kernel serves both interfaces through one `ecall`, and
  Redoubt's numbers 1..=24 collided with the legacy ones. Numbers belong to `redoubt-sys`
  (KERNEL-SPEC.md, ABI), so the spec's tables do not change; WP-K6 sets the base back to 0.
- **WP-K1 budgets and handle tables** (`e1d2c6216`): budgets as one RAM frame each, carving (R6, R7),
  accounts (R8), deadlines recorded, destruction that kills the budget's processes and sweeps every
  handle naming or stamped with a destroyed budget before any frame is freed (R10, I2, I10); handle
  tables of 128 32-byte handles a page (the cost table's figure confirmed), at most 4096 a process;
  each handle checks its object's and its stamp's ids. Answers 73 and 76 built; pending questions
  102, 111 and 115 built as their recommendations at single sites. Interim until K4/K6: the kernel
  sizes root, system and users from RAM, every loader process lives in `system`, only the first
  holds the three handles, and a Redoubt call inside a legacy interrupt callback is `NotPermitted`.
  Kernel 11.8k -> 12.9k lines; `unsafe` unchanged. The red team found nothing exploitable.
- **WP-W2 wire generator** (`3715363a9`): the generator follows answers 28, 41, 42, 56 and 98:
  `handle[N] KIND` names the object a handle must be (documentation in the generated code, checked
  by use); code 1 is `Malformed` in every protocol, reserved by the generator, so a protocol's own
  codes start at 2 and an error table may be empty (`example` renumbered). Vectors run on the BEAM
  and on beamlet; fuzzed. The review found nothing exploitable.
- **A revoked handle is 0 in its slot** (2026-09-19, editorial, from the WP-A2 review): R10 already
  says such a handle arrives as 0; KERNEL-SPEC.md's ABI section now says the slot keeps its position,
  so `receive`'s record carries a 0 there and decoders accept it.
- **WP-A2 ABI follows answers 28-101** (`c98034520`): `redoubt-sys` carries `process_start`'s `arg`;
  `serve` (number 18, later calls renumbered); `random` returning one `u64`; `budget_create`'s
  `first` flag in place of a class; `receive`'s one 24-slot record for call, send, interrupt, exit
  (with `blamed_labels`) and abandoned, with no badge notice and no handle kinds; received handles
  are optional per slot (a revoked handle is 0); `MAX_HANDLES`; each call's error row, asserted by
  the kernel in debug builds. Pending questions 102, 103, 105, 107, 115 and 116 are marked at
  their sites. The kernel's `budget_create` checks `first`, `random` answers in registers, and
  `kframe::write_byte` went (kernel `unsafe` 24 -> 23). The review found received messages could
  not carry a revoked handle (fixed). Found on the way: a debug-assertion kernel does not boot
  (`mem.rs:207`, the loader's memory-region table read against `from_raw_parts`'s rules): WP-K0b.
- **WP-R1b `redoubt-rt` follows answers 64 and 69-101** (`86117e7af`): the startup block is the `startup`
  message behind a `u32` length (question 112); `ninep_common` served by the 9P skeleton, with
  `new_connection` (admission before the walk, a random id, badges never reused) and `disconnect`
  (a connection and everything minted under it, freed without allocating; a stranger's id refused
  like one that does not exist); the badge-notice hook gone; a fair share per badge, with a
  self-minted connection counted in the share it came from; caps sized to fit, with a minimum of
  two per bucket; parked calls with a server-side deadline, resumed under `serve` and answered at
  once when abandoned; unasked handles closed before the reply. Byte quotas left the shared
  library for the file server (question 118): `new_connection` gained `quota` and the error
  `refused` (question 117). The review found no exploitable case; it found `disconnect` could
  strand descendants under memory pressure (fixed).
- **WP-K0b argument-block audit** (`e30d43304`): the kernel cast the loader's `MREx` table to a struct
  needing 8-byte alignment from data that is always 4 mod 8, so `from_raw_parts` broke its
  precondition on every boot: undefined behaviour in the trusted base, silent in release. The table
  is now the tag's words, decoded six at a time with checked narrowing. The audit found three more:
  `process_name` viewed a name tag as four times its length and read past it; the tag iterator never
  bounded a tag's data against the block; the initial-process count was tied to the page, not to
  `MAX_PROCESS_COUNT` (and the loader could wrap a `u8` PID). Booting the SMP spike checked found
  two latent bugs: the secondary hart's trap vector was a 2-byte-aligned function (a reserved mode)
  and its stack was in read-only memory. Two of the argument-block bugs are upstream Redoubt code. The
  bench gained a `checked` profile and `debug_assertions = true` per case, used by six cases on both
  widths with a longer listening window, plus a self-check that the checked build really reaches the
  kernel. Kernel core `unsafe` 23 -> 21. Tenet 6 amended (question 119) to say a checked build of
  the same sources is not a special build.
- **WP-S1 `keyd`** (`14bcc6e9d`): the key server as a typed protocol over `redoubt-rt`, its tables in
  INIT.md, keys from the manifest (one per argument, the key in argument i having root badge i), two
  purposes each with one message shape, and **every signature over a 32-byte digest keyd computed
  itself** (the RFC 4253 exchange hash with `K_S` from its own key, or the audit digest), so no
  container that signs longer messages can be what a keyd signature covers. No export operation
  exists; `grant` mints no wider than the caller and dies with what the caller holds; `release(0)`
  frees everything a holder granted. Constant time rests on the loader's `ed25519-compact`; SHA-256
  is written in the crate rather than bringing 27k lines and 123 `unsafe` sites into the process
  that holds every key. No `unsafe`. The review found a stale grant became a capability for another
  key after a restart (first minted badge now drawn at random, answer 126, in keyd and the 9P
  skeleton), a system caller could open a bucket per chained grant (only a root badge may grant),
  grants of a dead holder were stuck, and the timing test could not see a 25% leak (rewritten
  fixed-versus-random with a 5% control that must flag). The grant machinery now lives once in
  `redoubt-rt` (`Minted<T>`), shared with the 9P skeleton.
- **WP-V1 bundle signing domain** (`05955bf86`, answer 120): the loader verifies the boot bundle over
  `"redoubt.bundle.v1\0" || u64_le(len) || tar`, with `len` measured from the initrd it was handed,
  hashing the preamble and then the archive in place; the bench signs the same preimage. Both get it
  from one crate, `libs/signing` (no dependencies, no `unsafe`), so the halves cannot drift, and a
  host test pins the preamble to the bytes VERIFIED-BOOT.md states. A new `host-tests` bench kind
  runs those tests in the suite, because the one drift a shared crate cannot catch is a typo in the
  domain itself: changing `v1` to `v2` now fails the bench. The review showed two of the three first
  cases could never fail (Ed25519 refuses every other message anyway), so only the bare-archive case
  boots; the domain and length refusals are host assertions.
- **WP-K2 endpoints and messages** (`95788dcd0`): the seven IPC calls beside the legacy interface.
  There is no message queue: a message is queued exactly while its sender is blocked, so the queue
  is the set of blocked senders, kept in a page per thread (the page the cost table already charges
  for a thread) and found by walking them, which R2's round robin must do anyway. An endpoint holds
  only R2's cursor, so no sender can make the kernel allocate for a receiver. Taken calls move to an
  open-call page charged to the receiver (R4a); lends are charged to both sides (R3); abandoned
  calls are flagged in place and offered once (I15); a delivery the receiver cannot pay for is
  `Refused` to the sender and spends nothing (R4); a reply's handles that do not fit arrive as 0
  with `OutOfMemory`; R10 reaches queued and taken messages by stamp, and endpoints by owner. Answer
  103 landed with it: the `first` flag is gone from the kernel and the ABI (WP-A3 folded in). Four
  bugs its own cases found, among them `mint` from a message checking the message's badge, so every
  mint from a client's call was refused. The review found a lend to a receiver at its budget's exact
  minimum panicked the kernel, because the R4 check omitted the page tables it then charged: the
  kernel now counts them before deciding, so a refused delivery allocates nothing and there is
  nothing to unwind. Fifteen rule breaks were tested, fourteen caught; the fifteenth (the kernel's
  second badge-0 check) cannot be caught from userspace by construction, which is the point of
  having two, and was verified by bypassing the first. Interim until K4 and K5: one boot endpoint,
  a per-PID page reservation (question 127), and polled deadlines. Kernel 12.9k -> 14.9k lines;
  `unsafe` unchanged.
- **WP-K3 device objects and interrupts** (`12c52c2d7`): the machine's devices became kernel objects. The
  loader reads the device tree and describes each MMIO region (with a DMA flag for bus masters),
  each wired interrupt and the reset right in a `Devs` tag, and reports the interrupt controllers'
  ranges in a `Ctrl` tag; the kernel makes one page-sized object per entry and refuses to boot on an
  entry overlapping RAM or a controller. `map_device` maps the whole range and returns its length
  (question 146), `dma_alloc` returns contiguous zeroed pages and their physical address, the only
  call that does and only through a DMA handle, and `system_reset` goes through SBI. R5 has no
  acknowledge call and no handler: the kernel completes the controller's claim while the source is
  still enabled, masks it, and wakes whoever is in `receive`; the next `receive` unmasks.
  `map_anon`, `unmap` and `set_flags` landed with them. The review reproduced the one real hole: the
  controller exclusion was a loader heuristic with no kernel backstop, so a device tree whose PLIC
  node lacked one property handed the PLIC and CLINT to userspace; the check now lives in the
  kernel, verified by mutation in both directions. Two halves of R5 remain unprovable on QEMU (its
  16550 pulses the controller's pending bit rather than holding a level, and its PLIC accepts a
  completion for a disabled source): `uart-irq` says so rather than claiming a catch. Reviewers ran
  on Opus; Fable's credits had run out. Kernel 14.9k -> 15.6k lines, loader 1.1k -> 1.3k, kernel
  core `unsafe` 21 -> 20 (the legacy contiguous-DMA allocator `dma_alloc` replaces was deleted).
- **`fsd` typed operations** (2026-09-22, design amendment before WP-D2): added `rename`, `copy`,
  `get_attr` and `set_attr` to `fsd`'s typed protocol for within-volume operations 9P2000 does not
  express. Per-file attributes are stored in littlefs custom attributes and accessed through the
  same label and quota checks as 9P operations. `NAMESPACES.md` carries the wire table; `USERLAND.md`
  and `BUILD-PLAN.md` WP-D2 are updated. Cross-volume rename stays copy-and-remove.

- **The use case, the high/low pair, and the game** (2026-09-22, owner direction: the primary use case
  is running dangerous agents that try to break out and collude): the design gains a stated use case
  and threat model, because it had none at tenet level even though the whole point of the exercise is
  multi-agent containment. TENETS.md adds "The use case" (containment, not prevention; capability
  closure; label non-interference; human control) and "The high/low pair": a labelled agent with
  secrets and no network (**A**) colluding with an unlabelled, networked agent (**B**) that has agreed
  a code in advance. Labels remove every *intentional* path between them, so the note now says what it
  always meant — **the isolation unit is the label set, not the capability set**. **Collusion is not a
  goal the OS can meet**: on one machine, power, heat, EM and the clock couple any two domains, so even
  a perfect RTL cannot reach zero. The design's exact claim is **zero intentional paths** (the OS, by
  construction), plus a reduced and measured covert residual (RTL), with **placement — not co-locating
  the secret with the domain that wants it — the only true zero**. CONTAINMENT.md replaces "they never
  reach zero; the goal is low bandwidth" with that split, states that **read-down from a shared
  unlabelled volume is an intentional low-to-high path** (so a confined domain takes input by an audited
  steward push, not by reading), and adds the **channel table**: every signalling resource closed by
  software, reduced by RTL, or physical and unclosable (power/heat/EM/clock), with the rule that two
  domains co-reside only if the residual is below what the secret is worth. PLATFORM-FPGA.md makes the
  enumerated RTL closures required (separate cores, partitioned L2, isolated memory bandwidth,
  per-domain DMA/disk/NIC, no shared GPU) while stating they are a reduction, not a zero. INIT.md gains
  a `confined` manifest flag: `init` refuses a manifest that places differing label sets in one server
  instance, volume, endpoint, network instance or core, exactly as it refuses a server's own budget.
  GAME.md is new: the fight-club contract — scenarios (single-agent escape; the high/low pair;
  authority expansion; collusion through the human), setup, win conditions, and the **verdict
  taxonomy** (design hole vs implementation bug vs stated residual vs policy vs human). A **software**
  channel (an intentional path) is the only red win that counts against the OS; a **covert** channel is
  measured and reported, never graded, because no OS can close it. The design is unchanged in
  mechanism; what changed is that its containment claim is now stated exactly — zero intentional paths,
  a measured covert residual, and placement as the only zero — where a reader and an agent can find it.

