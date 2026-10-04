# GATE1 handoff 2 (gate1-implementer-2, 2026-10-02)

## State
- Branch wp-gate1 is at **f0e5ce674** on main 81b5ea38b. The tree is clean. Two commits:
  - 2c02a72ae: tests: a scheduling child's panic names where it was
  - f0e5ce674: tests: the kernel containment gate, hostile leases contained in one boot. This holds the case, roles, program, pages, the trace ring at 16384 pages, and the gate's own child entry.
- **Blocker, open with the orchestrator/Architect.** The ruled ring (kernel/src/sched.rs trace PAGES 8192 -> 16384) makes sched-budget-churn rv32 fail: the shell's victim share is 559 net against a ceiling of 550. With 8192 it is 499 and passes. memory_mib = 288 does not fix it (557). The cause is not the gate's code: it still fails with commit 1's sched.rs and the gate binary removed. Options put: a ring in between (12288; the gate would fill it to about 89%), K20's I/B/E/O records only where judged, or find why the share depends on the ring's frames. Detail: GATE1-ring-churn.md.
- The other bench failures are explained:
  - sched-latency rv32 was mine and is fixed. The gate's code had been linked into every sched binary through the shared child(). It now has containment_child, set with Bench::set_entry.
  - handle-chain-fault, endpoint-destroy-open-calls and process-chain-fault pass alone 3/3. In the bench one case got another's fault-injected kernel: a bench build race to look at.

## The gate
- Case tests/kernel-containment.toml, qemu_seed = 13, both widths, checked and tracing kernel. Program tests/programs/src/bin/kernel-containment.rs; roles in tests/programs/src/sched.rs, under "The containment gate".
- Leases come in pairs (D by deadline, H by decision), nine of each. D's deadline ends it while H lives, and H is destroyed after D's notices. Each lease's call and progress handles carry badges the stand-in mints (0x1000+seq, 0x2000+seq). The victim keys held calls by badge and counts CT_CALLERS = 4 per lease (fail bit 256). The launcher requires checked == 4 x leases made and none left.
- The stand-in takes notices with **one receive bounded by deadline + 5 s**. The earlier 200 ms poll made a fake 35 ms group on some seeds (red B1).
- Verdict bits: steward 1/2/4/8/16/32; victim 1/2/4/8/16/32/64/128/256.

## Sweep (sweep4, on ca5a6437b, before K20; run one seed at a time)
- 32/32 PASS, logs .wash/local/GATE1-sweep4-logs/<arch>-<seed>.log, script gate1-sweep4.sh.
- Notice p99 24716-26197 µs, one group. Seed 13 is the worst on both widths for the notice (26197 rv32, 25794 rv64) and lease end (30889 / 30251), and on rv32 for R10 (22517). rv64's worst R10 is seed 16 (22385). The table is on docs/kernel/README.md, Containment, under The run.
- Earlier sweeps: sweep2 (pre-K18) and sweep3 (K18, with the poll artefact) are superseded.
- **Next sweep: run seeds in parallel** (24 cores, icount makes the numbers host-independent). Build once first. Logs go to target/testbench/<case>-<arch>-smp1.log, a fixed name, so either run 2-way across widths or use separate target dirs.

## Pages
- README Containment: built, tested by bench:kernel-containment. It has the sweep table, the 65% margin, why the old group existed (the poll), and the per-width seed sentence. "Two leases live at every deadline's end" is written exactly.
- scheduling.md: the gate's notice at seed 13, 25,794 net / 95,579 gross / 1,158,114 µs audit on rv64 and 26,197 / 97,703 on rv32. Negatives: audit-unstamped rv64 84,710; audit-billed 44,761 / 45,577 (GATE1-neg-*.out). The ring line says 64 MiB.
- budgets.md: the 102/251 ms history is replaced by words. The "Built" sentence gives the seed-13 R10 p50/p99, 18.7/22.5 ms on rv32 and 18.6/22.4 ms on rv64.
- m1-separation.md: the row names `kernel-containment`, and the gate is under Progress.

## For INIT1 / K16
- The gate is fixture-sensitive. Any change to the shared sched.rs launcher or child path shifts other sched cases' rv32 numbers. Keep case-only code behind a case's own entry.
- The scheduling numbers are deterministic per seed; a change of one slice is a real change.
- Ring fill at seed 13 is 63% / 67% of 2^21 records. More trace record kinds will need room.
- Pinned seed 13. If the fixture or kernel changes the phase, re-sweep and re-pin from the sweep's worst.
- Split tooling for a notice miss: GATE1-two-lease-split.py (a, b, c1, c2 terms from X/Y/U/V and K/R/W).

## Update: final (ring from the top of RAM)
- Tip **ba01f7578** (2c02a72ae, ba01f7578) on 81b5ea38b, clean.
- kernel_frame takes the ring's frames from the top of RAM; the ring stays 16384 pages. The first-fit allocator scan is K21's finding.
- churn rv32 498, rv64 504. Gate at seed 13: fill 650,018 (31%) rv64 and 726,672 (35%) rv32 of 2,097,152; notice 25829 / 26195; R10 22376 / 22368; lease end 30332 / 30783.
- Whole bench alone on ee2e24cec: 282 PASS, 1 SKIP, 1 FAIL (size-budget, kernel 7853 > 7852). kernel_frame was then rewritten in two code lines: size-budget PASS, churn PASS, fmt and doccheck clean, no warnings. The whole bench was not rerun after that.
