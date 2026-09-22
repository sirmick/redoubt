//! The seam's one real implementation: [`Transport`] over the kernel's device calls
//! (KERNEL-SPEC.md: `map_device`, `dma_alloc`, `receive` on an IRQ handle).
//!
//! **This is the only module in `blkd` with `unsafe` in it**, and it holds four blocks: a
//! volatile read and a volatile write of an MMIO register, and the same two for a byte of the DMA
//! region. Everything else in the crate is safe Rust written against [`Transport`], so the driver
//! and the server are exercised in host tests against a fake device with no `unsafe` at all
//! (`crate::fake`).
//!
//! Volatile is what makes the accesses real: the DMA region is written by a device that is not
//! this program, and an ordinary read of it could be hoisted, folded or dropped. The per-byte
//! copies are the obvious way, not the fast one (TENETS.md: clarity wins); one request's data is
//! 32 KiB against a disk round trip.
//!
//! **Bounds are checked in safe code before every access**, against lengths this module was given
//! when it was built, so no offset the driver names can reach past the mapping.

use redoubt_rt::abi::Error;
use redoubt_rt::handle::{Irq, Mmio, time_now};

use crate::queue::DMA_LEN;
use crate::transport::{Fault, Transport};
use crate::virtio::reg;

/// The bytes of a virtio-mmio transport `blkd` reads: through the two configuration words that
/// hold a block device's capacity (§4.2.2, §5.2.4).
pub const REGS_NEEDED: usize = reg::CONFIG + 8;

/// One virtio-mmio device as the kernel hands it over: its registers, its DMA region and its
/// interrupt.
pub struct Device {
    /// Where `map_device` put the registers.
    regs: usize,
    /// How many bytes of them may be touched: the length `map_device` reported (QUESTIONS.md
    /// 146), not a number this driver assumed. Every access is checked against it, and
    /// [`Device::open`] refuses a region too short for the registers virtio-mmio puts a block
    /// device's configuration in, so `blkd` never reads past the device object it was given and
    /// never has to guess how big one is.
    regs_len: usize,
    /// Where `dma_alloc` put the region, in this process's address space.
    dma: usize,
    /// The same region's physical address, which is what the device is programmed with. A driver
    /// is told the physical address of pages the kernel gave it and can never map RAM by physical
    /// address (R11).
    dma_phys: u64,
    dma_len: usize,
    irq: Irq,
    /// The handle the mapping and the DMA region came from. Owned, not borrowed, because both
    /// live as long as this `Device` does and closing the handle underneath them would leave a
    /// mapping nothing names.
    _mmio: Mmio,
}

impl Device {
    /// Maps `mmio`'s registers and allocates the driver's DMA region from it.
    ///
    /// `dma_alloc` is allowed only with an MMIO handle carrying the DMA flag, and returns
    /// physically contiguous, zeroed pages (KERNEL-SPEC.md), which is what the queue's layout
    /// assumes: one run of [`crate::queue::DMA_PAGES`] pages, and the rings starting at zero.
    /// A region shorter than [`REGS_NEEDED`] is refused here rather than faulted on later: it
    /// is not a virtio-mmio transport, whatever else it is.
    pub fn open(mmio: Mmio, irq: Irq) -> Result<Device, Error> {
        let (regs, regs_len) = mmio.map()?;
        if regs_len < REGS_NEEDED {
            return Err(Error::WrongObject);
        }
        let (dma, dma_phys) = mmio.dma_alloc(crate::queue::DMA_PAGES)?;
        Ok(Device { regs, regs_len, dma, dma_phys, dma_len: DMA_LEN, irq, _mmio: mmio })
    }

    /// The address of `off` in the register window, if a 32-bit access there is inside it and
    /// aligned. Safe: it only does arithmetic, and returns `None` rather than a bad address.
    fn reg_at(&self, off: usize) -> Option<usize> {
        if !off.is_multiple_of(4) || off.checked_add(4)? > self.regs_len {
            return None;
        }
        self.regs.checked_add(off)
    }

    /// The address of `len` bytes at `off` in the DMA region, if they are inside it.
    fn dma_at(&self, off: usize, len: usize) -> Option<usize> {
        if off.checked_add(len)? > self.dma_len {
            return None;
        }
        self.dma.checked_add(off)
    }
}

impl Transport for Device {
    fn reg_read(&self, off: usize) -> Result<u32, Fault> {
        let at = self.reg_at(off).ok_or(Fault::Bounds)?;
        // SAFETY: `at` is inside the mapping `map_device` returned, which stays mapped for the
        // life of this process (nothing here unmaps it), and `reg_at` checked that the whole
        // 32-bit access lies inside it and is 4-byte aligned. Volatile because the value is a
        // device register: it changes without this program writing it, and the read has an
        // effect on the device.
        Ok(unsafe { (at as *const u32).read_volatile() })
    }

    fn reg_write(&self, off: usize, value: u32) -> Result<(), Fault> {
        let at = self.reg_at(off).ok_or(Fault::Bounds)?;
        // SAFETY: as in `reg_read`; the mapping is writable (it is a device's registers), and
        // `reg_at` checked bounds and alignment. Volatile because the write is the effect.
        unsafe { (at as *mut u32).write_volatile(value) };
        Ok(())
    }

    fn dma_read(&self, off: usize, out: &mut [u8]) -> Result<(), Fault> {
        let at = self.dma_at(off, out.len()).ok_or(Fault::Bounds)?;
        for (i, byte) in out.iter_mut().enumerate() {
            // SAFETY: `dma_at` checked `off .. off + out.len()` against `DMA_LEN` -- this
            // crate's own constant, the pages `dma_alloc` was asked for, not a length any other
            // party reported -- and `i < out.len()`, so `at + i` is inside the region, which
            // stays mapped for the life of this process. Volatile, and a byte at a time, so no
            // `&[u8]` of memory the device is writing ever exists and no read of it can be
            // hoisted above the completion check or folded with an earlier one.
            *byte = unsafe { ((at + i) as *const u8).read_volatile() };
        }
        Ok(())
    }

    fn dma_write(&self, off: usize, src: &[u8]) -> Result<(), Fault> {
        let at = self.dma_at(off, src.len()).ok_or(Fault::Bounds)?;
        for (i, byte) in src.iter().enumerate() {
            // SAFETY: as in `dma_read`; the region is mapped read-write for this process, and
            // `dma_at` checked every byte written against `DMA_LEN`, this crate's own constant.
            unsafe { ((at + i) as *mut u8).write_volatile(*byte) };
        }
        Ok(())
    }

    fn dma_phys(&self) -> u64 { self.dma_phys }

    fn dma_len(&self) -> usize { self.dma_len }

    fn wait_irq(&self, timeout_us: u64) -> Result<(), Fault> {
        match self.irq.wait(timeout_us) {
            Ok(()) => Ok(()),
            Err(Error::Timeout) => Err(Fault::Timeout),
            Err(_) => Err(Fault::Kernel),
        }
    }

    /// A clock that fails saturates rather than reads 0: a deadline of `now + timeout` computed
    /// from 0 would never be reached, and the request timeout is what bounds a device that does
    /// not answer. `time_now` cannot fail today; this is so that it could without deleting the
    /// timeout.
    fn now_us(&self) -> u64 { time_now().unwrap_or(u64::MAX) }
}
