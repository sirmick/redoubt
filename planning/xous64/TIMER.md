# xous64 timer design

Status: decided 2026-09-18.

## Problem
On Precursor the ticktimer server owns an MMIO timer peripheral and its interrupt, like any driver.
Standard RISC-V platforms have no such device for S-mode. The timer is a hart resource: the `time`
CSR, and a deadline programmed through SBI TIME (`sbi_set_timer`) or, with Sstc, the `stimecmp` CSR.
Neither can be touched from U-mode, and the interrupt arrives as `scause` = supervisor timer, not
through the PLIC.

## Design
Keep the microkernel split: the kernel exposes the hart timer as if it were a device, and policy
(tick rate, sleep queues, wall time) stays in a userspace ticktimer server.

- **Interrupt**: the supervisor timer interrupt is delivered as **IRQ 0**. PLIC source 0 does not exist
  ("no interrupt"), so the number is free. A server claims it with the ordinary `ClaimInterrupt`.
- **Reading time**: the kernel sets `scounteren.TM`, so userspace reads `rdtime` directly. No syscall.
- **Programming it**: `SysCall::PlatformSpecific`, numbers in `xous::arch::platform_call`:
  - `TIMER_TIMEBASE` -> ticks per second (from `/cpus/timebase-frequency`, passed by the loader in a
    `Time` tag).
  - `TIMER_SET_DEADLINE(abs_ticks)` -> arm the timer. Only the owner of IRQ 0 may call it.
- **One-shot semantics**: when the interrupt fires the kernel masks `sie.STIE` (the pending bit stays
  set until a new deadline is written) and dispatches IRQ 0. The handler arms the next deadline.
- **No nesting**: Xous does not nest interrupt handlers, so `disable_all_irqs()` masks the timer along
  with the interrupt controller, and `enable_all_irqs()` restores it if a deadline is armed.

## Abstraction
`arch/riscv/timer_*.rs` is a backend like the interrupt controller: `init`, `set_deadline`,
`on_interrupt`, `mask`, `unmask`, `timebase`. `timer_sbi.rs` uses SBI TIME via `sbi-rt`; an Sstc backend
would write `stimecmp` instead (no firmware round trip), and platforms with an MMIO timer owned by
userspace (Precursor) use `timer_none.rs`. Selected by capability feature, not by XLEN.

## Scheduling
This does not by itself add time-slice preemption. Stock Xous resumes the interrupted thread after an
interrupt handler returns; threads are rescheduled when messages are delivered or they yield. That
policy is kept for now. Per-hart time slicing is a Phase 3 (SMP) decision.
