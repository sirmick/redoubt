// SPDX-License-Identifier: MIT OR Apache-2.0

//! Interrupt controller backend for standard RISC-V platforms: a PLIC.
//!
//! The loader finds the PLIC and this hart's S-mode context in the device tree and
//! reports them in the `Plic` kernel argument. The kernel maps the PLIC for itself, so
//! no userspace process can claim it.
//!
//! The PLIC's claim/complete protocol maps onto Redoubt's interrupt flow like this: the
//! trap handler calls `pending()`, which claims the highest-priority interrupt. The
//! PLIC will not raise that source again until it is completed (`complete()`), which the
//! trap handler does before it masks the source (`device.rs`, R5).

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use plic::Plic;
use redoubt_layout::KERNEL_PLIC_BASE;
use redoubt_layout::Pid;
use redoubt_sys::MemFlags;

use crate::mem::MemoryType;

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
    // The loader stores addresses as two 32-bit words; `args::wide` narrows them.
    let base = crate::args::wide(arg.data, 0);
    let size = crate::args::wide(arg.data, 2);
    CONTEXT.store(arg.data[4] as usize, Ordering::Relaxed);
    // The DMA register window (WP-K5b) follows the PLIC's mapping.
    assert!(size <= redoubt_layout::KERNEL_DMA_REGS - KERNEL_PLIC_BASE, "the PLIC runs into the DMA window");

    crate::mem::MemoryManager::with_mut(|mm| {
        mm.map_range(
            base as *mut u8,
            KERNEL_PLIC_BASE as *mut u8,
            size,
            Pid::new(1).unwrap(),
            MemFlags::READ | MemFlags::WRITE,
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

/// Complete the interrupt `pending()` claimed, if any.
pub fn complete() {
    let claimed = CLAIMED.swap(0, Ordering::Relaxed);
    if claimed != 0 {
        plic().complete(context(), claimed);
    }
}

/// Claim the highest-priority pending interrupt. Returned as a bitmask with at most one
/// bit set, which is what the generic IRQ dispatcher expects.
pub fn pending() -> Option<usize> {
    plic().claim(context()).map(|irq| {
        CLAIMED.store(irq.get(), Ordering::Relaxed);
        irq.get() as usize
    })
}
