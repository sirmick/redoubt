//! A virtio-blk device that lies, driven by the fuzzer: every byte of the input picks what the
//! device does next. The claims, both asserted here:
//!
//! 1. the driver never panics, whatever the device says;
//! 2. the driver never gives the device an address outside the region the kernel gave it
//!    (`FakeDevice::strayed`).
#![no_main]

use libfuzzer_sys::fuzz_target;
use redoubt_blkd::fake::{FakeDevice, Policy, Scribble};
use redoubt_blkd::virtio::SECTOR_SIZE;
use redoubt_blkd::{Disk, read_partitions};

/// The bytes left, handed out one at a time; an exhausted input reads as zeros, so a short input
/// is an honest device rather than a refusal to run.
struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn u8(&mut self) -> u8 {
        match self.0.split_first() {
            Some((first, rest)) => {
                self.0 = rest;
                *first
            }
            None => 0,
        }
    }

    fn u32(&mut self) -> u32 { u32::from_le_bytes([self.u8(), self.u8(), self.u8(), self.u8()]) }

    fn u64(&mut self) -> u64 { u64::from(self.u32()) | (u64::from(self.u32()) << 32) }

    fn bit(&mut self) -> bool { self.u8() & 1 == 1 }

    /// A policy: mostly honest, because a device that fails bring-up tests very little.
    fn policy(&mut self) -> Policy {
        let lies = self.u32();
        let on = |n: u32| (lies >> n) & 1 == 1;
        Policy {
            magic: on(0).then(|| self.u32()),
            version: on(1).then(|| self.u32()),
            device_id: on(2).then(|| self.u32()),
            features: on(3).then(|| self.u64()),
            queue_num_max: on(4).then(|| self.u32()),
            capacity: on(5).then(|| self.u64()),
            config_never_settles: on(6),
            never_resets: on(7),
            status_drops_bits: on(8),
            queue_ready_stuck: on(9),
            queue_ready_before: on(10),
            used_idx_delta: on(11).then(|| self.u32() as u16),
            extra_used_entries: if on(12) { u16::from(self.u8() % 8) } else { 0 },
            used_id: on(13).then(|| self.u32()),
            used_len: on(14).then(|| self.u32()),
            blk_status: on(15).then(|| self.u8()),
            short_write: on(16),
            no_interrupt: on(17),
            never_complete: on(18),
            spurious_interrupts: if on(19) { u32::from(self.u8() % 4) } else { 0 },
            defer: on(20),
            scribble: on(21).then(|| match self.u8() % 5 {
                0 => Scribble::Descriptors,
                1 => Scribble::DescriptorLoop,
                2 => Scribble::Avail,
                3 => Scribble::Header,
                other => Scribble::Whole(other),
            }),
            scribble_on_every_read: on(22),
            keep_writing_data: on(23),
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let mut bytes = Bytes(data);
    let device = FakeDevice::new(512);
    device.set_policy(bytes.policy());
    let Ok(mut disk) = Disk::new(&device) else {
        assert_eq!(device.strayed(), 0);
        return;
    };
    // At most eight operations, so one input is bounded work.
    for _ in 0..8 {
        device.set_policy(bytes.policy());
        let sector = bytes.u64();
        let count = usize::from(bytes.u8() % 70);
        match bytes.u8() % 4 {
            0 => {
                let mut out = vec![0; count * SECTOR_SIZE as usize];
                let _ = disk.read(sector, &mut out);
            }
            1 => {
                let data = vec![bytes.u8(); count * SECTOR_SIZE as usize];
                let _ = disk.write(sector, &data);
            }
            2 => {
                let _ = disk.flush();
            }
            _ => {
                let _ = read_partitions(&mut disk);
            }
        }
        assert_eq!(device.strayed(), 0, "the driver named an address outside its DMA region");
        if bytes.bit() {
            break;
        }
    }
});
