//! The whole disk: bring-up, and the three operations `blkd` serves over a range.
//!
//! # Where the bytes go
//! A client's lent pages never reach the device (IO-ARCHITECTURE.md, DMA). A `write` is copied
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
//! from the beginning (INIT.md). A device that merely *reports* a failure (virtio-blk status
//! `IOERR` or `UNSUPP`) is behaving, so that fails one request and nothing more.

use crate::queue::{DATA_OFF, HEADER_OFF, Queue, STATUS_OFF, Segment};
use crate::transport::Transport;
use crate::virtio::{self, DATA_LEN, DeviceError, Features, MAX_SECTORS, SECTOR_SIZE, blk_status, request};

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

    pub fn transport(&self) -> &T { &self.transport }

    /// Reads `out.len() / 512` sectors from `sector` into `out`.
    ///
    /// `out` is `blkd`'s own buffer, never a client's lend: the reply is encoded from it
    /// afterwards.
    pub fn read(&mut self, sector: u64, out: &mut [u8]) -> Result<(), DeviceError> {
        let bytes = self.check_span(sector, out.len())?;
        let chain = Chain::of(&[
            Segment { off: HEADER_OFF, len: HEADER_BYTES, device_writes: false },
            Segment { off: DATA_OFF, len: bytes, device_writes: true },
            Segment { off: STATUS_OFF, len: 1, device_writes: true },
        ]);
        self.transaction(request::IN, sector, &chain)?;
        // The one read of the device's bytes, with this driver's own length.
        self.transport.dma_read(DATA_OFF, out).map_err(|e| self.break_on(e.into()))
    }

    /// Writes `data` (a whole number of sectors) at `sector`.
    pub fn write(&mut self, sector: u64, data: &[u8]) -> Result<(), DeviceError> {
        if self.read_only {
            return Err(DeviceError::ReadOnly);
        }
        let bytes = self.check_span(sector, data.len())?;
        // The client's bytes are copied into the DMA buffer here, and the device is given that
        // buffer; the lend they came from is never named to the device.
        self.transport.dma_write(DATA_OFF, data).map_err(|e| self.break_on(e.into()))?;
        let chain = Chain::of(&[
            Segment { off: HEADER_OFF, len: HEADER_BYTES, device_writes: false },
            Segment { off: DATA_OFF, len: bytes, device_writes: false },
            Segment { off: STATUS_OFF, len: 1, device_writes: true },
        ]);
        self.transaction(request::OUT, sector, &chain)
    }

    /// Issues virtio-blk's flush and returns only when the device says it completed
    /// (IO-ARCHITECTURE.md, `blkd`'s contract: `sync` is never acknowledged before the data is
    /// durable). `BLK_FLUSH` was required at negotiation, so the device cannot answer `UNSUPP`
    /// without breaking its own word.
    pub fn flush(&mut self) -> Result<(), DeviceError> {
        if self.read_only {
            // Nothing was ever written, so there is nothing to make durable.
            return Ok(());
        }
        let chain = Chain::of(&[
            Segment { off: HEADER_OFF, len: HEADER_BYTES, device_writes: false },
            Segment { off: STATUS_OFF, len: 1, device_writes: true },
        ]);
        self.transaction(request::FLUSH, 0, &chain)
    }

    /// `len` as a sector count that fits the disk, this driver's per-request bound and the data
    /// buffer. None of these refusals touches the device, so a client asking for too much does
    /// not cost anyone else the disk.
    fn check_span(&self, sector: u64, len: usize) -> Result<u32, DeviceError> {
        if len == 0 || len > DATA_LEN || !len.is_multiple_of(SECTOR_SIZE as usize) {
            return Err(DeviceError::Range);
        }
        let count = (len / SECTOR_SIZE as usize) as u64;
        if count > u64::from(MAX_SECTORS) {
            return Err(DeviceError::Range);
        }
        match sector.checked_add(count) {
            Some(end) if end <= self.sectors => Ok(len as u32),
            _ => Err(DeviceError::Range),
        }
    }

    /// One request, start to finish: header, chain, doorbell, completion, status byte.
    fn transaction(&mut self, kind: u32, sector: u64, chain: &Chain) -> Result<(), DeviceError> {
        if self.broken {
            return Err(DeviceError::Broken);
        }
        let writable = chain.writable();
        match self.run(kind, sector, chain, writable) {
            Ok(()) => Ok(()),
            // The device answered with a failure it is entitled to report; it is still speaking
            // the protocol, so it is not broken.
            Err(e @ (DeviceError::Rejected | DeviceError::ReadOnly | DeviceError::Range)) => Err(e),
            Err(e) => Err(self.break_on(e)),
        }
    }

    fn run(&mut self, kind: u32, sector: u64, chain: &Chain, writable: u32) -> Result<(), DeviceError> {
        let t = &self.transport;
        // The header is written fresh every time, so whatever the device did to it since the last
        // request is gone before the next one is offered.
        t.dma_write_u32(HEADER_OFF, kind)?;
        t.dma_write_u32(HEADER_OFF + 4, 0)?;
        t.dma_write_u64(HEADER_OFF + 8, sector)?;
        // A status byte of `OK` is never left behind from the last request: if the device writes
        // nothing at all, the value read back is one no device would mean.
        t.dma_write_u8(STATUS_OFF, 0xff)?;
        self.queue.submit(t, chain.as_slice())?;
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

    /// Marks the device broken and returns the error that did it.
    fn break_on(&mut self, error: DeviceError) -> DeviceError {
        self.broken = true;
        error
    }
}

/// The descriptor chain of one request: at most three segments, on the stack, no allocation.
struct Chain {
    segments: [Segment; MAX_SEGMENTS],
    len: usize,
}

/// Header, data, status: the longest chain `blkd` builds.
const MAX_SEGMENTS: usize = 3;

impl Chain {
    /// The chain of `segments`, which is a slice of at most [`MAX_SEGMENTS`] built right here;
    /// a longer one is a bug in this file and is truncated rather than given to the device.
    fn of(segments: &[Segment]) -> Chain {
        let mut chain =
            Chain { segments: [Segment { off: 0, len: 0, device_writes: false }; MAX_SEGMENTS], len: 0 };
        for segment in segments.iter().take(MAX_SEGMENTS) {
            chain.segments[chain.len] = *segment;
            chain.len += 1;
        }
        chain
    }

    fn as_slice(&self) -> &[Segment] { &self.segments[..self.len] }

    /// The bytes the device was given to write: what the used ring's reported length is checked
    /// against.
    fn writable(&self) -> u32 {
        self.as_slice().iter().filter(|s| s.device_writes).map(|s| s.len).fold(0, u32::saturating_add)
    }
}
