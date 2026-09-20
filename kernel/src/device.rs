// SPDX-License-Identifier: MIT OR Apache-2.0

//! Device objects (KERNEL-SPEC.md, Device; R5, R11) and the calls that use them:
//! `map_device`, `dma_alloc` and `system_reset`.
//!
//! A device object is one of three things: an **MMIO** range with a DMA flag, an **IRQ** with
//! its `fired` and `masked` flags, or the **Reset** right. The loader reads the machine's
//! device tree and describes each one in the argument block's `Devs` tag (BOOT.md); the kernel
//! turns each entry into an object at boot. Nothing else ever creates one, so the set is fixed
//! by the machine and a process reaches a device only through a handle it was given
//! (DEVICE-GRANTS.md, Replacement: this is what replaces the interim grants).
//!
//! # Where a device lives
//! One RAM frame of its own, allocated to `mem::OBJECT_OWNER`, exactly as a budget
//! (`budget.rs`) or an endpoint (`endpoint.rs`) is: that frame *is* the page the cost table
//! charges. The cost table does not list a device (it predates this package); a device is
//! charged like every other object, **one page to the budget that owns it**, which at boot is
//! the budget the loader's programs run in.
//!
//! # What the kernel keeps out
//! Two ranges never become device objects, because giving one away would give away everything
//! else: the interrupt controller (a process that could program the PLIC would own every
//! source) and any part of RAM (R11: userspace never names RAM by physical address). The
//! loader leaves the controller out; the kernel refuses to boot on a `Devs` entry that names
//! RAM or wraps the address space, so a hostile device tree cannot smuggle RAM in as a device.

use core::convert::TryFrom;

use redoubt_sys::{Error, PAGE_SIZE, ResetKind};
use xous_kernel::PID;

use crate::budget::BudgetFrame;
use crate::handle::{BudgetRef, DeviceRef, Handle, Object};
use crate::kframe;
use crate::mem::MemoryManager;

/// The cost table (KERNEL-SPEC.md, What objects cost), in pages.
pub const DEVICE_PAGES: u64 = 1;

/// First word of every device frame, so that a frame read as a device that is not one is
/// caught. Distinct from every other object's magic.
const MAGIC: u64 = u64::from_le_bytes(*b"device\0\0");

/// Which of the three forms a device object takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Mmio = 1,
    Irq = 2,
    Reset = 3,
}

/// A device as the kernel works with it; it lives in its frame as words.
#[derive(Clone, Copy)]
pub struct Device {
    pub id: u64,
    /// The budget it is charged to, as an endpoint's owner is. Destroying that budget
    /// destroys the device (R10).
    pub owner: BudgetRef,
    pub kind: Kind,
    /// MMIO: the physical range, page-aligned and a whole number of pages.
    pub base: u64,
    pub size: u64,
    /// MMIO: the device is a bus master, so `dma_alloc` is allowed through it.
    pub dma: bool,
    /// IRQ: the interrupt number.
    pub irq: u32,
    /// IRQ: it fired and no `receive` has taken it yet (R5).
    pub fired: bool,
    /// IRQ: the source is masked at the interrupt controller (R5).
    pub masked: bool,
}

// Word layout in the frame (a frame holds 512).
const W_ID: usize = 1;
const W_OWNER: usize = 2; // frame + 1
const W_OWNER_ID: usize = 3;
const W_KIND: usize = 4;
const W_BASE: usize = 5;
const W_SIZE: usize = 6;
const W_DMA: usize = 7;
const W_IRQ: usize = 8;
const W_FIRED: usize = 9;
const W_MASKED: usize = 10;
const WORDS: usize = 11;
const _: () = assert!(WORDS * 8 <= PAGE_SIZE);

impl MemoryManager {
    pub fn device(&self, frame: u32) -> Device {
        let phys = self.object_phys(frame);
        let w = |i: usize| kframe::read(phys, i * 8);
        // As in `budget.rs`: a frame that does not hold a device means a stale reference
        // survived R10's sweep, a violated invariant (I1), so the kernel stops.
        assert!(w(0) == MAGIC, "I1: frame {} holds no device", frame);
        let kind = match w(W_KIND) {
            1 => Kind::Mmio,
            2 => Kind::Irq,
            3 => Kind::Reset,
            _ => panic!("I1: device frame {} is corrupt", frame),
        };
        Device {
            id: w(W_ID),
            owner: BudgetRef { frame: (w(W_OWNER) as u32).wrapping_sub(1), id: w(W_OWNER_ID) },
            kind,
            base: w(W_BASE),
            size: w(W_SIZE),
            dma: w(W_DMA) != 0,
            irq: w(W_IRQ) as u32,
            fired: w(W_FIRED) != 0,
            masked: w(W_MASKED) != 0,
        }
    }

    pub fn store_device(&mut self, frame: u32, d: &Device) {
        let phys = self.object_phys(frame);
        let mut words = [0u64; WORDS];
        words[0] = MAGIC;
        words[W_ID] = d.id;
        words[W_OWNER] = u64::from(d.owner.frame) + 1;
        words[W_OWNER_ID] = d.owner.id;
        words[W_KIND] = d.kind as u64;
        words[W_BASE] = d.base;
        words[W_SIZE] = d.size;
        words[W_DMA] = u64::from(d.dma);
        words[W_IRQ] = u64::from(d.irq);
        words[W_FIRED] = u64::from(d.fired);
        words[W_MASKED] = u64::from(d.masked);
        for (i, word) in words.iter().enumerate() {
            kframe::write(phys, i * 8, *word);
        }
    }

    /// Whether `frame` holds a device. Used by R10's sweep, which scans the object frames.
    pub fn is_device_frame(&self, frame: u32) -> bool {
        self.is_object_frame(frame) && kframe::read(self.object_phys(frame), 0) == MAGIC
    }

    /// Whether `r` still names the device it named (a handle in a message R10 may have
    /// revoked meanwhile).
    pub fn is_live_device(&self, r: DeviceRef) -> bool {
        self.is_device_frame(r.frame) && self.device(r.frame).id == r.id
    }

    /// The device `r` names, which must still be the one it named (I1).
    pub fn device_at(&self, r: DeviceRef) -> Device {
        let d = self.device(r.frame);
        assert!(d.id == r.id, "I1: a handle names a device that is gone");
        d
    }

    /// The device `pid`'s handle `index` names: `BadHandle`, then `WrongObject`.
    pub fn device_handle(&self, pid: PID, index: u32) -> Result<(DeviceRef, Device), Error> {
        match self.handle(pid, index)?.object {
            Object::Device(d) => Ok((d, self.device_at(d))),
            _ => Err(Error::WrongObject),
        }
    }

    /// The device `pid`'s handle `index` names, which must be of `kind`.
    fn device_of_kind(&self, pid: PID, index: u32, kind: Kind) -> Result<Device, Error> {
        let (_, d) = self.device_handle(pid, index)?;
        if d.kind != kind { Err(Error::WrongObject) } else { Ok(d) }
    }

    /// The frame of the IRQ object for interrupt `irq`, if the machine has one. The scan is
    /// over the object frames, as R10's sweeps are; it runs once per interrupt.
    pub fn irq_device(&self, irq: usize) -> Option<u32> {
        let irq = u32::try_from(irq).ok()?;
        (0..=self.objects.high_frame)
            .find(|f| {
                self.is_device_frame(*f) && {
                    let d = self.device(*f);
                    d.kind == Kind::Irq && d.irq == irq
                }
            })
    }

    /// Create one device object, charged to `owner` (the cost table).
    fn new_device(&mut self, owner: BudgetFrame, d: &Device) -> Result<DeviceRef, Error> {
        self.charge(owner, DEVICE_PAGES)?;
        let frame = self.alloc_object_frame().inspect_err(|_| self.uncharge(owner, DEVICE_PAGES))?;
        let id = self.next_object_id();
        let owner = BudgetRef { frame: owner, id: self.budget(owner).id };
        self.store_device(frame, &Device { id, owner, ..*d });
        Ok(DeviceRef { frame, id })
    }

    /// Free a device object and give its page back to its owner.
    pub fn free_device(&mut self, frame: u32) {
        let owner = self.device(frame).owner;
        self.free_object_frame(frame);
        if self.is_live_budget(owner) {
            self.uncharge(owner.frame, DEVICE_PAGES);
        }
    }

    /// Create every device object the loader described, charged to `owner`, and put a handle
    /// to each in `first`'s table, in the `Devs` tag's order (BOOT.md).
    ///
    /// INTERIM (WP-K3; WP-R3 removes it): KERNEL-SPEC.md says `init` receives them all, and
    /// there is no `init` yet, so they go to the bundle's first program exactly as WP-K2's
    /// `boot_endpoint` gives out the one endpoint. The order is the loader's, which puts the
    /// Reset right first and the console and its interrupt next, so a test program can name
    /// one without a manifest.
    pub fn boot_devices(&mut self, owner: BudgetFrame, first: Option<PID>, stamp: BudgetRef) {
        let Some(tag) =
            crate::args::KernelArguments::get().iter().find(|a| a.name == u32::from_le_bytes(*b"Devs"))
        else {
            println!("Devices: the loader reported none");
            return;
        };
        assert!(tag.data.len() % ENTRY_WORDS == 0, "Devs is not a whole number of entries");
        let (mut mmio, mut irqs) = (0, 0);
        for words in tag.data.chunks_exact(ENTRY_WORDS) {
            let d = self.decode_entry(words);
            match d.kind {
                Kind::Mmio => mmio += 1,
                Kind::Irq => irqs += 1,
                Kind::Reset => {}
            }
            let device = self.new_device(owner, &d).expect("boot: no room for a device object");
            if let Some(pid) = first {
                let handle = Handle { object: Object::Device(device), badge: 0, stamp };
                self.install_handle(pid, handle).expect("boot: no room for a device handle");
            }
        }
        println!("Devices: {} mmio, {} irq, 1 reset, all held by the first program (INTERIM)", mmio, irqs);
    }

    /// One `Devs` entry: kind, two 64-bit values (low word first), a flag word (BOOT.md).
    /// Every check here is fail-closed: a malformed entry stops the boot rather than becoming
    /// an object that names something it must not.
    fn decode_entry(&self, words: &[u32]) -> Device {
        let value = |i: usize| u64::from(words[i]) | u64::from(words[i + 1]) << 32;
        let (base, size, flags) = (value(1), value(3), words[5]);
        let none = BudgetRef { frame: 0, id: 0 };
        let mut d = Device {
            id: 0,
            owner: none,
            kind: Kind::Reset,
            base: 0,
            size: 0,
            dma: false,
            irq: 0,
            fired: false,
            masked: false,
        };
        match words[0] {
            1 => {
                let end = base.checked_add(size).expect("Devs: an MMIO region wraps");
                assert!(size != 0, "Devs: an empty MMIO region");
                assert!(
                    base % PAGE_SIZE as u64 == 0 && size % PAGE_SIZE as u64 == 0,
                    "Devs: an MMIO region is not whole pages"
                );
                // R11: userspace never names RAM by physical address, so a device object
                // never does either. Refusing here means no `map_device` has to check.
                assert!(!self.overlaps_ram(base, end), "Devs: an MMIO region overlaps RAM");
                assert!(usize::try_from(end).is_ok(), "Devs: an MMIO region does not fit a usize");
                d.kind = Kind::Mmio;
                d.base = base;
                d.size = size;
                d.dma = flags & 1 != 0;
            }
            2 => {
                let irq = u32::try_from(base).expect("Devs: an interrupt number too wide");
                // The timer is a hart resource, not a device (BOOT.md): it is not a PLIC
                // source, and until WP-K5 the legacy path still delivers it as IRQ 0.
                assert!(irq != 0, "Devs: interrupt 0 is the hart timer, not a device");
                d.kind = Kind::Irq;
                d.irq = irq;
                // Masked until someone receives on it (R5), so an unheld source cannot storm.
                d.masked = true;
            }
            3 => d.kind = Kind::Reset,
            other => panic!("Devs: unknown device kind {}", other),
        }
        d
    }
}

/// Words in one `Devs` entry.
const ENTRY_WORDS: usize = 6;

impl MemoryManager {
    /// `map_device(h(MMIO)) -> addr` (KERNEL-SPEC.md). The whole range, at an address the
    /// kernel chooses (R11), readable and writable and never executable.
    ///
    /// The MMIO page-ownership table is not touched. A device handle may be copied like any
    /// other, so two holders may both map the device; the handle, not a page owner, is the
    /// authority. Nothing is charged for the pages themselves -- they are not RAM -- only for
    /// the page tables that map them, which `alloc_page` charges as it takes them.
    pub fn map_device(&mut self, pid: PID, h: u32) -> Result<usize, Error> {
        let d = self.device_of_kind(pid, h, Kind::Mmio)?;
        let (base, size) = (d.base as usize, d.size as usize);
        let at = self
            .find_virtual_address(core::ptr::null_mut(), size, xous_kernel::MemoryType::Default)
            .map_err(|_| Error::OutOfMemory)? as usize;
        let flags = xous_kernel::MemoryFlags::R | xous_kernel::MemoryFlags::W;
        for offset in (0..size).step_by(PAGE_SIZE) {
            if crate::arch::mem::map_page_inner(self, pid, base + offset, at + offset, flags, true).is_err() {
                for undo in (0..offset).step_by(PAGE_SIZE) {
                    crate::arch::mem::unmap_page_inner(self, at + undo).ok();
                }
                return Err(Error::OutOfMemory);
            }
        }
        Ok(at)
    }

    /// `dma_alloc(h(MMIO), npages) -> addr, phys` (KERNEL-SPEC.md; IO-ARCHITECTURE.md, DMA):
    /// contiguous, zeroed RAM the device may be programmed with. **The one call that returns a
    /// physical address**, and only for a device the platform says is a bus master.
    pub fn dma_alloc(&mut self, pid: PID, h: u32, npages: usize) -> Result<(usize, u64), Error> {
        let d = self.device_of_kind(pid, h, Kind::Mmio)?;
        if npages == 0 {
            return Err(Error::InvalidArgument);
        }
        if !d.dma {
            return Err(Error::NotPermitted);
        }
        let len = npages.checked_mul(PAGE_SIZE).ok_or(Error::OutOfMemory)?;
        let at = self
            .find_virtual_address(core::ptr::null_mut(), len, xous_kernel::MemoryType::Default)
            .map_err(|_| Error::OutOfMemory)? as usize;
        // Charged and zeroed before anything is mapped (R6, R11).
        let phys = self.alloc_contiguous(pid, npages)?;
        let flags = xous_kernel::MemoryFlags::R | xous_kernel::MemoryFlags::W;
        for i in 0..npages {
            let offset = i * PAGE_SIZE;
            if crate::arch::mem::map_page_inner(self, pid, phys + offset, at + offset, flags, true).is_err() {
                for undo in (0..offset).step_by(PAGE_SIZE) {
                    crate::arch::mem::unmap_page_inner(self, at + undo).ok();
                }
                self.free_frames(pid, phys, npages);
                return Err(Error::OutOfMemory);
            }
        }
        Ok((at, phys as u64))
    }

    /// `system_reset(h(Reset), kind)`: power off or reboot. Returns only on a refusal.
    pub fn system_reset(&self, pid: PID, h: u32, kind: ResetKind) -> Result<(), Error> {
        self.device_of_kind(pid, h, Kind::Reset)?;
        println!("system_reset: {:?} asked for by PID {}", kind, pid.get());
        crate::platform::reset(kind == ResetKind::Reboot)
    }
}

/// Whether a device object owns `irq`, so the trap handler knows whether to take R5's path or
/// the legacy handler table's (until WP-K6 deletes that). It only looks.
#[cfg(baremetal)]
pub fn irq_wanted(irq: usize) -> bool { MemoryManager::with(|mm| mm.irq_device(irq).is_some()) }

/// R5: interrupt `irq` fired. The kernel masks the source and sets `fired`; a thread already
/// waiting in `receive` on the handle is answered at once (which clears `fired` again).
///
/// The caller has already completed the interrupt controller's claim (`arch/riscv/irq.rs`).
#[cfg(baremetal)]
pub fn irq_fired(irq: usize) {
    crate::services::SystemServices::with_mut(|ss| {
        MemoryManager::with_mut(|mm| {
            let Some(frame) = mm.irq_device(irq) else { return };
            let mut d = mm.device(frame);
            d.fired = true;
            d.masked = true;
            mm.store_device(frame, &d);
            crate::arch::irq::disable_irq(irq);
            crate::message::irq_ready(ss, mm, frame);
        })
    })
}
