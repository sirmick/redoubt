// SPDX-License-Identifier: MIT OR Apache-2.0

//! Interrupt controller backend for standard RISC-V platforms: a PLIC.
//!
//! The loader finds the PLIC and this hart's S-mode context in the device tree and
//! reports them in the `Plic` kernel argument. The kernel maps the PLIC for itself, so
//! no userspace process can claim it.
//!
//! The PLIC's claim/complete protocol maps onto Xous's interrupt flow like this: the
//! trap handler calls `pending()`, which claims the highest-priority interrupt. The
//! PLIC will not raise that source again until it is completed, which happens in
//! `enable_all_irqs()` once the userspace handler has returned. While a handler runs,
//! external interrupts are masked at the hart (`sie.SEIE`), because S-mode interrupts
//! cannot otherwise be held off while the hart is in U-mode.

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use plic::Plic;
use riscv::register::sie;
use xous_kernel::arch::KERNEL_PLIC_BASE;
use xous_kernel::{MemoryFlags, MemoryType, PID};

/// Sources are enabled at this priority. The context threshold is 0, so any non-zero
/// priority is delivered.
const PRIORITY: u32 = 1;

static CONTEXT: AtomicUsize = AtomicUsize::new(0);
/// The interrupt claimed by `pending()` and not yet completed, or 0.
static CLAIMED: AtomicU32 = AtomicU32::new(0);

fn plic() -> &'static Plic {
    // SAFETY: `init` maps the PLIC's registers at `KERNEL_PLIC_BASE` for the kernel alone,
    // and the mapping is never removed. `Plic` is a register block that is only accessed
    // through volatile reads and writes, so a shared reference to it is sound.
    unsafe { &*(KERNEL_PLIC_BASE as *const Plic) }
}

fn context() -> usize { CONTEXT.load(Ordering::Relaxed) }

/// Map the PLIC described by the `Plic` kernel argument, if there is one.
pub fn init() {
    let Some(arg) = crate::args::KernelArguments::get().iter().find(|a| a.name == u32::from_le_bytes(*b"Plic"))
    else {
        println!("No PLIC reported by the loader; external interrupts are unavailable");
        return;
    };
    // The loader stores addresses as two 32-bit words. Rebuild as u64 (avoiding a `<< 32`
    // that would overflow a 32-bit usize) and narrow; a PLIC's MMIO fits in 32 bits on rv32.
    let base = (arg.data[0] as u64 | (arg.data[1] as u64) << 32) as usize;
    let size = (arg.data[2] as u64 | (arg.data[3] as u64) << 32) as usize;
    CONTEXT.store(arg.data[4] as usize, Ordering::Relaxed);

    crate::mem::MemoryManager::with_mut(|mm| {
        mm.map_range(
            base as *mut u8,
            KERNEL_PLIC_BASE as *mut u8,
            size,
            PID::new(1).unwrap(),
            MemoryFlags::R | MemoryFlags::W,
            MemoryType::Default,
        )
        .expect("unable to map the PLIC")
    });
    plic().set_threshold(context(), 0);
}

pub fn enable_irq(irq_no: usize) {
    plic().set_priority(irq_no as u32, PRIORITY);
    plic().enable(irq_no as u32, context());
}

pub fn disable_irq(irq_no: usize) { plic().disable(irq_no as u32, context()); }

pub fn disable_all_irqs() {
    // SAFETY: masking an interrupt source cannot violate memory safety.
    unsafe { sie::clear_sext() };
}

pub fn enable_all_irqs() {
    let claimed = CLAIMED.swap(0, Ordering::Relaxed);
    if claimed != 0 {
        plic().complete(context(), claimed);
    }
    // SAFETY: the kernel itself runs with `sstatus.SIE` clear, so unmasking the source only
    // takes effect in U-mode, where the trap handler is ready for it.
    unsafe { sie::set_sext() };
}

/// Claim the highest-priority pending interrupt. Returned as a bitmask with at most one
/// bit set, which is what the generic IRQ dispatcher expects.
pub fn pending() -> usize {
    match plic().claim(context()) {
        Some(irq) => {
            CLAIMED.store(irq.get(), Ordering::Relaxed);
            1 << irq.get()
        }
        None => 0,
    }
}

/// For debug output: 1 if external interrupts are unmasked at the hart.
#[allow(dead_code)]
pub fn mask() -> usize { sie::read().sext() as usize }
