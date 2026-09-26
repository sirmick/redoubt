// SPDX-License-Identifier: MIT OR Apache-2.0

//! Hart timer backend for platforms with SBI firmware, using the SBI TIME extension. The kernel
//! owns the timer (kernel/timer.md, "The hart timer"); `crate::time` decides what it is armed for.
//!
//! SBI TIME rather than Sstc: it works under every SBI firmware on both widths, with no firmware
//! configuration (Sstc needs `menvcfg.STCE`), and one `ecall` per arming is nothing at a 10 ms
//! slice.

use core::sync::atomic::{AtomicUsize, Ordering};

use riscv::register::{scounteren, sie};

// The timebase is a frequency in Hz, set once at boot and read-only afterwards, so a plain
// `AtomicUsize` is enough and stays lock-free on rv32 (which has no 64-bit atomics). Any
// real RISC-V timebase fits in 32 bits.
static TIMEBASE: AtomicUsize = AtomicUsize::new(0);

/// The `time` CSR when the kernel started, so that time counts from boot.
static BOOT_TICKS: crate::cell::KernelCell<u64> = crate::cell::KernelCell::new(0);

pub fn init() {
    if let Some(arg) =
        crate::args::KernelArguments::get().iter().find(|a| a.name == u32::from_le_bytes(*b"Time"))
    {
        TIMEBASE.store(crate::args::wide(arg.data, 0), Ordering::Relaxed);
    }
    // Fail closed: without a timebase no timeout, slice or deadline means anything.
    assert!(timebase() != 0, "boot: the loader reported no timebase (`Time`)");
    BOOT_TICKS.with(|t| *t = riscv::register::time::read64());
    // Nothing is due yet.
    sbi_rt::set_timer(u64::MAX);
    // SAFETY: these only choose which interrupts reach the trap handler. The kernel itself runs
    // with `sstatus.SIE` clear, so the timer interrupt is taken from U-mode or in `idle`, where
    // the trap handler is ready for it; `scounteren.TM` exposes the read-only `time` counter to
    // U-mode (`rdtime`), with no memory effect.
    unsafe {
        sie::set_stimer();
        scounteren::set_tm();
    }
}

/// Ticks of the `time` CSR per second.
pub fn timebase() -> u64 { TIMEBASE.load(Ordering::Relaxed) as u64 }

/// Ticks since boot.
pub fn now_ticks() -> u64 { riscv::register::time::read64().saturating_sub(BOOT_TICKS.with(|t| *t)) }

/// Monotonic microseconds since boot (kernel/timer.md, `time_now`), rounded down.
pub fn now_us() -> u64 { ticks_to_us(now_ticks()) }

/// Ticks since boot as microseconds, rounded down.
pub fn ticks_to_us(ticks: u64) -> u64 {
    // Whole seconds, then the remainder: no 128-bit arithmetic, and no overflow while the
    // remainder (below the timebase, which fits in 32 bits) times 10^6 fits in 64 bits.
    let hz = timebase().max(1);
    (ticks / hz).saturating_mul(1_000_000).saturating_add((ticks % hz) * 1_000_000 / hz)
}

/// The first tick at or after `us` microseconds since boot, rounded up, so that an interrupt
/// armed for it never comes before `now_us() >= us`; `u64::MAX` for a time that never comes.
pub fn us_to_ticks(us: u64) -> u64 {
    let hz = timebase().max(1);
    let (secs, frac) = (us / 1_000_000, us % 1_000_000);
    // frac < 10^6 and hz < 2^32: the product fits in 64 bits.
    secs.checked_mul(hz).and_then(|t| t.checked_add((frac * hz).div_ceil(1_000_000))).unwrap_or(u64::MAX)
}

/// Request a timer interrupt once `ticks` (since boot) have passed; `u64::MAX` for none. Writing
/// a deadline also clears a pending timer interrupt.
pub fn set(ticks: u64) {
    let at = ticks.checked_add(BOOT_TICKS.with(|t| *t)).unwrap_or(u64::MAX);
    sbi_rt::set_timer(at);
}
