// SPDX-License-Identifier: MIT OR Apache-2.0

//! Device grants: which process may claim which MMIO region and interrupt.
//!
//! A userspace process may map a device page or claim an interrupt only if the boot
//! bundle's manifest granted it that resource (default deny). The loader delivers grants
//! as `Grnt` argument tags, one per granted process; this module reads them on demand at
//! a claim, the way `process_name` reads `PNam`. See `docs/DEVICE-GRANTS.md`.

use redoubt_abi::PID;

use crate::args::{KernelArgument, KernelArguments};
use crate::mem::MemoryManager;

/// The kernel (PID 1) owns the machine and needs no grant.
const KERNEL_PID: u8 = 1;

fn grant_tags() -> impl Iterator<Item = KernelArgument> {
    KernelArguments::get().iter().filter(|arg| arg.name == u32::from_le_bytes(*b"Grnt"))
}

/// A `Grnt` tag for `pid`, if one exists. Layout: `[pid, n_mmio, n_irq, mmio.., irq..]`,
/// each MMIO region four words (base_lo, base_hi, len_lo, len_hi).
///
/// A grant names a PID the loader gave one of its programs, so it is that program's only: a
/// process with a process object was created later (`process_create`), perhaps with the PID of a
/// program that had exited, and inherits nothing from it.
fn grant_for(mm: &MemoryManager, pid: PID) -> Option<KernelArgument> {
    if crate::process::object_of(mm, pid).is_some() {
        return None;
    }
    grant_tags().find(|arg| arg.data.first() == Some(&(pid.get() as u32)))
}

fn u64_at(data: &[u32], i: usize) -> u64 { data[i] as u64 | (data[i + 1] as u64) << 32 }

/// May `pid` map the device region `[base, base + len)`? True if a single granted region
/// contains it. (A device claim must fall entirely within one grant, not straddle two.)
pub fn may_map_device(mm: &MemoryManager, pid: PID, base: usize, len: usize) -> bool {
    if pid.get() == KERNEL_PID {
        return true;
    }
    let Some(end) = base.checked_add(len) else { return false };
    let Some(grant) = grant_for(mm, pid) else { return false };
    let n_mmio = grant.data[1] as usize;
    (0..n_mmio).any(|i| {
        let region = &grant.data[3 + i * 4..];
        let (g_base, g_len) = (u64_at(region, 0) as usize, u64_at(region, 2) as usize);
        base >= g_base && (end as u64) <= (g_base as u64 + g_len as u64)
    })
}

/// May `pid` claim interrupt `irq`?
pub fn may_claim_irq(mm: &MemoryManager, pid: PID, irq: usize) -> bool {
    if pid.get() == KERNEL_PID {
        return true;
    }
    let Some(grant) = grant_for(mm, pid) else { return false };
    let (n_mmio, n_irq) = (grant.data[1] as usize, grant.data[2] as usize);
    let irqs = &grant.data[3 + n_mmio * 4..][..n_irq];
    irqs.contains(&(irq as u32))
}
