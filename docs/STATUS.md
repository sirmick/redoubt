# Implementation status

Redoubt boots on QEMU `virt` under vendored RustSBI on rv64 and rv32. The native OS userland
is not integrated end to end; beamlet currently runs on the host. [PLAN](PLAN.md) owns milestone
outcomes, [SWARM](SWARM.md#claims) owns package state, and [BUILD-PLAN](BUILD-PLAN.md) owns acceptance.

| Area | Implemented and checked | Remaining |
| --- | --- | --- |
| Boot | Loader verifies domain-separated Ed25519 bundles; no usable seed or bad signature refuses boot. Both widths boot. | Loader/firmware authentication, production keys, rollback protection. |
| Memory and devices | Typed W^X page tables, zeroed anonymous RAM, default-deny legacy device grants; new MMIO/DMA/reset handles. | User code retains a writable kernel-only physmap alias. DMA drivers remain trusted. Device-policy questions 142–147 are open. |
| Budgets and IPC | Accounting, carving, labels, revocation, handles, endpoints, call/send/receive/reply, abandoned calls and protected loans. Explicit caller ownership and server delivery outcomes; partial replies and failed-output rollback. | IPC1 model/timer/native-exit acceptance; see below. |
| Record validation | Budget and IPC records must be backed, permitted, owned RAM. MMIO and borrowed aliases are rejected before record access. | Preserve this guard when adding syscalls. |
| Scheduling and processes | Cooperative scheduling and legacy process/thread facilities; timer IRQ tests and a two-hart lock spike. | New process/thread syscall family, native exit cleanup, kernel-owned timeouts/deadlines and preemption. No full SMP scheduler. |
| Native libraries and servers | sys/rt/wire/signing, pure-Rust littlefs, keyd, bootfsd, consoled and in-tree virtio-blk/blkd have host tests. | Init/startup integration, fsd, network/steward/SSH stack and Redoubt beamlet platform. Console typed size/resize are target behavior; typed parking awaits 163. |
| Verification | Boot attack cases, checked builds, host tests, wire-generator drift checks and a fail-closed unsafe ratchet. | Model replay, kernel hosted tests in the bench, kernel fuzzing, omitted server bench/budget registrations. |

## Acceptance gaps

- **IPC1 is not accepted.** The model is absent from the active workspace; its remote source needs
  current scheduling and IPC semantics. K5 real-timer cases and native process-exit cleanup are
  pending. The shared server's terminal `process_exit` fallback is host-tested only because the
  new syscall is unimplemented. Concurrent completion coverage remains open; reconcile its
  milestone scope with [PLAN's SMP section](PLAN.md#smp-after-milestone-1).
- Questions **164–166** leave mediation, authority closure and the wakeup bound unresolved.
  They qualify the security/latency claims, not just their implementation schedule.
- `consoled` unknown-request handle cleanup and the broader raw-syscall/owning-runtime
  composition need follow-up; [review tasks](https://github.com/sirmick/redoubt/blob/main/ASTRA.md).
- `wx` remains a survival test until process-exit verdict integration. The host VM and host
  substitute kernels do not establish target lifecycle correctness.

## Verification

Run `./test --list` for the case inventory and `./test` for the bench. [testbench.md](testbench.md)
defines verdicts and checked builds. Case definitions and tests own the exact coverage.

The budget/MMIO regression in `budget-syscall-attack` reproduced an rv32 kernel panic before
the shared-validator fix. It checks input/output refusal, error precedence and continued kernel
service on both widths. IPC coverage remains in `ipc-outcomes` and the revocation/lend cases.

The unsafe ratchet rejects missing or empty configured source roots; it does not prove that
all TCB components were configured. Runtime ceiling: 9, no undocumented uses. Server omissions
are tracked in SWARM. Validation for this change (2026-09-22): the full bench passed 99 executions across 56 cases,
including budget/MMIO and IPC outcomes on rv32/rv64; wire-generator tests passed 16/16.
This is regression evidence, not acceptance of the outstanding packages.
