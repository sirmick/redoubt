# K25 — findings so far (draft; final report replaces this)

Branch wp-K25 (from main fdafcf2cb), head f39990f3c. Scratch trees (not branches):
`.worktrees/K25-base` (777bba164) and `.worktrees/K25-smp1` (14aceaa63), detached; to be removed.

## Reproduced on main fdafcf2cb (consoles: /tmp/k25/consoles-main)

| case | rv64 | rv32 |
| --- | --- | --- |
| sum-clear | FAIL (timeout) | FAIL (timeout) |
| lend-untouched-page smp=4 | FAIL (`harts: FAIL`) | FAIL (same) |
| sched-exit-churn | FAIL threads-exit net 439 | FAIL threads-exit net 432 (also fails at 777bba164: 433) |
| sched-cluster-old-control | PASS | FAIL |

## Causes 1 and 2: fixed (commits 54102cd9a, fd6579b5e, f39990f3c)

1. sum-clear: SMP1's `retry_stale` also "retried" the kernel's own S-mode load through a valid
   user leaf; `resume_current` then resumed the user thread with the S-mode trap's registers
   (`PROGRAM HALT: Instruction page fault of 0xffffffffffd06aae`). Now only user-mode faults retry.
   Page: kernel/memory-layout.md `satp` says so.
2. lend-untouched-page smp=4: `hart::report` printed `harts: FAIL: 4 in the tree, 4 started, 1 ran
   user code` for a one-process boot. Now a plain count; smp-boot forbids `^harts: [0-9]+ in the
   tree` and still expects the "all" line.
3. Both cases moved into guest time (docs/testbench.md said they would once they passed).
   jobs.mk, both widths: sum-clear, lend-untouched-page (smp 1, 4), smp-boot (2, 4): all PASS.

## Causes 3 and 4: no logic defect found; timing shifts on margin-less fixtures

### sched-exit-churn threads-exit (rv64)
- The program binary is byte-identical at 777bba164 and 14aceaa63; 777bba164's kernel+stride on
  SMP1's tree (loader, layout) gives net 450 (pass): the shift is the kernel's.
- At 777bba164 it sat exactly on its bound: net 450, gross 443. rv32 fails there already (433).
- Instruction-level profile (QEMU exec log, whole run) and in-kernel tick stamps: SMP1 adds ~2 µs to
  the entry path (lock, serve, assert) and its return path is shorter; no single costly addition.
- Pure perturbation, no logic change (an n-iteration delay at every trap entry):
  base kernel n=0/30/100 -> net 450/452/446; SMP1 kernel n=0/30/100 -> 439/484/444.
  Six extra trace records moved gross from 443->436 (base) and 432->477 (SMP1).
  So the clause is chaotic in kernel timing: the race between the churn worker's exit (0.9 slice)
  and the main thread's 1 ms poll decides how often the attacker's budget empties (D/W 186 vs 290
  per window), and the victim's own switch overhead (~5-7% of its charge, accrued as its "user"
  time between `leave` and the next `from_user`) plus nobody's time (~3.5%) eat the whole 50/1000
  tolerance.

### sched-cluster-old-control (rv32)
- Systematic, not chaotic: base passes at n=0/30/100, SMP1 fails at all three.
- Mechanism: the control's timer_wake attempt 1 is served at ~1037 ms guest time in both kernels,
  but under SMP1 the cluster's setup ends ~16-20 ms later (CLUSTER-WINDOW 835575 -> 855519), so the
  same service reads as 130 ms latency instead of 151 ms; net falls under 50 ms and the known-bad
  control no longer shows a miss.
- Where the setup time went (per-call guest ticks over the run, rv32 checked):
  `unmap` 174.8k -> 214.5k ticks per call (x2), `map_device` 72.2k -> 88.1k per call (x5): +15.8 ms
  in all, per-page overhead SMP1 added to the map/unmap paths (per-hart audit log index,
  `stale::flushed`'s hart mask, and the KernelCell lock-holder assert, which alone is ~28% of it).
  Everything else is within 2%.

## Open
- How to treat 3 and 4 (question sent to the orchestrator).
- The smoke-set line: sched-exit-churn fails rv32 at 777bba164 and is chaotic on rv64; adding it
  to every package's short gate would fail or flake it.
