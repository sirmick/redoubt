// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

//! Which process handles which interrupt.

use xous_kernel::{MemoryAddress, PID};

use crate::arch;
use crate::cell::KernelCell;

/// Interrupts are numbered `0..MAX_IRQS`. The arch layer reports pending interrupts as a
/// bitmask, so this cannot exceed the width of a `usize`.
const MAX_IRQS: usize = 32;

/// A handler is a function in the owning process, plus the argument it asked for.
type Handler = (PID, MemoryAddress, Option<MemoryAddress>);

static IRQ_HANDLERS: KernelCell<[Option<Handler>; MAX_IRQS]> = KernelCell::new([None; MAX_IRQS]);

/// The handler registered for `irq`. Out-of-range numbers simply have no handler: they
/// come straight from syscall arguments, so they must never index the table.
fn handler(irq: usize) -> Option<Handler> { IRQ_HANDLERS.with(|handlers| handlers.get(irq).copied().flatten()) }

#[cfg(baremetal)]
pub fn handle(irqs_pending: usize) -> Result<xous_kernel::Result, xous_kernel::Error> {
    use crate::services::SystemServices;
    for irq_no in (0..MAX_IRQS).filter(|irq_no| irqs_pending & (1 << irq_no) != 0) {
        let Some((pid, f, arg)) = handler(irq_no) else {
            klog!("[!] Masked an unhandled IRQ #{:?}", irq_no);
            // If there is no handler, mask this interrupt to prevent an IRQ storm.
            // This is considered an error.
            arch::irq::disable_irq(irq_no);
            continue;
        };
        return SystemServices::with_mut(|ss| {
            // Disable all other IRQs and redirect into userspace
            arch::irq::disable_all_irqs();
            klog!("Making a callback to PID{}: {:x?} ({:08x}, {:x?})", pid, f, irq_no as usize, arg);
            ss.make_callback_to(
                pid,
                f.get() as *mut usize,
                crate::services::CallbackType::Interrupt(
                    irq_no,
                    arg.map(|x| x.get() as *mut usize).unwrap_or(core::ptr::null_mut::<usize>()),
                ),
            )
            .map(|_| xous_kernel::Result::ResumeProcess)
        });
    }
    Ok(xous_kernel::Result::ResumeProcess)
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
) -> Result<(), xous_kernel::Error> {
    IRQ_HANDLERS.with(|handlers| {
        let slot = handlers.get_mut(irq).ok_or(xous_kernel::Error::InterruptNotFound)?;
        if slot.is_some() {
            return Err(xous_kernel::Error::InterruptInUse);
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

pub fn interrupt_free(irq: usize, pid: PID) -> Result<(), xous_kernel::Error> {
    // Only the owner may free an interrupt. To everyone else it does not exist.
    if interrupt_owner(irq) != Some(pid) {
        return Err(xous_kernel::Error::InterruptNotFound);
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
