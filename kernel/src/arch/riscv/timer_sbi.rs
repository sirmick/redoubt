// SPDX-License-Identifier: MIT OR Apache-2.0

//! Hart timer backend for platforms with SBI firmware, using the SBI TIME extension.
//! See `planning/redoubt/TIMER.md`.

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use riscv::register::{scounteren, sie};

/// The timer is presented to userspace as this interrupt. PLIC source 0 does not exist.
pub const IRQ: usize = xous_kernel::arch::platform_call::TIMER_IRQ;

// The timebase is a frequency in Hz, set once at boot and read-only afterwards, so a plain
// `AtomicUsize` is enough and stays lock-free on rv32 (which has no 64-bit atomics). Any
// real RISC-V timebase fits in 32 bits.
static TIMEBASE: AtomicUsize = AtomicUsize::new(0);
/// A deadline has been set and its interrupt has not been delivered yet.
static ARMED: AtomicBool = AtomicBool::new(false);
/// An interrupt handler is running, so the timer interrupt must stay off.
static MASKED: AtomicBool = AtomicBool::new(false);

pub fn init() {
    if let Some(arg) = crate::args::KernelArguments::get().iter().find(|a| a.name == u32::from_le_bytes(*b"Time")) {
        TIMEBASE.store((arg.data[0] as u64 | (arg.data[1] as u64) << 32) as usize, Ordering::Relaxed);
    }
    BOOT_TICKS.with(|t| *t = riscv::register::time::read64());
    // Let userspace read the `time` CSR directly, via `scounteren.TM`.
    // SAFETY: this exposes a read-only counter to U-mode and has no memory effect.
    unsafe { scounteren::set_tm() };
}

/// Mask or unmask the supervisor timer interrupt at the hart.
fn set_interrupt_enabled(enabled: bool) {
    // SAFETY: the kernel itself runs with `sstatus.SIE` clear, so this only changes whether
    // the interrupt is taken from U-mode, where the trap handler is ready for it.
    unsafe {
        if enabled { sie::set_stimer() } else { sie::clear_stimer() }
    }
}

/// Ticks of the `time` CSR per second, or 0 if the loader did not report it.
pub fn timebase() -> u64 { TIMEBASE.load(Ordering::Relaxed) as u64 }

/// The `time` CSR when the kernel started, so that `now_us` counts from boot.
static BOOT_TICKS: crate::cell::KernelCell<u64> = crate::cell::KernelCell::new(0);

/// Monotonic microseconds since boot (KERNEL-SPEC.md, `time_now`).
pub fn now_us() -> u64 {
    let ticks = riscv::register::time::read64().saturating_sub(BOOT_TICKS.with(|t| *t));
    // Whole seconds, then the remainder: no 128-bit arithmetic, and no overflow while the
    // remainder (below the timebase, which fits in 32 bits) times 10^6 fits in 64 bits.
    let hz = timebase().max(1);
    (ticks / hz) * 1_000_000 + (ticks % hz) * 1_000_000 / hz
}

/// Whether `irq` is the timer, rather than a source on the interrupt controller.
pub fn owns(irq: usize) -> bool { irq == IRQ }

/// Request an interrupt once `time` reaches `deadline`. Writing a deadline also clears a
/// pending timer interrupt.
pub fn set_deadline(deadline: u64) {
    sbi_rt::set_timer(deadline);
    ARMED.store(true, Ordering::Relaxed);
    if !MASKED.load(Ordering::Relaxed) {
        set_interrupt_enabled(true);
    }
}

/// The timer interrupt fired. It stays pending until a new deadline is written, so it
/// has to be masked or the hart would trap again immediately.
pub fn on_interrupt() {
    ARMED.store(false, Ordering::Relaxed);
    set_interrupt_enabled(false);
}

pub fn mask() {
    MASKED.store(true, Ordering::Relaxed);
    set_interrupt_enabled(false);
}

pub fn unmask() {
    MASKED.store(false, Ordering::Relaxed);
    if ARMED.load(Ordering::Relaxed) {
        set_interrupt_enabled(true);
    }
}
