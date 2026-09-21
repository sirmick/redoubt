// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

//! Which process handles which interrupt.

use redoubt_abi::{MemoryAddress, PID};

use crate::arch;
use crate::cell::KernelCell;

/// Interrupts are numbered `0..MAX_IRQS`: IRQ 0 is the hart timer (TIMER.md) and 1..=1023
/// are PLIC sources (the PLIC's 10-bit source-id space, so any source fits). The arch
/// layer reports one pending interrupt at a time (`arch::intc::pending()`), so this is a
/// plain table index, not a bitmask, and is not bounded by the width of a `usize`.
const MAX_IRQS: usize = 1024;

/// A handler is a function in the owning process, plus the argument it asked for.
type Handler = (PID, MemoryAddress, Option<MemoryAddress>);

static IRQ_HANDLERS: KernelCell<[Option<Handler>; MAX_IRQS]> = KernelCell::new([None; MAX_IRQS]);

/// The handler registered for `irq`. Out-of-range numbers simply have no handler: they
/// come straight from syscall arguments, so they must never index the table.
fn handler(irq: usize) -> Option<Handler> { IRQ_HANDLERS.with(|handlers| handlers.get(irq).copied().flatten()) }

/// Dispatch the single interrupt the arch layer claimed. Redirects into the owning
/// process's handler, or masks the source if nobody owns it (an unexpected IRQ).
#[cfg(baremetal)]
pub fn handle(irq: usize) -> Result<redoubt_abi::Result, redoubt_abi::Error> {
    use crate::services::SystemServices;
    let Some((pid, f, arg)) = handler(irq) else {
        klog!("[!] Masked an unhandled IRQ #{}", irq);
        // No handler: mask the source so it cannot storm. This is an error.
        arch::irq::disable_irq(irq);
        return Ok(redoubt_abi::Result::ResumeProcess);
    };
    SystemServices::with_mut(|ss| {
        // Disable all other IRQs and redirect into userspace.
        arch::irq::disable_all_irqs();
        klog!("Making a callback to PID{}: {:x?} ({:08x}, {:x?})", pid, f, irq, arg);
        ss.make_callback_to(
            pid,
            f.get() as *mut usize,
            crate::services::CallbackType::Interrupt(
                irq,
                arg.map(|x| x.get() as *mut usize).unwrap_or(core::ptr::null_mut::<usize>()),
            ),
        )
        .map(|_| redoubt_abi::Result::ResumeProcess)
    })
}

#[allow(dead_code)] // needed to silence a hosted mode warning
pub fn for_each_irq<F>(mut op: F)
where
    F: FnMut(usize, &PID, MemoryAddress, Option<MemoryAddress>),
{
    for irq in 0..MAX_IRQS {
        if let Some((pid, f, arg)) = handler(irq) {
            op(irq, &pid, f, arg);
        }
    }
}

pub fn interrupt_claim(
    irq: usize,
    pid: PID,
    f: MemoryAddress,
    arg: Option<MemoryAddress>,
) -> Result<(), redoubt_abi::Error> {
    // A source with a device object is R5's, and a handle to it is the only authority over
    // it (WP-K3): the legacy claim is not a second one. (Both this path and the grants go
    // with WP-K6.)
    #[cfg(baremetal)]
    if crate::mem::MemoryManager::with(|mm| mm.irq_device(irq).is_some()) {
        return Err(redoubt_abi::Error::AccessDenied);
    }
    // Default deny: a process may claim only interrupts the bundle granted it.
    #[cfg(baremetal)]
    if !crate::grants::may_claim_irq(pid, irq) {
        return Err(redoubt_abi::Error::AccessDenied);
    }
    IRQ_HANDLERS.with(|handlers| {
        let slot = handlers.get_mut(irq).ok_or(redoubt_abi::Error::InterruptNotFound)?;
        if slot.is_some() {
            return Err(redoubt_abi::Error::InterruptInUse);
        }
        *slot = Some((pid, f, arg));
        Ok(())
    })?;
    arch::irq::enable_irq(irq);
    Ok(())
}

/// The process that has claimed `irq`, if any.
#[allow(dead_code)]
pub fn interrupt_owner(irq: usize) -> Option<PID> { handler(irq).map(|(pid, _, _)| pid) }

pub fn interrupt_free(irq: usize, pid: PID) -> Result<(), redoubt_abi::Error> {
    // Only the owner may free an interrupt. To everyone else it does not exist.
    if interrupt_owner(irq) != Some(pid) {
        return Err(redoubt_abi::Error::InterruptNotFound);
    }
    arch::irq::disable_irq(irq);
    IRQ_HANDLERS.with(|handlers| handlers[irq] = None);
    Ok(())
}

/// Iterate through the IRQ handlers and remove any handler that exists
/// for the given PID.
pub fn release_interrupts_for_pid(pid: PID) {
    for irq in (0..MAX_IRQS).filter(|irq| interrupt_owner(*irq) == Some(pid)) {
        arch::irq::disable_irq(irq);
        IRQ_HANDLERS.with(|handlers| handlers[irq] = None);
    }
}
