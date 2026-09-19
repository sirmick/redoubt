# History

What was done, and what we learned. Current state: STATUS.md. Next: PLAN.md.

## Fork and first boot (2026-09-15 to 09-18)
- Hard fork of betrusted-io/xous-core at `c025441`.
- New S-mode loader for SBI and device-tree platforms (`global_asm!` entry, no C toolchain); the
  boot bundle became a signed ustar of ELFs, replacing Xous's `create-image` tool and its MiniELF format.
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
  (memory punned as register arrays, `u32` fields in ABI types) remains to audit in `xous-rs` and
  `xous-ipc`.
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
- **`xous-rs` `Result` marshalling:** `Unimplemented` encoded to another variant's opcode, and
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
5. Device authority becomes handles; `xous-names` deleted; RAM never nameable; pages zeroed.
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

## Milestone 1 build (from 2026-09-19)
One line per merged work package (SWARM.md). Open owner questions: QUESTIONS.md.
- **WP-T1 bench extensions** (`987bacbed`): SSH sessions driven through host OpenSSH (checked-in
  test keys, pinned host keys, real exit statuses), virtio disk and net per case, bundle data
  entries, `poweroff`, `allow_panic`, `must_fail` self-checks. The red team found seven ways to
  make the bench pass wrongly (among them a loopback sshd that gave any local user a shell, and
  the console no longer being read after the last expect); all fixed, each with a `must_fail` case.
