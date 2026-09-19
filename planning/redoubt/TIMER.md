# The hart timer

Owns: how the hart timer works today. The decided replacement is in RESOURCES.md (Clocks).

The RISC-V S-mode timer is a hart resource, not a device: the `time` CSR, and a deadline programmed
through SBI TIME (`sbi_set_timer`) or, with Sstc, the `stimecmp` CSR. U-mode cannot program it, and
its interrupt arrives as `scause` = supervisor timer, not through the PLIC.

## Built today
The kernel exposes the timer to userspace as if it were a device (backend `arch/riscv/timer_sbi.rs`):
- **Interrupt:** the supervisor timer interrupt is delivered as **IRQ 0** (PLIC source 0 does not
  exist, so the number is free). A process claims it with `ClaimInterrupt`.
- **Reading time:** the kernel sets `scounteren.TM`, so user mode reads `rdtime` directly.
- **Programming it:** `SysCall::PlatformSpecific` calls `TIMER_TIMEBASE` (ticks per second, from the
  loader's `Time` tag) and `TIMER_SET_DEADLINE(abs_ticks)`, allowed only to the owner of IRQ 0.
- **One-shot:** when it fires, the kernel masks `sie.STIE` and dispatches IRQ 0; the handler arms the
  next deadline. `disable_all_irqs()` masks the timer with the interrupt controller.
- Test `timer`: five one-shot ticks at 20 Hz.

There is no preemption: threads are rescheduled when messages are delivered or they yield.

## Decided (RESOURCES.md)
The kernel owns the timer outright: a deadline queue for time slices and sleepers, blocking receive
with a timeout, `scounteren.TM` cleared (user processes get 1 ms time from the kernel), and IRQ 0,
the `PlatformSpecific` timer calls and the userspace timer server are deleted. The backend
abstraction stays: `timer_sbi.rs` today, an Sstc backend (write `stimecmp`, no firmware round trip)
selected by capability feature, not by XLEN.
