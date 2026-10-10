# IRQ1 report (irq1-implementer)

Branch `wp-IRQ1` (worktree .worktrees/IRQ1), on main b2f1a1bb9 (CONW1). Two commits:
- 2078fea2d loader: the Hart argument carries each started hart's PLIC S-mode context
- 916ff1d3e kernel, testbench: device interrupts reach every hart, so a driver wake waits at most one section

Design note: .wash/local/IRQ1-design.md. Measurements: .wash/local/IRQ1-numbers-1.md, -2.md.

## Delivered
- Loader: `Hart` ends with each listed hart's S-mode context by boot index (host test
  `each_listed_hart_carries_its_s_mode_context_by_boot_index`).
- Kernel (intc_plic.rs): CONTEXTS per hart; each started hart enables every source `Devs` names on
  its own context, threshold 0 (`irq::online`, boot hart at init, others in hart_main under the
  lock); claim/complete on the caller's context, one section, a checked assert that no claim is
  open. Masking by priority 0/1 (init writes 0 to every Devs source). SEIE set in timer::init_hart
  on every hart (arch::init's sie block removed: arch unsafe 11 -> 10, ratchet lowered).
- Halts (arch/riscv/mod.rs): lock and shootdown waits halt for SSIP alone (`halt_for_ipi`), and no
  halt runs `wfi` while an awaited interrupt is pending (`halt_masking`); checked-build count
  `halts: N skipped` required > 0 in both lock-contention cases.
- Billing: an external entry from user mode starts unbilled; a claim begins billing the
  interrupted budget from the expiry's end (as before); a claim of nothing leaves the rest to the
  expiry's last budget or nobody. Trace records `x`/`c`; oracle check + host test
  `a_device_interrupt_that_claims_nothing_bills_nobody`.
- Cases: sched-lock-contention gates p50 and p99 at 2 harts; -4 gates p50, records p99, requires a
  non-boot hart to claim >= 10; new must_fail twin `irq-boot-hart-only` (feature of that name).
- Test programs: sched-latency and kernel-containment's clock check brackets time_now between two
  RTC reads (it failed at 2 harts by 1.07 ms: an interrupt on the measuring hart between reads).
- Pages: boot.md (step 1, Hart row, contract, status), devices.md R5, scheduling.md (Charging,
  R78, Responsiveness, R23, residuals incl. QEMU's turns at four harts), testbench.md (oracle
  records, statuses), SECURITY.md R78 row, plan m2 several-harts step 4.

## Numbers (gate run on 6ceb71fa1, same code as 916ff1d3e)
Driver wake net p50/p99 ms: 2 harts rv64 8.5/10.6, rv32 8.5/10.4 (base 18.5/19.2, 18.3/19.1);
4 harts rv64 4.3/59.1, rv32 5.3/58.7 (base 4.3/59.3, 7.1/86.9; p99 varies 40-59 by run).
Twin: 20.2 ms gross p50, fails as required. Claims 2 harts [200, 1] (the boot hart is the idle
one in this build), 4 harts [59,3,137,2] / [36,140,23,2]; twin [201, 0].

## Gate (q run, both widths, exit 0 each; .worktrees/IRQ1/.tmp/gate/results.txt)
build (rv64+rv32), formatting, no-cruft, docs, size-budget (kernel 9827 -> 9938, loader 913 -> 922,
Size budget lines in each commit), unsafe-budget (kernel arch 10), host-tests (24 suites),
sched-lock-contention, -4, irq-boot-hart-only, uart-irq, irq-first-receive, receive-bad-record,
device, rustsbi-boot, smp-boot, smp-lock-wait, smp-evict, smp-shootdown, smp-fence, sched-latency,
kernel-containment, sched-timer-flood, sched-wake-no-preempt, sched-budget-churn, sched-share,
smoke set (userland-boot, init-boot, bench-net-peer, ipc-outcomes, sum-clear,
lend-untouched-page, and at --smp 4). After the last amend (pages, two expect lines): docs,
sched-lock-contention(-4), irq-boot-hart-only rerun, PASS. Not run: the whole bench (train's),
model/mutations (no model rule changed: R5's mask is the same mask; billing of an empty claim is
not in the model).

## Documentation check
Checked: README.md, GETTING-STARTED.md (no interrupt/hart claims), docs/kernel/README.md
(interrupt row still true; its unsafe tally "kernel's 44" was already stale on main, 13+11+16=40,
now 39: not changed here, flagged), docs/plan/m2-usable-shell.md (step 4 updated), loader has no
README, devices.md, boot.md, scheduling.md, testbench.md, SECURITY.md updated as above.

## Open risks / notes for review
- Which hart idles at 2 harts depends on boot timing; the twin's failure relies on its hammer
  landing on the boot hart (said on the page). The 4-hart claim count carries the any-hart verdict.
- A "Size budget" increase of 111 kernel lines: intc_plic, trace records, oracle-facing counters.
- I read the diffs in full, not every whole file committed (scheduling.md, testbench.md,
  sched_oracle.rs and sched.rs are thousands of lines).
- SMP4 will add trace letters h, j, y: whoever rebases second merges the oracle's parse list.
