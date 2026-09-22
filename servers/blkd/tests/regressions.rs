//! The findings the WP-D1 red team raised, each with the case that would have caught it.
//!
//! Every one needs to see the seam itself — how many times the driver acknowledged an interrupt,
//! what bytes it put in the DMA region, what its clock said — so these wrap
//! `redoubt_blkd::fake::FakeDevice` in a [`Transport`] that watches or lies about one operation
//! and passes the rest through. The device-side attacks that need only a [`Policy`] live in
//! `device.rs`.

use core::cell::{Cell, RefCell};

use redoubt_blkd::fake::{FakeDevice, Policy, Scribble};
use redoubt_blkd::image::{Entry, Image};
use redoubt_blkd::queue;
use redoubt_blkd::transport::{Fault, Transport};
use redoubt_blkd::virtio::{DeviceError, SECTOR_SIZE, bit, feature, reg};
use redoubt_blkd::{Disk, read_partitions};

const SECTORS: u64 = 8192;
const SECTOR: usize = SECTOR_SIZE as usize;

fn image() -> Image {
    Image::new(SECTORS, &[Entry { first_lba: 64, last_lba: 1063 }, Entry { first_lba: 2048, last_lba: 4095 }])
}

fn device() -> FakeDevice {
    let device = FakeDevice::with_image(image().bytes);
    device.set_policy(Policy { defer: true, ..Policy::default() });
    device
}

/// A seam that passes everything through and counts what matters, so a test can assert on what
/// the driver did rather than only on what it answered.
struct Watch<'a> {
    inner: &'a FakeDevice,
    /// Writes of `InterruptACK`.
    acks: Cell<u64>,
    /// Every non-zero run the driver put into the data buffer.
    payloads: RefCell<Vec<Vec<u8>>>,
    /// A clock of the test's own: `None` passes the device's through. A storming device advances
    /// it by `wake_cost` on every wake, which is what a wake costs on a real machine and what the
    /// ten-second deadline is counted in.
    clock: Cell<Option<u64>>,
    wake_cost: Cell<u64>,
    /// Operations, so a test that expects an unbounded spin can cut it off instead of hanging.
    ops: Cell<u64>,
    cap: u64,
    /// The device re-asserts its interrupt the instant it is acknowledged, and never completes.
    storm: Cell<bool>,
}

impl<'a> Watch<'a> {
    fn new(inner: &'a FakeDevice) -> Watch<'a> {
        Watch {
            inner,
            acks: Cell::new(0),
            payloads: RefCell::new(Vec::new()),
            clock: Cell::new(None),
            wake_cost: Cell::new(0),
            ops: Cell::new(0),
            cap: 4_000_000,
            storm: Cell::new(false),
        }
    }

    fn tick(&self) -> Result<(), Fault> {
        let n = self.ops.get() + 1;
        self.ops.set(n);
        if n > self.cap { Err(Fault::Kernel) } else { Ok(()) }
    }
}

impl Transport for Watch<'_> {
    fn reg_read(&self, off: usize) -> Result<u32, Fault> {
        self.tick()?;
        if self.storm.get() && off == reg::INTERRUPT_STATUS {
            return Ok(1);
        }
        self.inner.reg_read(off)
    }

    fn reg_write(&self, off: usize, value: u32) -> Result<(), Fault> {
        self.tick()?;
        if off == reg::INTERRUPT_ACK {
            self.acks.set(self.acks.get() + 1);
            if self.storm.get() {
                // The device ignores the acknowledgement and keeps its line asserted.
                return Ok(());
            }
        }
        self.inner.reg_write(off, value)
    }

    fn dma_read(&self, off: usize, out: &mut [u8]) -> Result<(), Fault> {
        self.tick()?;
        self.inner.dma_read(off, out)
    }

    fn dma_write(&self, off: usize, src: &[u8]) -> Result<(), Fault> {
        self.tick()?;
        if off >= queue::DATA_OFF && src.iter().any(|b| *b != 0) {
            self.payloads.borrow_mut().push(src.to_vec());
        }
        self.inner.dma_write(off, src)
    }

    fn dma_phys(&self) -> u64 { self.inner.dma_phys() }

    fn dma_len(&self) -> usize { self.inner.dma_len() }

    fn wait_irq(&self, timeout_us: u64) -> Result<(), Fault> {
        self.tick()?;
        if self.storm.get() {
            // The line is asserted, so `receive` returns at once, every time -- but the round
            // trip through the kernel still costs time, which is what the deadline counts.
            if let Some(now) = self.clock.get() {
                self.clock.set(Some(now.saturating_add(self.wake_cost.get())));
            }
            return Ok(());
        }
        self.inner.wait_irq(timeout_us)
    }

    fn now_us(&self) -> u64 { self.clock.get().unwrap_or_else(|| self.inner.now_us()) }
}

/// **A2.** `flush` used to answer ok on a device that was read-only *and* broken, because the
/// read-only shortcut came before the broken check. A `sync` that answers ok is a promise the
/// data is durable, and that promise must never come from a device that has already lied.
#[test]
fn a_broken_read_only_device_does_not_answer_ok_to_a_flush() {
    let device = device();
    device.set_policy(Policy {
        defer: true,
        features: Some(bit(feature::VERSION_1) | bit(feature::BLK_FLUSH) | bit(feature::BLK_RO)),
        ..Policy::default()
    });
    let mut disk = Disk::new(&device).expect("bring-up");
    assert!(disk.read_only());
    // A read-only disk answers ok to a flush while it is healthy: nothing was written.
    assert_eq!(disk.flush(), Ok(()));
    // Break it.
    device.set_policy(Policy { defer: true, used_idx_delta: Some(9), ..Policy::default() });
    let mut out = vec![0; SECTOR];
    assert_eq!(disk.read(64, &mut out), Err(DeviceError::Io));
    assert!(disk.is_broken());
    assert_eq!(disk.flush(), Err(DeviceError::Broken), "a broken device promised durability");
}

/// **A3.** The completion is often already there when the driver first looks, and that path used
/// to skip `InterruptACK` entirely: virtio wants the driver to acknowledge an interrupt it was
/// sent (§4.2.2), and the kernel masking the source until the next `receive` (R5) made that
/// survivable rather than right.
#[test]
fn every_completion_acknowledges_the_interrupt() {
    for defer in [false, true] {
        let device = device();
        device.set_policy(Policy { defer, ..Policy::default() });
        let watch = Watch::new(&device);
        let mut disk = Disk::new(&watch).expect("bring-up");
        let mut out = vec![0; SECTOR];
        for _ in 0..16 {
            disk.read(64, &mut out).expect("a read");
        }
        assert!(watch.acks.get() >= 16, "defer={defer}: {} acks for 16 completions", watch.acks.get());
        assert_eq!(
            watch.inner.reg_read(reg::INTERRUPT_STATUS).unwrap() & 1,
            0,
            "defer={defer}: the line is still asserted"
        );
    }
}

/// **A10.** A `write` used to copy the client's bytes into the DMA region before testing whether
/// the device was already broken, so a device that had lied was handed another client's plaintext
/// on its way to being refused.
#[test]
fn a_broken_device_is_never_handed_a_clients_write_payload() {
    let device = device();
    let watch = Watch::new(&device);
    let mut disk = Disk::new(&watch).expect("bring-up");
    device.set_policy(Policy { defer: true, used_idx_delta: Some(9), ..Policy::default() });
    let mut out = vec![0; SECTOR];
    assert_eq!(disk.read(64, &mut out), Err(DeviceError::Io));
    assert!(disk.is_broken());
    watch.payloads.borrow_mut().clear();

    let secret = vec![0x5a; SECTOR];
    assert_eq!(disk.write(64, &secret), Err(DeviceError::Broken));
    assert!(watch.payloads.borrow().is_empty(), "the payload was copied into the DMA region anyway");
    // And it is not sitting in the buffer for the device to read at leisure.
    let mut peek = vec![0; SECTOR];
    watch.dma_read(queue::DATA_OFF, &mut peek).unwrap();
    assert_ne!(peek, secret, "a broken device can still read the client's plaintext");
    // The same for a flush and a read: neither touches the device either.
    assert_eq!(disk.flush(), Err(DeviceError::Broken));
    assert_eq!(disk.read(64, &mut out), Err(DeviceError::Broken));
}

/// **A11.** `avail.flags` used to be written once at queue setup, so a device that scribbled
/// `VIRTQ_AVAIL_F_NO_INTERRUPT` into it kept it that way for ever — and the claim that everything
/// the device is told is written fresh before every request was narrowly false.
#[test]
fn avail_flags_is_written_again_before_every_request() {
    let device = device();
    device.set_policy(Policy { defer: true, scribble: Some(Scribble::Avail), ..Policy::default() });
    let mut disk = Disk::new(&device).expect("bring-up");
    let mut out = vec![0; SECTOR];
    // The scribble happens while this request is in flight; whatever it answers, the next request
    // must put the flags back.
    let _ = disk.read(64, &mut out);
    device.set_policy(Policy { defer: true, ..Policy::default() });
    if !disk.is_broken() {
        assert_eq!(disk.read(64, &mut out), Ok(()));
    }
    let mut flags = [0; 2];
    device.dma_read(queue::AVAIL_OFF, &mut flags).unwrap();
    assert_eq!(u16::from_le_bytes(flags), 0, "avail.flags was left as the device wrote it");
}

/// **A1.** A device that re-asserts its interrupt the instant it is acknowledged, on a machine
/// whose clock has failed. `now_us` used to read a failed `time_now` as 0, which put the deadline
/// ten seconds into a past that never arrives, so the request never timed out at all. It now
/// saturates, so a clock that cannot be read fails the request immediately — fail closed, and
/// bounded either way.
#[test]
fn an_interrupt_storm_is_bounded_even_when_the_clock_has_failed() {
    let device = device();
    let watch = Watch::new(&device);
    let mut disk = Disk::new(&watch).expect("bring-up");
    device.set_policy(Policy { defer: true, never_complete: true, ..Policy::default() });
    watch.storm.set(true);
    watch.clock.set(Some(u64::MAX)); // what `kernel.rs` reports when `time_now` fails
    let before = watch.ops.get();
    let err = disk.read(64, &mut vec![0; SECTOR]).unwrap_err();
    let spun = watch.ops.get() - before;
    assert_eq!(err, DeviceError::Timeout, "a dead clock must fail the request, not delete the deadline");
    assert!(spun < 100, "the request spun {spun} times before giving up");
    assert!(disk.is_broken());
}

/// The same storm with a clock that works: the ten-second deadline bounds it, and `blkd` wakes
/// rather than sleeps while it waits. That is the stated residual, here as a case so the bound is
/// the deadline and not something else.
#[test]
fn an_interrupt_storm_with_a_working_clock_ends_at_the_deadline() {
    let device = device();
    let watch = Watch::new(&device);
    let mut disk = Disk::new(&watch).expect("bring-up");
    device.set_policy(Policy { defer: true, never_complete: true, ..Policy::default() });
    watch.storm.set(true);
    // A wake costs 100 ms on this imaginary machine, so a hundred of them reach the deadline.
    watch.clock.set(Some(1));
    watch.wake_cost.set(100_000);
    let before = watch.ops.get();
    let err = disk.read(64, &mut vec![0; SECTOR]).unwrap_err();
    assert_eq!(err, DeviceError::Timeout);
    assert!(watch.ops.get() - before < watch.cap, "only the test's own cap stopped it");
    // The clock, not a counter, is what ended it: it advanced by the whole deadline.
    assert!(watch.clock.get().unwrap() >= 10_000_000, "it gave up before the deadline");
    assert!(disk.is_broken());
}

/// **The editor's item 1.** A badge names a GPT **entry**, so a gap in the table does not
/// renumber the volumes after it. With entry 0 unused, badge 1 names nothing and badge 2 is the
/// partition in entry 1 — the volume the manifest meant.
#[test]
fn a_gap_in_the_table_does_not_renumber_the_volumes_after_it() {
    let mut image = Image::new(
        SECTORS,
        &[Entry { first_lba: 64, last_lba: 1063 }, Entry { first_lba: 2048, last_lba: 4095 }],
    );
    // Blank entry 0's type GUID: it becomes an unused entry, and entry 1 keeps its place.
    let at = redoubt_blkd::image::ARRAY_LBA as usize * SECTOR;
    image.bytes[at..at + 16].fill(0);
    image.refresh_array_crc();

    let device = FakeDevice::with_image(image.bytes);
    device.set_policy(Policy { defer: true, ..Policy::default() });
    let mut disk = Disk::new(&device).expect("bring-up");
    let roots = read_partitions(&mut disk).expect("a table with a gap is still a table");
    assert_eq!(roots.len(), redoubt_blkd::image::ENTRIES as usize, "one slot per entry, holes and all");
    assert_eq!(roots[0], None, "the unused entry answers to nothing");
    assert_eq!(roots[1].map(|r| (r.first(), r.sectors())), Some((2048, 2048)));
    assert!(roots[2..].iter().all(Option::is_none));
}
