//! The whole disk: bring-up, and the three operations `blkd` serves over a range.
//!
//! # Where the bytes go
//! A client's lent pages never reach the device (servers/blkd.md R51). A `write` is copied
//! from the lend into the DMA data buffer before the request is offered; a `read` is copied out
//! of the DMA data buffer into `blkd`'s own memory, **once**, with a length this driver chose,
//! before anything looks at a byte of it. So the device sees only [`crate::queue`]'s region, and
//! the bytes the rest of `blkd` works on are bytes that cannot change underneath it.
//!
//! # Broken stays broken
//! A device that breaks the protocol, or that misses `virtio::REQUEST_TIMEOUT_US`, is marked
//! [`Disk::is_broken`] and answers every later request with [`DeviceError::Broken`]. Carrying on
//! after a timeout would mean a completion arriving for a request no longer tracked, and carrying
//! on after a lie would mean trusting the liar; `init` restarts `blkd`, which resets the device
//! from the beginning (servers/init.md, "Restarts and reboots"). A device that merely *reports*
//! a failure (virtio-blk status `IOERR` or `UNSUPP`) is behaving, so that fails one request and
//! nothing more.

use crate::queue::{DATA_OFF, HEADER_OFF, Queue, STATUS_OFF, Segment};
use crate::transport::Transport;
use crate::virtio::{self, DATA_LEN, DeviceError, Features, SECTOR_SIZE, blk_status, request};

/// The virtio-blk request header, in bytes (§5.2.6).
const HEADER_BYTES: u32 = 16;

/// The virtio-blk device, brought up and ready to serve.
pub struct Disk<T: Transport> {
    transport: T,
    queue: Queue,
    sectors: u64,
    read_only: bool,
    broken: bool,
}

impl<T: Transport> Disk<T> {
    /// Identifies the device, negotiates features, sets the queue up and reads the capacity
    /// (virtio 1.2, §3.1.1). Every step refuses rather than works around.
    pub fn new(transport: T) -> Result<Disk<T>, DeviceError> {
        virtio::identify(&transport)?;
        let Features { read_only } = virtio::negotiate(&transport)?;
        let mut queue = Queue::new();
        queue.configure(&transport)?;
        virtio::driver_ok(&transport)?;
        let sectors = virtio::capacity(&transport)?;
        Ok(Disk { transport, queue, sectors, read_only, broken: false })
    }

    /// The disk's size in 512-byte sectors, as the configuration space reported it at bring-up.
    /// It is read once: a device that changes it later changes nothing, since every range was
    /// checked against this number when it was made.
    pub fn sectors(&self) -> u64 { self.sectors }

    pub fn read_only(&self) -> bool { self.read_only }

    pub fn is_broken(&self) -> bool { self.broken }

    /// Reads `out.len() / 512` sectors from `sector` into `out`.
    ///
    /// `out` is `blkd`'s own buffer, never a client's lend: the reply is encoded from it
    /// afterwards.
    pub fn read(&mut self, sector: u64, out: &mut [u8]) -> Result<(), DeviceError> {
        self.usable()?;
        let bytes = self.check_span(sector, out.len())?;
        // The data buffer is cleared before the device is asked to fill it, so a device that
        // writes fewer bytes than it was given — or none — hands back zeros rather than what the
        // last request left there, which may have been another client's. A hostile device can
        // choose the bytes it returns anyway, but `blkd` itself never passes one client's data to
        // another, and that does not depend on the device at all.
        self.clear_data(bytes)?;
        self.transaction(
            request::IN,
            sector,
            &[
                Segment { off: HEADER_OFF, len: HEADER_BYTES, device_writes: false },
                Segment { off: DATA_OFF, len: bytes, device_writes: true },
                Segment { off: STATUS_OFF, len: 1, device_writes: true },
            ],
        )?;
        // The one read of the device's bytes, with this driver's own length.
        self.transport.dma_read(DATA_OFF, out).map_err(|e| self.break_on(e.into()))
    }

    /// Writes `data` (a whole number of sectors) at `sector`.
    pub fn write(&mut self, sector: u64, data: &[u8]) -> Result<(), DeviceError> {
        // Broken first, and before the payload is copied anywhere: a device that has already
        // lied is never handed another client's bytes.
        self.usable()?;
        if self.read_only {
            return Err(DeviceError::ReadOnly);
        }
        let bytes = self.check_span(sector, data.len())?;
        // The client's bytes are copied into the DMA buffer here, and the device is given that
        // buffer; the lend they came from is never named to the device.
        self.transport.dma_write(DATA_OFF, data).map_err(|e| self.break_on(e.into()))?;
        self.transaction(
            request::OUT,
            sector,
            &[
                Segment { off: HEADER_OFF, len: HEADER_BYTES, device_writes: false },
                Segment { off: DATA_OFF, len: bytes, device_writes: false },
                Segment { off: STATUS_OFF, len: 1, device_writes: true },
            ],
        )
    }

    /// Issues virtio-blk's flush and returns only when the device says it completed
    /// (servers/blkd.md, "Messages": `sync` is never acknowledged before the data is durable).
    /// `BLK_FLUSH` was required at negotiation, so the device cannot answer `UNSUPP` without
    /// breaking its own word.
    pub fn flush(&mut self) -> Result<(), DeviceError> {
        // Broken first: a broken read-only device must not answer ok to a `sync`.
        self.usable()?;
        if self.read_only {
            // Nothing was ever written, so there is nothing to make durable.
            return Ok(());
        }
        self.transaction(
            request::FLUSH,
            0,
            &[
                Segment { off: HEADER_OFF, len: HEADER_BYTES, device_writes: false },
                Segment { off: STATUS_OFF, len: 1, device_writes: true },
            ],
        )
    }

    /// `Broken` once the device has lied or timed out, and nothing else ever again.
    fn usable(&self) -> Result<(), DeviceError> {
        if self.broken { Err(DeviceError::Broken) } else { Ok(()) }
    }

    /// `len` as a sector count that fits the disk, this driver's per-request bound and the data
    /// buffer. None of these refusals touches the device, so a client asking for too much does
    /// not cost anyone else the disk.
    fn check_span(&self, sector: u64, len: usize) -> Result<u32, DeviceError> {
        // `DATA_LEN` is exactly `MAX_SECTORS` sectors (`virtio.rs`), so this one check bounds both
        // the byte length and the sector count, and rejects an empty or unaligned request too.
        if len == 0 || len > DATA_LEN || !len.is_multiple_of(SECTOR_SIZE as usize) {
            return Err(DeviceError::Range);
        }
        let count = (len / SECTOR_SIZE as usize) as u64;
        match sector.checked_add(count) {
            Some(end) if end <= self.sectors => Ok(len as u32),
            _ => Err(DeviceError::Range),
        }
    }

    /// One request, start to finish: header, chain, doorbell, completion, status byte.
    fn transaction(&mut self, kind: u32, sector: u64, chain: &[Segment]) -> Result<(), DeviceError> {
        self.usable()?;
        let writable = writable(chain);
        match self.run(kind, sector, chain, writable) {
            Ok(()) => Ok(()),
            // The device answered with a failure it is entitled to report; it is still speaking
            // the protocol, so it is not broken.
            Err(e @ (DeviceError::Rejected | DeviceError::ReadOnly | DeviceError::Range)) => Err(e),
            Err(e) => Err(self.break_on(e)),
        }
    }

    fn run(&mut self, kind: u32, sector: u64, chain: &[Segment], writable: u32) -> Result<(), DeviceError> {
        let t = &self.transport;
        // The header is written fresh every time, so whatever the device did to it since the last
        // request is gone before the next one is offered.
        t.dma_write_u32(HEADER_OFF, kind)?;
        t.dma_write_u32(HEADER_OFF + 4, 0)?;
        t.dma_write_u64(HEADER_OFF + 8, sector)?;
        // A status byte of `OK` is never left behind from the last request: if the device writes
        // nothing at all, the value read back is one no device would mean.
        t.dma_write_u8(STATUS_OFF, 0xff)?;
        self.queue.submit(t, chain)?;
        self.queue.complete(t, writable)?;
        let mut status = [0; 1];
        t.dma_read(STATUS_OFF, &mut status)?;
        match status[0] {
            blk_status::OK => Ok(()),
            blk_status::IOERR | blk_status::UNSUPP => Err(DeviceError::Rejected),
            // Not one of the three §5.2.6 defines: the device is not speaking virtio-blk.
            _ => Err(DeviceError::Io),
        }
    }

    /// Zeroes the first `bytes` of the data buffer, a page at a time so the zeros are a constant
    /// rather than an allocation.
    fn clear_data(&mut self, bytes: u32) -> Result<(), DeviceError> {
        const ZEROS: [u8; 512] = [0; 512];
        let mut done = 0usize;
        let total = bytes as usize;
        while done < total {
            let n = ZEROS.len().min(total - done);
            self.transport.dma_write(DATA_OFF + done, &ZEROS[..n]).map_err(|e| self.break_on(e.into()))?;
            done += n;
        }
        Ok(())
    }

    /// Marks the device broken and returns the error that did it.
    fn break_on(&mut self, error: DeviceError) -> DeviceError {
        self.broken = true;
        error
    }
}

/// The bytes the device was given to write: what the used ring's reported length is checked
/// against. A chain longer than one request needs is refused by `Queue::submit`, which knows the
/// ring's size, so nothing here has to bound it.
fn writable(chain: &[Segment]) -> u32 {
    chain.iter().filter(|s| s.device_writes).map(|s| s.len).fold(0, u32::saturating_add)
}
