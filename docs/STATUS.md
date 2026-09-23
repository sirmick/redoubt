# Implementation status

Redoubt boots on QEMU `virt` under vendored RustSBI on rv64 and rv32. The native OS userland
is not integrated end to end; beamlet currently runs on the host. [PLAN](PLAN.md) owns milestone
outcomes, [SWARM](SWARM.md#claims) owns package state, and [BUILD-PLAN](BUILD-PLAN.md) owns acceptance.

| Area | Implemented and checked | Remaining |
| --- | --- | --- |
| Boot | Loader verifies domain-separated Ed25519 bundles; no usable seed or bad signature refuses boot. Both widths boot. | Loader/firmware authentication, production keys, rollback protection. |
| Memory and devices | Typed W^X page tables, zeroed anonymous RAM, default-deny legacy device grants; new MMIO/DMA/reset handles. | User code retains a writable kernel-only physmap alias. DMA drivers remain trusted. Device-policy questions 142–147 are open. |
| Budgets and IPC | Accounting, carving, labels, revocation, handles, endpoints, call/send/receive/reply, abandoned calls and protected loans. Explicit caller ownership and server delivery outcomes; partial replies and failed-output rollback. | IPC1 model/timer/concurrent-completion acceptance; see below. |
| Record validation | Budget and IPC records must be backed, permitted, owned RAM. MMIO and borrowed aliases are rejected before record access. | Preserve this guard when adding syscalls. |
| Scheduling and processes | Native process/thread creation, startup and exit; creator-paid notices, blame, loan cleanup and PID reuse; cooperative scheduling, timer IRQ tests and a two-hart lock spike. | Kernel-owned timeouts/deadlines and preemption. No full SMP scheduler. |
| Native libraries and servers | sys/rt/wire/signing, pure-Rust littlefs, keyd, bootfsd, consoled and in-tree virtio-blk/blkd have host tests. | Init/startup integration, fsd, network/steward/SSH stack and Redoubt beamlet platform. Console typed size/resize are target behavior; typed parking awaits 163. |
| Verification | Boot attack cases, checked builds, registered server host tests and both-width builds, wire-generator drift checks and a fail-closed unsafe ratchet. | Model replay, hosted-kernel compilation repair and bench registration, and kernel fuzzing. |

## Acceptance gaps

- **IPC1 is not accepted.** The model is absent from the active workspace; its remote source needs
  current scheduling and IPC semantics. K5 real-timer cases remain pending. Native exit and
  loan cleanup have real-kernel coverage; the shared server's terminal fallback serving path
  remains host-tested pending server boot integration. Concurrent completion coverage remains open; reconcile its
  milestone scope with [PLAN's SMP section](PLAN.md#smp-after-milestone-1).
- Questions **164–166** leave mediation, authority closure and the wakeup bound unresolved.
  They qualify the security/latency claims, not just their implementation schedule.
- `consoled` unknown-request handle cleanup and the broader raw-syscall/owning-runtime
  composition need follow-up; [review tasks](https://github.com/sirmick/redoubt/blob/main/ASTRA.md).
- **K4 is not fully accepted.** Its native lifecycle integration and kernel-notice-based `wx`
  verdicts are implemented. Clean guest bundle-file readback remains an explicit gate for the
  R2/R3 startup handoff (answer 169); the current bundle-file case still tests loader refusal.
- The hosted-kernel test target fails to compile: both the recovery baseline and final candidate
  report 112 compiler errors. It is not part of the registered bench; native QEMU results do
  not establish that hosted target's compatibility.

## Verification

Run `./test --list` for the case inventory and `./test` for the bench. [testbench.md](testbench.md)
defines verdicts and checked builds. Case definitions and tests own the exact coverage.

The budget/MMIO regression in `budget-syscall-attack` reproduced an rv32 kernel panic before
the shared-validator fix. It checks input/output refusal, error precedence and continued kernel
service on both widths. IPC coverage remains in `ipc-outcomes` and the revocation/lend cases.

The unsafe ratchet rejects missing or empty configured source roots; it does not prove that
all TCB components were configured. Runtime ceiling: 9; blkd: 4; bootfsd/consoled together: 0;
all require zero undocumented uses. Server host/build registrations do not establish boot integration.

Prior budget-record/IPC validation (2026-09-22, through `26cba3022`): the full bench passed
99 executions across 56 cases, including budget/MMIO and IPC outcomes on rv32/rv64;
wire-generator tests passed 16/16. This is regression evidence, not acceptance of the
outstanding packages.

Server-verification recovery (2026-09-22): 67 server host tests and 9 harness tests passed;
all three servers built for both widths. The full bench passed 107 executions across 61 cases,
with no failures or skips. Actual unsafe counts were blkd 4, bootfsd/consoled 0 and runtime 9,
with zero undocumented uses. The consistency, defensive and simplification reviews completed;
the documentation attribution finding was corrected. This adds permanent verification coverage,
not server boot integration.

Native process-lifecycle recovery (2026-09-22): the consistency, defensive and simplification
reviews completed. Findings corrected exit-notice label checks, live-thread allocation tracking,
map/start check ordering, creator-budget teardown, final-thread syscall/return cleanup, and
missing blame/lend regressions. Simplification removed duplicated frame-release code and unused
process metadata. IPC stress tests now acknowledge server capacity and distinguish queued
cancellation from received-call abandonment using the specified lend disposition.
The final complete bench (`cargo testbench`) passed 115 executions across 65 cases, with no
failures or skips, including process and IPC cases on both widths. Focused validation passed
8 process, 10 IPC and 2 timer executions. The unsafe checker includes `kernel/src/process.rs`;
all existing ceilings remain unchanged, runtime stays at 9, and undocumented counts remain zero.
These results do not close bundle-file readback, timer/preemption, concurrent completion or
real-kernel model replay acceptance.
