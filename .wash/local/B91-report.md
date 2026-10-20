# B91 report: smp-evict-mttcg's missing shootdown line

Branch `wp-B91`, head `62bc7eaa7` on origin/main `5ced0b2b3` (rebased after train 83). Two commits:
`fed13ec45` (the case program and both twins' comments) and `62bc7eaa7` (testbench.md's
timed-bounds condition sentence). Tier A (a kernel case), reviewer kernel-red-4. No kernel line
moves: the size and unsafe budgets are untouched.

## The cause (arm ii, the kernel case's ordering)

Evidence: `.tmp/B91/train82-fail-rv32-smp-evict-mttcg.smp2.console.log` (train 82's kept
console) and `.tmp/B91/base/*.console.log` (my baseline, below). Every failing console runs to
the kernel's last shutdown line (`halts: ...`), so the reader drained it and the last line was
flushed (arms i and iii excluded); the `shootdown:` line is absent because the kernel never
printed it: `ptable.rs::evict` prints only when `hart::shootdown` asked a hart, and at the
destroy no other hart ran the writer (`audit_left` passed, rightly).

How the writer left its hart: the judge slept 5 ms (`wait_ms` = a receive timeout, a kernel
deadline). `time::rearm` lands a sleeping payer's deadline on the first hart to re-arm, often the
writer's at its 1 ms slice end; there `sched::repick` sees `time::due()`, goes through `kmain`,
whose expiry wakes the judge and whose pick gives the judge that hart. The writer is `Ready` on no
hart while `wake_idle` IPIs the idle hart to fetch it; the judge returns to user mode and calls
`destroy` at once, racing that pick; when the destroy takes the lock first, `asked == 0`. Under
icount the two harts interleave the same way every boot, so only the MTTCG twin flaked.

## The fix (by construction, `tests/programs/src/bin/smp-evict.rs` only)

The Architect asked for the judge to order on the writer's own signal, spinning. No shared memory
exists between two processes here (`process_map` moves pages; a lent page is unmapped at the
lender, `lender-touches-lent`; a one-way send is a rendezvous), so the writer's counter is B's
page count: the writer maps one more page of B at each of its first `BEATS` (400) passes, and the
judge reads it with `budget_usage`, spinning on its own hart until the count has grown twice,
then destroys at once. The writer is the only other runnable thread, so its slice ends re-pick it
in place and nothing moves it between the second beat and the destroy. The clock bounds only the
watch (`WAIT`, 5 s, a loud `b.check` failure). The check line reports B's pages at the destroy.
Measured over 80 fixed boots: the judge had its two beats within the writer's first sixteen,
once by its 127th (`rv64-14`); 400 is the headroom, and B's frames (image, 16 written, up to 400
beats) stay under the checker's 512.

## Runs (all through `scripts/jobs.mk` under `q`; the quiet class for the mttcg twin)

- Baseline, the original program, 40 boots per width at 2 harts (`.tmp/B91/base/summary.log`,
  every console kept): **8 of 80 failed** (rv64 3, rv32 5), all "guest exited while waiting for
  /^shootdown.../", all consoles complete and without the line. 08:31-08:34Z.
- Fixed (BEATS 256), 40 per width: **80 of 80 passed** (`.tmp/B91/fixed/`), every console with
  the shootdown line. 08:34-08:36Z.
- Fixed at the final tree (BEATS 400, after the rebase), 20 per width: **40 of 40 passed**
  (`.tmp/B91/fixed2/`).
- `make -k -f scripts/jobs.mk set CASES="docs formatting asid-reuse-stale smp-boot
  smp-evict-mttcg smp-evict smp-fence smp-inflight-race smp-lock-wait smp-shootdown-mttcg
  smp-shootdown"` after the rebase: all PASS on both widths (smp-boot at 2 and 4 harts);
  `set CASES="formatting smp-evict"` again at the final tree: PASS. The icount twin's verdict is
  unchanged (PASS both widths, its console shows the heartbeat line and the shootdown line).
- Compile check both widths: `cargo check -p test-programs --release --target
  riscv64gc-unknown-none-elf --target riscv32imac-unknown-none-elf --bin smp-evict`: ok.
- The fix's negative: the baseline above is the program without it, failing 1 in 10.

## Pages

- `tests/smp-evict-mttcg.toml`, `tests/smp-evict.toml`: one comment each stating the ordering the
  case relies on (the mttcg one what the sleep cost). No package ids, no dates.
- The program's module docs state the mechanism and why it spins.
- `docs/testbench.md` ("On a shared host", after "bound on a time ... until it fails alone"): the
  condition every stated bound on a time holds under (one whole bench at a time, QEMU its alone,
  other work host-only and off the bench's cores). BENCH2's branch did not carry it (checked its
  testbench.md diff before the rebase, and the published text after).
- Summaries checked, no change needed: `README.md`, `GETTING-STARTED.md`, `docs/README.md` (no
  mention of the case); `docs/kernel/memory.md` Residual-risks bullet and
  `docs/kernel/memory-layout.md` `satp` status line (cite `bench:smp-evict` as the destruction's
  shootdown case, still true); `docs/plan/m2-usable-shell.md:243` (the same citation);
  `docs/testbench.md:293` (the mttcg list, unchanged). No page described the judge's ordering, so
  the sentence lives with the case.

## Open

- The heartbeat is a `map_anon` a pass (a kernel entry a pass on the writer's hart); the case's
  property (B's frames reused only after the shootdown) is unchanged, with more frames to reuse.
- The hart id the Architect mentioned is not readable from user mode; the observation is that B's
  count grew while the judge held its own hart, which places the writer on the other.
