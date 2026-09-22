//! A virtio-blk device that lies, driven by the fuzzer: every byte of the input picks what the
//! device does next. The claims, both asserted here:
//!
//! 1. the driver never panics, whatever the device says;
//! 2. the driver never gives the device an address outside the region the kernel gave it
//!    (`FakeDevice::strayed`).
//!
//! The policy comes from `redoubt_blkd::fake::policy_from`, which the randomized sweep in
//! `tests/device.rs` uses too, so a lie cannot be exercised in one and missing from the other.
#![no_main]

use libfuzzer_sys::fuzz_target;
use redoubt_blkd::fake::{FakeDevice, policy_from};
use redoubt_blkd::virtio::SECTOR_SIZE;
use redoubt_blkd::{Disk, read_partitions};

/// The bytes left, handed out one at a time; an exhausted input reads as zeros, so a short input
/// is an honest device rather than a refusal to run.
pub struct Bytes<'a>(pub &'a [u8]);

impl Bytes<'_> {
    pub fn u8(&mut self) -> u8 {
        match self.0.split_first() {
            Some((first, rest)) => {
                self.0 = rest;
                *first
            }
            None => 0,
        }
    }

    pub fn u64(&mut self) -> u64 {
        let mut v = [0; 8];
        v.iter_mut().for_each(|b| *b = self.u8());
        u64::from_le_bytes(v)
    }
}

fuzz_target!(|data: &[u8]| {
    let mut bytes = Bytes(data);
    let device = FakeDevice::new(512);
    device.set_policy(policy_from(&mut || bytes.u8()));
    let Ok(mut disk) = Disk::new(&device) else {
        assert_eq!(device.strayed(), 0);
        return;
    };
    // At most eight operations, so one input is bounded work.
    for _ in 0..8 {
        device.set_policy(policy_from(&mut || bytes.u8()));
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
    }
});
