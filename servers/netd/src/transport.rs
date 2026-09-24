//! The seam between `netd` and the kernel: `blkd`'s seam (IO-ARCHITECTURE.md), with one more
//! operation, a byte-wide register read, because virtio-net's MAC is an array of bytes and
//! virtio-mmio asks for byte-wide accesses to byte-wide configuration fields (§4.2.2.2).
//!
//! On the machine it is [`crate::kernel::Device`], the one module in this crate with `unsafe` in
//! it; in tests it is [`crate::fake::FakeNic`]'s two views, a deliberately hostile virtio-net
//! device in safe Rust. **Every access is bounds-checked by the implementation**, so an offset
//! the region does not hold is [`Fault::Bounds`], a bug in us, never a write past the end.
//!
//! One `Transport` is **one DMA region** and the device's registers. `netd` has two regions, one
//! per queue, so it has two transports over the same registers: one per thread, and neither
//! thread ever touches the other's region.

/// The seam failed. None of these is the device telling `netd` something.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The kernel refused the call (the handle went away).
    Kernel,
    /// An offset outside the region or the registers: a bug here, not a hostile device.
    Bounds,
    /// The interrupt did not arrive before the deadline.
    Timeout,
}

/// A virtio-mmio device's registers, one DMA region, its interrupt and the clock.
pub trait Transport {
    /// Reads the 32-bit register at `off` bytes from the MMIO base.
    fn reg_read(&self, off: usize) -> Result<u32, Fault>;

    /// Reads the byte at `off` bytes from the MMIO base (a byte of configuration space).
    fn reg_read_u8(&self, off: usize) -> Result<u8, Fault>;

    /// Writes the 32-bit register at `off` bytes from the MMIO base.
    fn reg_write(&self, off: usize, value: u32) -> Result<(), Fault>;

    /// Copies `out.len()` bytes out of the DMA region at `off`.
    fn dma_read(&self, off: usize, out: &mut [u8]) -> Result<(), Fault>;

    /// Copies `src` into the DMA region at `off`.
    fn dma_write(&self, off: usize, src: &[u8]) -> Result<(), Fault>;

    /// Fills `len` bytes of the DMA region at `off` with zeros.
    fn dma_zero(&self, off: usize, len: usize) -> Result<(), Fault>;

    /// The physical address of the DMA region's first byte: what goes into a descriptor.
    fn dma_phys(&self) -> u64;

    /// The DMA region's length in bytes.
    fn dma_len(&self) -> usize;

    /// Waits up to `timeout_us` microseconds for the device's interrupt (R5).
    fn wait_irq(&self, timeout_us: u64) -> Result<(), Fault>;

    /// Microseconds since boot, for deadlines.
    fn now_us(&self) -> u64;

    /// Orders what came before against what comes after, both ways.
    fn fence(&self) { core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst); }

    fn dma_read_u16(&self, off: usize) -> Result<u16, Fault> {
        let mut bytes = [0; 2];
        self.dma_read(off, &mut bytes)?;
        Ok(u16::from_le_bytes(bytes))
    }

    fn dma_read_u32(&self, off: usize) -> Result<u32, Fault> {
        let mut bytes = [0; 4];
        self.dma_read(off, &mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

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

/// A shared borrow is a transport too, so a test can keep the device while the driver holds it.
impl<T: Transport + ?Sized> Transport for &T {
    fn reg_read(&self, off: usize) -> Result<u32, Fault> { (**self).reg_read(off) }

    fn reg_read_u8(&self, off: usize) -> Result<u8, Fault> { (**self).reg_read_u8(off) }

    fn reg_write(&self, off: usize, value: u32) -> Result<(), Fault> { (**self).reg_write(off, value) }

    fn dma_read(&self, off: usize, out: &mut [u8]) -> Result<(), Fault> { (**self).dma_read(off, out) }

    fn dma_write(&self, off: usize, src: &[u8]) -> Result<(), Fault> { (**self).dma_write(off, src) }

    fn dma_zero(&self, off: usize, len: usize) -> Result<(), Fault> { (**self).dma_zero(off, len) }

    fn dma_phys(&self) -> u64 { (**self).dma_phys() }

    fn dma_len(&self) -> usize { (**self).dma_len() }

    fn wait_irq(&self, timeout_us: u64) -> Result<(), Fault> { (**self).wait_irq(timeout_us) }

    fn now_us(&self) -> u64 { (**self).now_us() }

    fn fence(&self) { (**self).fence() }
}
