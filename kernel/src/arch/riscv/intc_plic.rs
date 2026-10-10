// SPDX-License-Identifier: MIT OR Apache-2.0

//! Interrupt controller backend for standard RISC-V platforms: a PLIC.
//!
//! The loader finds the PLIC in the device tree and reports it in the `Plic` kernel argument, and
//! each started hart's S-mode context in `Hart`. The kernel maps the PLIC for itself, so no
//! userspace process can claim it.
//!
//! Every started hart takes device interrupts: each enables every source the loader's `Devs`
//! names on its own context, threshold 0, when it comes online (`online`). A source is masked and
//! unmasked by its priority, one write that reaches every context, never by its enable bits
//! (kernel/boot.md, "The interrupt controller contract").
//!
//! The PLIC's claim/complete protocol maps onto Redoubt's interrupt flow like this: the
//! trap handler calls `pending()`, which claims the highest-priority interrupt on this hart's
//! context. The PLIC will not raise that source again until it is completed (`complete()`), which
//! the trap handler does in the same kernel section, before it masks the source (`device.rs`,
//! R5). A hart that trapped for a source another hart claimed first claims nothing.

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use plic::Plic;
use redoubt_layout::KERNEL_PLIC_BASE;
use redoubt_layout::Pid;
use redoubt_sys::MemFlags;

use crate::arch::hart::{self, MAX_HARTS};
use crate::mem::MemoryType;

/// An unmasked source's priority. Every context's threshold is 0, so it is delivered.
const PRIORITY: u32 = 1;

/// Each started hart's S-mode context, by boot index.
static CONTEXTS: [AtomicUsize; MAX_HARTS] = [const { AtomicUsize::new(0) }; MAX_HARTS];
/// The interrupt claimed by `pending()` and not yet completed, or 0. One word for every hart: a
/// claim and its completion are in one kernel section, under the kernel lock.
static CLAIMED: AtomicU32 = AtomicU32::new(0);

fn plic() -> &'static Plic {
    // SAFETY: `init` maps the PLIC's registers at `KERNEL_PLIC_BASE` for the kernel alone,
    // and the mapping is never removed. `Plic` is a register block that is only accessed
    // through volatile reads and writes, so a shared reference to it is sound.
    unsafe { &*(KERNEL_PLIC_BASE as *const Plic) }
}

/// This hart's S-mode context.
fn context() -> usize { CONTEXTS[hart::index()].load(Ordering::Relaxed) }

/// Map the PLIC described by the `Plic` kernel argument, if there is one, mask every source the
/// loader's `Devs` names, and bring the boot hart's context online.
pub fn init() {
    let Some(arg) =
        crate::args::KernelArguments::get().iter().find(|a| a.name == u32::from_le_bytes(*b"Plic"))
    else {
        println!("No PLIC reported by the loader; external interrupts are unavailable");
        return;
    };
    // The loader stores addresses as two 32-bit words; `args::wide` narrows them.
    let base = crate::args::wide(arg.data, 0);
    let size = crate::args::wide(arg.data, 2);
    CONTEXTS[0].store(arg.data[4] as usize, Ordering::Relaxed);
    // The other harts' from `Hart`, by boot index; the boot hart's there is the same.
    for (i, context) in hart::contexts().enumerate() {
        assert!(
            i > 0 || context == arg.data[4] as usize,
            "Hart and Plic disagree on the boot hart's context"
        );
        CONTEXTS[i].store(context, Ordering::Relaxed);
    }
    // The DMA register window follows the PLIC's mapping.
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
    // Masked until someone receives on it (R5): a priority's reset value is the PLIC's to choose.
    for irq in crate::device::irq_sources() {
        disable_irq(irq);
    }
    online();
}

/// This hart takes device interrupts from now on: every source the loader's `Devs` names enabled
/// on its context, threshold 0. Each source is still masked by its priority until a `receive`
/// unmasks it. The boot hart at `init`; every other hart once it first holds the kernel lock
/// (`hart::hart_main`). It writes only this hart's context.
pub fn online() {
    let context = context();
    for irq in crate::device::irq_sources() {
        plic().enable(irq as u32, context);
    }
    plic().set_threshold(context, 0);
}

/// Unmask a source: its priority, which every context sees. A priority write is also what makes
/// QEMU's PLIC look at its sources again (it re-evaluates its output when a priority, a pending
/// bit or a claim changes, not when an enable bit does), so a source that became pending while
/// masked is delivered at its unmask. This is the only unmask, so every re-arm after a claim
/// (`receive`'s) goes through it.
pub fn enable_irq(irq_no: usize) { plic().set_priority(irq_no as u32, PRIORITY); }

/// Mask a source: priority 0, which the PLIC never delivers, on any context. Its enable bits stay,
/// so a completion for it is never ignored (a PLIC ignores one for a source not enabled on the
/// completing context).
pub fn disable_irq(irq_no: usize) { plic().set_priority(irq_no as u32, 0); }

/// Complete the interrupt `pending()` claimed, if any, on this hart's context.
pub fn complete() {
    let claimed = CLAIMED.swap(0, Ordering::Relaxed);
    if claimed != 0 {
        plic().complete(context(), claimed);
    }
}

/// Claim the highest-priority pending interrupt on this hart's context: its source number, or
/// `None` when nothing is pending there (another hart claimed it first, or it was masked).
pub fn pending() -> Option<usize> {
    #[cfg(debug_assertions)]
    assert_eq!(CLAIMED.load(Ordering::Relaxed), 0, "a claim before the last one was completed");
    let claimed = plic().claim(context()).map(|irq| {
        CLAIMED.store(irq.get(), Ordering::Relaxed);
        irq.get() as usize
    });
    #[cfg(debug_assertions)]
    match claimed {
        Some(_) => CLAIMS[hart::index()].fetch_add(1, Ordering::Relaxed),
        None => EMPTY[hart::index()].fetch_add(1, Ordering::Relaxed),
    };
    claimed
}

/// A checked build's count, by boot index, of the sources each hart claimed and of its claims that
/// found nothing, another hart's context having claimed the source first ([`report`]).
#[cfg(debug_assertions)]
static CLAIMS: [AtomicU32; MAX_HARTS] = [const { AtomicU32::new(0) }; MAX_HARTS];
#[cfg(debug_assertions)]
static EMPTY: [AtomicU32; MAX_HARTS] = [const { AtomicU32::new(0) }; MAX_HARTS];

/// A checked build's account at `system_reset` (`hart::report`), the started harts only: each
/// hart's claims, its claims that found nothing, and their sum, the interrupts its context took
/// whoever claimed them.
#[cfg(debug_assertions)]
pub fn report() {
    let n = hart::started();
    let claimed = CLAIMS.each_ref().map(|count| count.load(Ordering::Relaxed));
    let empty = EMPTY.each_ref().map(|count| count.load(Ordering::Relaxed));
    let took: [u32; MAX_HARTS] = core::array::from_fn(|i| claimed[i] + empty[i]);
    println!(
        "external interrupts: claimed by hart {:?}, found nothing by hart {:?}, taken by hart {:?}",
        &claimed[..n],
        &empty[..n],
        &took[..n]
    );
}
