// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

//! Interrupt controller backend for VexRiscv-based SoCs (Precursor, bao1x): a custom
//! pair of supervisor CSRs holding the interrupt mask and pending bits.

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

// use RAM-based backing so this variable is automatically saved on suspend
static SIM_BACKING: AtomicUsize = AtomicUsize::new(0);

// Interrupts are enabled very early on, so just assume they're on by default
static IRQ_ENABLED: AtomicBool = AtomicBool::new(true);


#[cfg(not(feature = "vexii-test"))]
fn sim_read() -> usize {
    let existing: usize;
    unsafe { core::arch::asm!("csrrs {0}, 0x9C0, zero", out(reg) existing) };
    existing
}

#[cfg(not(feature = "vexii-test"))]
fn sim_write(new: usize) { unsafe { core::arch::asm!("csrrw zero, 0x9C0, {0}", in(reg) new) }; }

#[cfg(not(feature = "vexii-test"))]
fn sip_read() -> usize {
    let existing: usize;
    unsafe { core::arch::asm!("csrrs {0}, 0xDC0, zero", out(reg) existing) };
    existing
}

// using verilator-only as a proxy for the bao1x config;
// when the flag is off, assume precursor config
#[cfg(all(feature = "vexii-test", feature = "verilator-only"))]
use crate::platform::bao1x::{
    LEGACY_INT_VMEM,
    legacy_int::{SUPER_MASK, SUPER_PENDING},
};
#[cfg(all(feature = "vexii-test", not(feature = "verilator-only")))]
use crate::platform::precursor::{
    LEGACY_INT_VMEM,
    legacy_int::{SUPER_MASK, SUPER_PENDING},
};

#[cfg(feature = "vexii-test")]
fn sim_read() -> usize {
    let legacy_int = utralib::CSR::new(LEGACY_INT_VMEM as *mut u32);
    legacy_int.r(SUPER_MASK) as usize
}

#[cfg(feature = "vexii-test")]
fn sim_write(new: usize) {
    let mut legacy_int = utralib::CSR::new(LEGACY_INT_VMEM as *mut u32);
    legacy_int.wo(SUPER_MASK, new as u32);
}

#[cfg(feature = "vexii-test")]
fn sip_read() -> usize {
    let legacy_int = utralib::CSR::new(LEGACY_INT_VMEM as *mut u32);
    legacy_int.r(SUPER_PENDING) as usize
}

/// Disable external interrupts
pub fn disable_all_irqs() {
    SIM_BACKING.store(sim_read(), Ordering::Relaxed);
    IRQ_ENABLED.store(false, Ordering::Relaxed);
    sim_write(0x0);
}

/// Enable external interrupts
#[export_name = "_enable_all_irqs"]
pub extern "C" fn enable_all_irqs() {
    IRQ_ENABLED.store(true, Ordering::Relaxed);
    sim_write(SIM_BACKING.load(Ordering::Relaxed));
}

/// Enable a given IRQ. If interrupts are currently disabled, then update the
/// SIM backing instead so that it will be enabled when interrupts are restored.
pub fn enable_irq(irq_no: usize) {
    // Note that the vexriscv "IRQ Mask" register is inverse-logic --
    // that is, setting a bit in the "mask" register unmasks (i.e. enables) it.
    if IRQ_ENABLED.load(Ordering::Relaxed) {
        sim_write(sim_read() | (1 << irq_no));
    } else {
        SIM_BACKING
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |existing| Some(existing | (1 << irq_no)))
            .ok();
    }
}

/// Disable a given IRQ. If interrupts are currently disabled, then update the
/// SIM backing instead so that it will be disabled when interrupts are restored.
pub fn disable_irq(irq_no: usize) {
    if IRQ_ENABLED.load(Ordering::Relaxed) {
        sim_write(sim_read() & !(1 << irq_no));
    } else {
        SIM_BACKING
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |existing| Some(existing & !(1 << irq_no)))
            .ok();
    }
}

/// The set of interrupts that are both pending and enabled, one bit per IRQ.
pub fn pending() -> usize { sip_read() & sim_read() }

/// The current interrupt mask, for debug output.
#[allow(dead_code)]
pub fn mask() -> usize { sim_read() }
