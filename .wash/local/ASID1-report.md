# ASID1 report (implementer)

Branch `wp-asid1` in `/home/mcloonan/redoubt/.worktrees/asid1`, on main 0d207732f.

## Commits

1. `6fc8dc989 tests: asid-cost records what a switch and a page-table change cost`: the cost
   case alone (program `asid-cost`, cases `asid-cost` under icount and `asid-cost-host` in host
   time), so it can be run on main's kernel before the change.
2. `bcee118b0 kernel, paging: each process's PID is its ASID, and a switch flushes nothing`:
   everything else, pages included. Carries two `Size budget:` lines (kernel 8157 to 8356,
   libs/paging 158 to 257 after the reviews' folds; kernel 8358).

(Paging and kernel are one commit: the paging crate's docs and flush rules describe the
kernel's use of them, which a split would leave untrue at the first commit.)

## Design question (ruled: A, architect-14)

Q1: rule 6's flush before `add_header_page` is subsumed by rule 4
(the header's map always links a table into a fresh root, so it flushes the whole ASID), which
would make `asid-no-reuse-flush` vacuous. Built as option A: `add_header_page` records and does
not flush; one `(x0, pid)` flush at the end of `MemoryMapping::allocate`, after the header and
before the process can first run. The page lines say "before it first runs", as ruled.

## The flush table as built (kernel/src/arch/riscv/mem.rs)

| Site | Becomes |
| --- | --- |
| `activate` | `write_satp`, no flush |
| `map_page_inner` | `(virt, asid)`; whole ASID if a table was linked; above the user half (the PLIC) the leaf gains `G` and flushes `(virt, x0)` (F2) |
| `map_kernel_page` | leaf gains `G`; `(virt, x0)`; G walk after |
| `unmap_page_inner` | `(virt, asid)` (skipped under `asid-no-leaf-flush`) |
| `return_page_inner` | `(src, src asid)`, `(dest, dest asid)` |
| `lend_out`, `lend_back`, `drop_lent`, `unmap_from` | `(virt, space's asid)` |
| `map_into`, `map_into_with` | `(virt, space's asid)`; whole ASID if a table was linked |
| `free_empty_tables` | whole ASID after unlinking, before the frame is freed |
| `ensure_page_exists_inner` | `(virt, asid)`; in the shared kernel half the leaf gains `G`, flushes `(virt, x0)` and the `G` walk runs (F3; comment: no caller reaches it today) |
| `set_user_page_flags` | `(virt, asid)` |
| `prepare_map` (added, F1) | whole ASID if it linked a table: the map after it links none |
| `walk_making` failure after linking | flushes the whole ASID itself |
| `MemoryMapping::allocate` | records `(pid, all)`; header mapped; `(x0, pid)` at the end (skipped under `asid-no-reuse-flush`) |

The ASID always comes from the space's `satp` (`Space::of`). `asm.rs`: `flush_mmu` (whole),
`flush_asid` (`sfence.vma zero, a0`), `flush_page` (`a0, a1`), `flush_page_global` (`a0, zero`).
One Rust `flush(Flush)` with one `unsafe`; `write_satp` holds activate's former `unsafe`.

Findings (sites the brief's table misses): F1 `prepare_map`; F2 the PLIC through
`map_page_inner`; F3 `ensure_page_exists_inner`'s kernel-half case; `add_header_page` (Q1).

## Rules 6 and 7

- Rule 6: end of `MemoryMapping::allocate` (mem.rs), every allocation.
- Rule 7: `crate::arch::mem::leave(space)`, first line of `release_owned_frames`
  (kernel/src/mem.rs). If the dying root is the hart's, `satp` moves to `KERNEL_SATP` (set at
  boot); then `(x0, asid)`. `current_pid` is left alone (the caller's switch to kmain follows).

## Boot probe

`arch::process::check_asid_field()` (one call in `main.rs::init`, right after
`platform::early_init`): `mem::read_back_asid_ones` writes ones, reads back, restores;
`paging::SATP.asid_width(readback, ASID_BITS)` decides; `Err` panics "R17: the hart's satp ASID
field holds N bits; every PID needs M"; else prints `asid: N bits`, then
`mem::enter_kernel_asid` switches to ASID 1, one `sfence.vma`, and (checked) the G walk.
Width per QEMU: expected 9 (rv32) and 16 (rv64) from QEMU's source; to be confirmed by a boot.

## Audit (checked build)

`paging::tlb::{Stale, Flush, Unflushed}` (host-tested); kernel `mem::audit` holds an
`Unflushed<16>` log. Every leaf/table write records; every flush removes what it covers.
`audit::returning()` beside `sched::leave` in both ways out (syscall.rs `resume`, irq.rs
`return_registers`): a non-empty log panics "ASID audit: a page-table write is unflushed at a
return from the kernel: ASID n, page 0x...". A full log panics naming the oldest record.
`check_globals` at boot and after each kernel-half map: G on every 4 KiB and root-level leaf of
the shared kernel half, none in the user half or on the per-process entry (intermediate-level
leaves inside a kernel subtree are not visited: there are none).

## QEMU's TLB (QEMU 11.0.2, the image's trixie-backports build)

- `target/riscv/insn_trans/trans_privileged.c.inc`, `trans_sfence_vma`: always
  `gen_helper_tlb_flush`, whatever rs1/rs2; `op_helper.c` `helper_tlb_flush` -> `tlb_flush(cs)`:
  every `sfence.vma` empties the whole TLB.
- `csr.c` `write_satp` -> `legalize_xatp`: if MODE, ASID or PPN changes (and the mode is valid),
  `tlb_flush`, and the value is kept with the whole ASID field. So every switch empties the TLB
  and the probe reads 9 / 16 bits.
- So under QEMU neither negative can produce a stale read; only the audit can fail them.

## Host tests (paging, new; in `host-tests`)

satp_round_trips_on_both_layouts, each_record_names_the_narrowest_flush_that_covers_it, a_full_asid_leaves_the_root_and_mode_alone,
an_asid_past_the_field_is_refused, the_asid_width_decision (all ones, exact, one short, none, a
gap), each_flush_covers_what_the_spec_says (4 flush kinds x 4 record kinds),
a_flush_empties_what_it_covers_and_keeps_the_rest_in_order, a_full_log_names_its_oldest_record,
records_print_what_they_name.

## Commands (all through in-dev, from the worktree)

- `cargo test -p paging` 0 (8 passed)
- `cargo build -p redoubt-kernel --release --features qemu-virt --target riscv{64,32}imac-unknown-none-elf` 0, 0
- same with `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true ...OVERFLOW_CHECKS=true`, features
  `qemu-virt` and `qemu-virt,asid-no-reuse-flush,asid-no-leaf-flush`, both widths: 0 (no warnings)
- `cargo build -p loader --release` both widths 0; `cargo build -p test-programs --release
  --bin asid-cost --bin asid-reuse-stale` both widths 0
- `cargo +nightly fmt --all --check` 0 (after `fmt --all`)
- `cargo testbench --arch rv64 host-tests` (every *host-tests case, 16) 0, all PASS, before
  the rebuild of the commits; reruns below.
- Unsafe: net 0 (flush: one block, as before; satp write: one block, moved from activate).
  `unsafe-budget` PASS.

Gates on b744bc3f2 (`cargo testbench --arch rv64 <case>`): size-budget 0, docs 0, formatting 0,
no-cruft 0, unsafe-budget 0; every *host-tests case (16, model 644 s) 0 on 13dbd82e0, whose
tip differs only in F3's lines, two page sentences and the size ceiling. Kernel release and
checked builds, both widths, 0 after F3. no-cruft first failed on two constants named `PAGE`
(renamed MAPPED / HERE, THERE); size-budget first failed (raised with `Size budget:` lines).

asid-cost on QEMU measures the kernel's own work (flush instructions, walks, the audit-free
release path), not TLB reuse: QEMU empties its TLB on every sfence.vma and satp change.

## Not yet run (needs QEMU, asked)

- `asid-cost` and `asid-cost-host` on commit 29ef4d387 alone (main's kernel), then on the tip.
- `asid-reuse-stale` both widths; then the two recorded negatives (kernel_features added
  locally, not committed).
- The probe's printed width on each width.
- The whole bench is the orchestrator's.

## Page lines as written

memory-layout.md `satp` status: "built · partly tested: QEMU's TLB is not tagged by ASID, so a
missing flush shows only through the checked build's audit; one hart is argued from the code ·
tested: bench:asid-reuse-stale, bench:pid-reuse-authority, bench:uaf-lent-page,
host:paging::satp_round_trips_on_both_layouts, host:paging::each_flush_covers_what_the_spec_says".
Paragraph 1: the brief's text verbatim. Paragraph 2: the brief's text, with "before anything
is mapped in it" written "before it first runs" (ruling 1), then the ruling-3 sentence verbatim
("On QEMU every `sfence.vma` and every `satp` change empties the whole TLB, ... ([the FPGA
platform](../beyond/fpga-platform.md))."), then "The loader enters ..." unchanged.

`G` row: "on the kernel's shared leaves: the physmap, the kernel image, the stacks, the PLIC and
the DMA register window; never in the user half or on the per-process entry" (PLIC added, F2).

Residuals: "A flush acts on this hart only" as the brief; "QEMU cannot show a missing flush. Its
TLB is not tagged by ASID, and it empties it on every `satp` write that changes the space and on
every `sfence.vma` whatever its operands, so the bench finds a missing flush only through the
checked build's audit."

memory.md: the brief's text verbatim. processes.md: "...flushed whole when the PID is given out
again, before it first runs, so no cached translation of the earlier process
survives ([`satp`](memory-layout.md#satp))." fpga-platform.md: both bullets verbatim.
m2-usable-shell.md: item 4 verbatim. boot.md: the `ASID_BITS` row verbatim after
`MAX_PROCESS_COUNT`'s; Hardware bounds' status and R17's status (now 10) list
host:paging::the_asid_width_decision; R17's prose gains "The kernel refuses a hart whose `satp`
ASID field is narrower than `ASID_BITS`, naming both widths: it writes ones to the field, reads it
back and counts the bits that stuck, before anything else writes `satp`". docs/SECURITY.md's R17
row lists the test and `kernel/src/arch/riscv/process.rs`.

## Risks

- `current_pid` names the dying process between rule 7's move and the switch to kmain, while
  `satp` is the kernel's (nothing in that window reads `current_pid`'s space).
- With `smp`, the log is one lock-protected log, not per hart; SMP1's.
- Under QEMU a missing flush is visible only through the audit, never as a stale read.

## Review folds

- Editor: rewrap; "every sfence.vma and every satp change" everywhere (pages, both tomls, the
  commit message); bench:asid-reuse-stale off the `satp` status's tested list until it runs.
- Simplifier: `Stale::flush()` (tlb.rs) gives each record's narrowest flush, so `Space`
  branches on `shared` once; `Space::flush_asid()` for allocate and `leave`; one satp API, the
  `SatpLayout` methods (`SATP.make/root/asid/is_active`), the free functions dropped (loader
  uses `paging::SATP.make(root, 0)`; status line names host:paging::satp_round_trips_on_both_layouts).
- Gates on a09fd277d: size-budget, docs, formatting, no-cruft, unsafe-budget 0; `cargo test -p
  paging` 0 (9); kernel release/checked/negatives and loader, both widths, 0. host-tests rerun
  after Red's fold.
- Red: no kernel finding. Test fix folded: asid-reuse-stale maps two pages at V and unmaps only
  the first, so the leaf table stays and only the leaf flush covers the unmap's write (else
  free_empty_tables' whole-ASID flush masked asid-no-leaf-flush).

## Gates on the tip c3e468a1d (in-dev, `cargo testbench --arch rv64 <case>`)

size-budget 0, docs 0, formatting 0, no-cruft 0, unsafe-budget 0; `host-tests` (all 16
*host-tests cases) 0, model-host-tests 896 s. `cargo test -p paging` 0: the status lines' three
(satp_round_trips_on_both_layouts, each_flush_covers_what_the_spec_says,
the_asid_width_decision) each ok, 9 of 9.

## QEMU (host given by the orchestrator, alone)

Branch rebased onto main ac84b87b3: 6fc8dc989 (asid-cost) and 0326204f4 (tip). Every command
`in-dev cargo testbench <case>` (both widths).

Test-program fixes found on the first runs (folded into the owning commits): both programs
mapped at 0x2000_0000, which is BUNDLE_AT in the first program, so `map_fixed` was refused (now
0x5800_0000); asid-reuse-stale's 64-bit marker was truncated in a 32-bit message word on rv32
(now 32-bit markers).

Probe: `asid: 9 bits` (rv32) and `asid: 16 bits` (rv64), as QEMU's source predicts.

asid-reuse-stale on the tip: PASS rv64, PASS rv32 (exit 0); each run drew A's PID again (rv64
494, rv32 246) and B read 0.

Recorded negatives (kernel_features added to the case locally, not committed; both widths):
- asid-no-reuse-flush: FAIL rv64, FAIL rv32 (exit 1), PANIC at mem.rs:189, "ASID audit: a
  page-table write is unflushed at a return from the kernel: ASID 92, every page" (rv64, with
  allow_panic to see the message): A's allocation, at process_create's return.
- asid-no-leaf-flush: FAIL rv64, FAIL rv32 (exit 1), same panic, "... ASID 2, page 0x58000000":
  the one-process unmap of V, at its return.
Both verdicts are the kernel's audit; neither shows as a stale read (QEMU empties its TLB).

asid-cost: all four PASS before (88909e857, main's kernel, exit 0) and after (c5bd6d15c, exit 0).
Microseconds for 10,000 each; icount: 125 guest instructions per us (includes idle time QEMU
skips with sleep=off).

| | before rv64 | after rv64 | before rv32 | after rv32 |
| --- | --- | --- | --- | --- |
| icount: round trips, one process | 24497061 | 31149992 (+27%) | 25591742 | 27548110 (+7.6%) |
| icount: round trips, two processes | 25642746 | 32306810 (+26%) | 26885856 | 28829254 (+7.2%) |
| icount: a switch (ns) | 57284 | 57840 | 64705 | 64057 |
| icount: map/touch/unmap | 2595352 | 2664643 (+2.7%) | 2906117 | 2932941 (+0.9%) |
| host: round trips, one process | 4137245 | 4060344 (-1.9%) | 4217689 | 4124409 (-2.2%) |
| host: round trips, two processes | 4500685 | 4663965 (+3.6%) | 4576025 | 4668877 (+2.0%) |
| host: a switch (ns) | 18172 | 30181 | 17916 | 27223 |
| host: map/touch/unmap | 445640 | 465689 (+4.5%) | 464562 | 458675 (-1.3%) |

Finding (cost): the icount round trips rose (rv64 +27%, rv32 +7%), within one process as much as
between two, while the switch itself is flat and the host-time round trip within a process fell.
A round trip costs about 2.5 ms of virtual time (about 300,000 instructions) before and after,
far more than the IPC path's work, so most of it is time the guest idles and `sleep=off` skips
to the next timer; a change in where the work falls against the timer moves that idle time.
Nothing on the empty-call path changed but `activate`, which now writes `satp` and no longer
calls the flush, and the release build has no audit. Not proven: confirming it needs a traced
run (sched-trace) of asid-cost before and after, which needs the host again.

Gates on the rebased tip 0326204f4: size-budget, docs, formatting, no-cruft, unsafe-budget 0;
`cargo test -p paging` 9/9; kernel release and checked builds and the loader, both widths, 0.
`bench:asid-reuse-stale` is back in the `satp` status's tested list.

## Cost trace (Architect's ruling (b)): instrumented asid-cost, before 6fc8dc989, after 0326204f4

Local instrument (`.wash/local/asid1-kcount-*.patch`, never committed): `time` ticks (100 ns;
under icount shift=3 one tick is 12.5 guest instructions) split into user, kernel and idle
(`wfi`), `sfence.vma` count, kernel entries from user; snapshot at each `time_now`. Release
build, as the cost case. Per round trip, 10,000-call windows:

| | before rv64 | after rv64 | before rv32 | after rv32 |
| --- | --- | --- | --- | --- |
| icount, within: kernel ticks | 24,187 | 30,817 (+27.4%) | 25,097 | 27,053 (+7.8%) |
| icount, within: user ticks | 335 | 335 | 512 | 512 |
| icount, within: idle | 0 | 0 | 0 | 0 |
| icount, within: sfence.vma | 14 | 0 | 14 | 0 |
| icount, within: entries | 3 | 3 | 3 | 3 |
| icount, between: kernel ticks | 25,335 | 31,956 (+26%) | 26,360 | 28,322 (+7.4%) |
| icount, between: sfence.vma | 18 | 0 | 18 | 0 |
| map/touch/unmap: sfence.vma | 2.10 | 2.00 | 2.11 | 2.00 |

Reading: no idle anywhere, so the rise is not timer alignment; it is kernel instructions (about
+83,000 per round trip on rv64, +24,000 on rv32), at the same entries per round trip. Not
flushes: sfence.vma per round trip fell from 14 to 0. Not lends: the empty call lends nothing.
The hot path's code barely changed (rv64 symbol sizes: `walk` 320 -> 218 bytes, `dispatch`
-8, `virt_to_phys` -2, `activate` -6), so the extra is a data-dependent amount of work per
entry, not code on the path. Also: on main an entry already costs about 100,000 instructions
(8,000 ticks), an empty round trip about 300,000.

## Cost bisect (two instrumented runs, before 6fc8dc989, after 0326204f4)

Phase stamps inside every entry (`/tmp/asid1-k2/apply.py`; local only). Ticks per round trip,
rv64 icount, within one process (between processes the same deltas):

| phase | before | after | diff |
| --- | --- | --- | --- |
| from_user | 24.5 | 24.5 | 0 |
| expiry at entry | 27.1 | 27.1 | 0 |
| billing | 1944.6 | 1944.8 | 0 |
| system call (handle) | 21908.5 | 28555.0 | +6646.5 |
| of which record_frames | 7270.1 | 13922.9 | +6652.8 |
| ... user_frame (walk) | 3085.0 | 6411.4 | +3326.4 |
| ... check_owned_range (walk) | 3843.8 | 7170.2 | +3326.4 |
| to exit | 635.5 | 635.5 | 0 |
| switch | 181.1 | 177.1 | -4 |

rv32: system call +1951.6, all of it in record_frames (+1964.7), split evenly between the two walks.

Cause: my refactor of `walk` (`table.child(..).ok_or(PageError::Unmapped)?`) moves the 32-byte
`Table` through a `Result` with two `memcpy` calls a level in the release build (the old `match`
copied it directly); `record_frames` walks twice for every word of every record. Not the ASIDs,
not flushes. Fix folded (bcee118b0): `walk` back to a `match`; its rv64 code is 62 lines and no
`memcpy` (main's: 127 lines, the regressed one: 77 lines with 2 `memcpy`); no mem.rs function
calls `memcpy` now. Kernel ceiling 8361. To confirm: one short asid-cost run on the tip.

Observation for later (no work in this package): on main a kernel entry costs about 100,000
guest instructions and an empty round trip about 300,000; `record_frames` alone is about 7,000
ticks (88,000 instructions) a round trip, a page walk and an ownership walk for every word.

## Confirming run on bcee118b0 (both widths; all PASS, exit 0)

| | main (88909e857) | bcee118b0 | change |
| --- | --- | --- | --- |
| icount rv64, round trips within | 24,497,061 | 24,053,620 | -1.8% |
| icount rv64, round trips between | 25,642,746 | 25,218,347 | -1.7% |
| icount rv64, map/touch/unmap | 2,595,352 | 2,609,902 | +0.6% |
| icount rv32, round trips within | 25,591,742 | 24,511,960 | -4.2% |
| icount rv32, round trips between | 26,885,856 | 25,801,560 | -4.0% |
| icount rv32, map/touch/unmap | 2,906,117 | 2,909,581 | +0.1% |
| host rv64, within / between | 4,137,245 / 4,500,685 | 3,848,481 / 4,387,280 | -7.0% / -2.5% |
| host rv32, within / between | 4,217,689 / 4,576,025 | 3,719,833 / 4,281,699 | -11.8% / -6.4% |
| host map/touch/unmap rv64 / rv32 | 445,640 / 464,562 | 455,280 / 468,556 | +2.2% / +0.9% |

(microseconds for 10,000; icount: 125 guest instructions per µs.) asid-reuse-stale PASS on
both widths on bcee118b0 (B read 0; asid 9 / 16 bits).
