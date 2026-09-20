//! The seam between `blkd` and the kernel, and the only thing the rest of the crate knows about
//! hardware.
//!
//! Everything the driver does to a device is one of seven operations: read or write a 32-bit
//! MMIO register, read or write bytes of the DMA region, learn that region's physical address
//! and length, wait for the interrupt, and read the clock. [`Transport`] is those, and nothing
//! else. On the machine it is implemented by [`crate::kernel::Device`], which is the one module
//! in this crate with `unsafe` in it; in tests it is implemented by [`crate::fake::FakeDevice`],
//! which is a deliberately hostile virtio-blk device in safe Rust.
//!
//! **Every access is bounds-checked by the implementation**, so the driver can name an offset
//! without proving anything about it, and an offset the region does not hold is [`Fault::Bounds`]
//! — a bug in us, reported, never a write past the end.
//!
//! **Every access goes through this trait**, which is what makes the hostile model real: the fake
//! device sees each read and write in order and may change the region between any two of them,
//! exactly as a device with no IOMMU can.

/// The seam failed. None of these is a device telling `blkd` something; they are `blkd`'s own
/// machinery not working.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The kernel refused the call (the handle went away, the IRQ object is gone).
    Kernel,
    /// An offset outside the region. The driver's offsets are constants checked by
    /// [`crate::queue::LAYOUT_FITS`], so this means a bug here, not a hostile device.
    Bounds,
    /// The interrupt did not arrive before the deadline.
    Timeout,
}

/// One virtio-mmio device: its registers, its DMA region, its interrupt and the clock.
///
/// The DMA region is the **whole** of what the device may reach. It is one contiguous run of
/// pages from `dma_alloc`, and the driver puts every address it ever gives the device inside it
/// ([`crate::queue`]).
pub trait Transport {
    /// Reads the 32-bit register at `off` bytes from the MMIO base.
    fn reg_read(&self, off: usize) -> Result<u32, Fault>;

    /// Writes the 32-bit register at `off` bytes from the MMIO base.
    fn reg_write(&self, off: usize, value: u32) -> Result<(), Fault>;

    /// Copies `out.len()` bytes out of the DMA region at `off`.
    fn dma_read(&self, off: usize, out: &mut [u8]) -> Result<(), Fault>;

    /// Copies `src` into the DMA region at `off`.
    fn dma_write(&self, off: usize, src: &[u8]) -> Result<(), Fault>;

    /// The physical address of the DMA region's first byte: what goes into a descriptor.
    fn dma_phys(&self) -> u64;

    /// The DMA region's length in bytes.
    fn dma_len(&self) -> usize;

    /// Waits up to `timeout_us` microseconds for the device's interrupt (R5: `receive` on the IRQ
    /// handle unmasks the source and returns when it has fired; there is no acknowledge call).
    fn wait_irq(&self, timeout_us: u64) -> Result<(), Fault>;

    /// Microseconds since boot, for deadlines.
    fn now_us(&self) -> u64;

    /// Orders what came before against what comes after, both ways. Called before the driver
    /// tells the device to look at the region, and after it learns the device has written to it.
    fn fence(&self) { core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst); }

    /// The 16-bit little-endian value at `off`.
    fn dma_read_u16(&self, off: usize) -> Result<u16, Fault> {
        let mut bytes = [0; 2];
        self.dma_read(off, &mut bytes)?;
        Ok(u16::from_le_bytes(bytes))
    }

    /// The 32-bit little-endian value at `off`.
    fn dma_read_u32(&self, off: usize) -> Result<u32, Fault> {
        let mut bytes = [0; 4];
        self.dma_read(off, &mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    /// The 64-bit little-endian value at `off`.
    fn dma_read_u64(&self, off: usize) -> Result<u64, Fault> {
        let mut bytes = [0; 8];
        self.dma_read(off, &mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn dma_write_u8(&self, off: usize, value: u8) -> Result<(), Fault> { self.dma_write(off, &[value]) }

    fn dma_write_u16(&self, off: usize, value: u16) -> Result<(), Fault> {
        self.dma_write(off, &value.to_le_bytes())
    }

    fn dma_write_u32(&self, off: usize, value: u32) -> Result<(), Fault> {
        self.dma_write(off, &value.to_le_bytes())
    }

    fn dma_write_u64(&self, off: usize, value: u64) -> Result<(), Fault> {
        self.dma_write(off, &value.to_le_bytes())
    }
}

/// A shared borrow is a transport too, so a test can keep the device to change its policy while
/// the driver holds it. The driver never needs to own its transport: everything it does to one
/// takes `&self`.
impl<T: Transport + ?Sized> Transport for &T {
    fn reg_read(&self, off: usize) -> Result<u32, Fault> { (**self).reg_read(off) }

    fn reg_write(&self, off: usize, value: u32) -> Result<(), Fault> { (**self).reg_write(off, value) }

    fn dma_read(&self, off: usize, out: &mut [u8]) -> Result<(), Fault> { (**self).dma_read(off, out) }

    fn dma_write(&self, off: usize, src: &[u8]) -> Result<(), Fault> { (**self).dma_write(off, src) }

    fn dma_phys(&self) -> u64 { (**self).dma_phys() }

    fn dma_len(&self) -> usize { (**self).dma_len() }

    fn wait_irq(&self, timeout_us: u64) -> Result<(), Fault> { (**self).wait_irq(timeout_us) }

    fn now_us(&self) -> u64 { (**self).now_us() }

    fn fence(&self) { (**self).fence() }
}
