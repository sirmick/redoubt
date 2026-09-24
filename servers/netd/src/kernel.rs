//! The seam's one real implementation, [`Transport`] over the kernel's device calls
//! (KERNEL-SPEC.md: `map_device`, `dma_alloc`, `receive` on an IRQ handle), and the hand-over of
//! the receive thread's half to that thread.
//!
//! **This is the only module in `netd` with `unsafe` in it**:
//! - volatile reads and writes of a register (32-bit and, for the MAC, 8-bit), which the panic hook also uses
//!   to reset the device;
//! - volatile reads and writes of a byte of a DMA region;
//! - the one `Box::from_raw` that takes the receive thread's half back out of [`RX_PART`].
//!
//! Everything else in the crate is safe Rust written against [`Transport`], run in host tests
//! against a hostile fake device with no `unsafe` at all. Bounds are checked in safe code before
//! every access, against lengths this module was given when it was built.

use alloc::boxed::Box;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use redoubt_rt::abi::Error;
use redoubt_rt::handle::{Irq, Mmio, time_now};

use crate::RxPart;
use crate::ring::{REGION_LEN, REGION_PAGES};
use crate::transport::{Fault, Transport};
use crate::virtio::reg;

/// The bytes of a virtio-mmio slot `netd` touches: through the MAC in the configuration space.
pub const REGS_NEEDED: usize = reg::CONFIG + 8;

/// A device's registers as `map_device` mapped them: address and length. Copied into both
/// threads' views; the mapping stays for the life of the process.
#[derive(Clone, Copy, Debug)]
pub struct Regs {
    base: usize,
    len: usize,
}

impl Regs {
    /// Maps `mmio`'s registers, refusing a region too short to be a virtio-net transport.
    pub fn map(mmio: &Mmio) -> Result<Regs, Error> {
        let (base, len) = mmio.map()?;
        if len < REGS_NEEDED {
            return Err(Error::WrongObject);
        }
        Ok(Regs { base, len })
    }

    /// Host tests only: "registers" in ordinary memory, given up for good as a device's mapping
    /// is, so the panic hook can be run on the host ([`Regs::arm_panic_reset`]). Nothing on the
    /// machine can build a `Regs` but [`Regs::map`].
    #[cfg(not(target_os = "none"))]
    pub fn in_memory(words: &'static mut [u32]) -> Regs {
        Regs { base: words.as_mut_ptr() as usize, len: core::mem::size_of_val(words) }
    }

    /// Host tests only: reads the register at `off`, as the hook left it.
    #[cfg(not(target_os = "none"))]
    pub fn read_register(&self, off: usize) -> Result<u32, Fault> { self.read(off) }

    /// The address of a `width`-byte access at `off`, if it is inside the mapping and aligned.
    fn at(&self, off: usize, width: usize) -> Option<usize> {
        if !off.is_multiple_of(width) || off.checked_add(width)? > self.len {
            return None;
        }
        self.base.checked_add(off)
    }

    fn read(&self, off: usize) -> Result<u32, Fault> {
        let at = self.at(off, 4).ok_or(Fault::Bounds)?;
        // SAFETY: `at` is inside the mapping `map_device` returned (a `Regs` is only ever made
        // from one, by `Regs::map`, or copied from one), which stays mapped for the life of this
        // process (nothing here unmaps it), and `Regs::at` checked the whole 32-bit access lies
        // inside it and is 4-byte aligned. Volatile: the value is a device register. (In host
        // tests, `Regs::in_memory` stands a leaked `&'static mut [u32]` in for the mapping: it
        // too lives for ever, is aligned, and is reached only through these accesses.)
        Ok(unsafe { (at as *const u32).read_volatile() })
    }

    fn read_u8(&self, off: usize) -> Result<u8, Fault> {
        let at = self.at(off, 1).ok_or(Fault::Bounds)?;
        // SAFETY: as in `read`, for one byte, which needs no alignment. The configuration space
        // asks for byte accesses to byte fields (virtio 1.2, §4.2.2.2).
        Ok(unsafe { (at as *const u8).read_volatile() })
    }

    fn write(&self, off: usize, value: u32) -> Result<(), Fault> {
        let at = self.at(off, 4).ok_or(Fault::Bounds)?;
        // SAFETY: as in `read`; the mapping is writable (it is a device's registers).
        unsafe { (at as *mut u32).write_volatile(value) };
        Ok(())
    }

    /// Makes these registers the ones the panic hook resets, and installs the hook
    /// (`redoubt_rt::start::set_panic_hook`). Once per process; `false` if it was done already.
    pub fn arm_panic_reset(&self) -> bool {
        let armed = PANIC_BASE.compare_exchange(0, self.base, Ordering::AcqRel, Ordering::Acquire).is_ok();
        PANIC_LEN.store(self.len, Ordering::Release);
        armed && redoubt_rt::start::set_panic_hook(panic_reset)
    }
}

/// The registers the panic hook resets: copied from the one `Regs` `arm_panic_reset` was called
/// on, so rebuilding a `Regs` from them names the same mapping. 0 before it is armed.
static PANIC_BASE: AtomicUsize = AtomicUsize::new(0);
static PANIC_LEN: AtomicUsize = AtomicUsize::new(0);

/// The panic hook: stop the device (status 0, read back, bounded), so it writes nothing more into
/// pages the panic is about to free. Bounded, no allocation, only atomics and two registers.
fn panic_reset() {
    let (base, len) = (PANIC_BASE.load(Ordering::Acquire), PANIC_LEN.load(Ordering::Acquire));
    if base == 0 {
        return;
    }
    let regs = Regs { base, len };
    let _ = regs.write(reg::STATUS, 0);
    for _ in 0..crate::virtio::RESET_TRIES {
        if regs.read(reg::STATUS) == Ok(0) {
            return;
        }
    }
}

/// One view of the device: its registers, one DMA region and, for the receive thread's view,
/// the interrupt.
pub struct Device {
    regs: Regs,
    dma: usize,
    dma_phys: u64,
    irq: Option<Irq>,
}

impl Device {
    /// A view over `regs` with a fresh region of [`REGION_PAGES`] pages from `dma_alloc` on
    /// `mmio` (which must carry the DMA flag: `dma_alloc` refuses otherwise).
    pub fn new(regs: Regs, mmio: &Mmio, irq: Option<Irq>) -> Result<Device, Error> {
        let (dma, dma_phys) = mmio.dma_alloc(REGION_PAGES)?;
        Ok(Device { regs, dma, dma_phys, irq })
    }

    fn dma_at(&self, off: usize, len: usize) -> Option<usize> {
        if off.checked_add(len)? > REGION_LEN {
            return None;
        }
        self.dma.checked_add(off)
    }
}

impl Transport for Device {
    fn reg_read(&self, off: usize) -> Result<u32, Fault> { self.regs.read(off) }

    fn reg_read_u8(&self, off: usize) -> Result<u8, Fault> { self.regs.read_u8(off) }

    fn reg_write(&self, off: usize, value: u32) -> Result<(), Fault> { self.regs.write(off, value) }

    fn dma_read(&self, off: usize, out: &mut [u8]) -> Result<(), Fault> {
        let at = self.dma_at(off, out.len()).ok_or(Fault::Bounds)?;
        for (i, byte) in out.iter_mut().enumerate() {
            // SAFETY: `dma_at` checked `off .. off + out.len()` against `REGION_LEN`, this crate's
            // own constant and the pages `dma_alloc` was asked for, and `i < out.len()`, so
            // `at + i` is inside the region, which stays mapped for the life of this process.
            // Volatile and a byte at a time, so no `&[u8]` of memory the device writes ever
            // exists and no read of it can be hoisted above the check that made it valid.
            *byte = unsafe { ((at + i) as *const u8).read_volatile() };
        }
        Ok(())
    }

    fn dma_write(&self, off: usize, src: &[u8]) -> Result<(), Fault> {
        let at = self.dma_at(off, src.len()).ok_or(Fault::Bounds)?;
        for (i, byte) in src.iter().enumerate() {
            // SAFETY: as in `dma_read`; the region is mapped read-write for this process.
            unsafe { ((at + i) as *mut u8).write_volatile(*byte) };
        }
        Ok(())
    }

    fn dma_zero(&self, off: usize, len: usize) -> Result<(), Fault> {
        let at = self.dma_at(off, len).ok_or(Fault::Bounds)?;
        for i in 0..len {
            // SAFETY: as in `dma_write`, for `len` bytes `dma_at` checked.
            unsafe { ((at + i) as *mut u8).write_volatile(0) };
        }
        Ok(())
    }

    fn dma_phys(&self) -> u64 { self.dma_phys }

    fn dma_len(&self) -> usize { REGION_LEN }

    fn wait_irq(&self, timeout_us: u64) -> Result<(), Fault> {
        let irq = self.irq.as_ref().ok_or(Fault::Kernel)?;
        match irq.wait(timeout_us) {
            Ok(()) => Ok(()),
            Err(Error::Timeout) => Err(Fault::Timeout),
            Err(_) => Err(Fault::Kernel),
        }
    }

    /// A clock that fails saturates rather than reads 0, so a deadline computed from it is never
    /// silently in the far future.
    fn now_us(&self) -> u64 { time_now().unwrap_or(u64::MAX) }
}

/// The receive thread's half, between [`start_rx_thread`] and [`take_rx_part`]. Only ever null
/// or a pointer from `Box::into_raw` in [`start_rx_thread`].
static RX_PART: AtomicPtr<RxPart> = AtomicPtr::new(null_mut());

/// Starts the receive thread at `entry` on the stack whose top is `sp`, handing it `part`. No
/// address travels in a message: the part is moved in memory this process owns, before the thread
/// that takes it exists. If the thread cannot be started, the part is taken back and dropped here
/// (the `Box` reclaimed, never retried with); the caller resets the device.
pub fn start_rx_thread(part: RxPart, entry: extern "C" fn(usize) -> !, sp: usize) -> Result<(), Error> {
    let raw = Box::into_raw(Box::new(part));
    let previous = RX_PART.swap(raw, Ordering::AcqRel);
    debug_assert!(previous.is_null(), "one receive thread per process");
    redoubt_rt::handle::thread_create(entry as usize, sp, 0).map(|_| ()).inspect_err(|_| {
        drop(take_rx_part());
    })
}

/// The receive thread's half, once: `None` if it has been taken already.
pub fn take_rx_part() -> Option<RxPart> {
    let raw = RX_PART.swap(null_mut(), Ordering::AcqRel);
    if raw.is_null() {
        return None;
    }
    // SAFETY: every non-null value `RX_PART` ever holds came from `Box::into_raw` in
    // `start_rx_thread`, and `swap` hands it to exactly one caller, so this is the one
    // `Box::from_raw` of that allocation.
    Some(*unsafe { Box::from_raw(raw) })
}
