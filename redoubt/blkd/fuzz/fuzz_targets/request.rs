//! Arbitrary bytes as a typed request to the `blkd` server, from an arbitrary badge and label
//! set. The claim: every one is answered — with a reply or an error code — and none panics,
//! makes the server hold state it was not asked for, or reaches a sector outside the range the
//! badge names.
#![no_main]

use libfuzzer_sys::fuzz_target;
use redoubt_blkd::fake::{FakeDevice, Policy};
use redoubt_blkd::image::{Entry, Image};
use redoubt_blkd::server::{BUDGET, BlockServer, COST, LIMITS, answer_with};
use redoubt_blkd::{Disk, read_partitions};
use redoubt_rt::abi::{Error, Handle, Labels, ReceivedHandles};
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::minted::Minter;
use std::num::NonZeroU64;

/// The disk every run starts from: two partitions, the second right after the first.
const FIRST: Entry = Entry { first_lba: 64, last_lba: 1063 };
const SECOND: Entry = Entry { first_lba: 2048, last_lba: 4095 };
const SECTORS: u64 = 8192;

struct FakeKernel {
    next_handle: u32,
    rng: u64,
}

impl Minter for FakeKernel {
    fn mint(&mut self, _badge: NonZeroU64) -> Result<Handle, Error> {
        self.next_handle = self.next_handle.wrapping_add(1).max(1);
        Handle::new(self.next_handle).ok_or(Error::OutOfMemory)
    }

    fn random(&mut self) -> Result<u64, Error> {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        Ok(self.rng)
    }
}

fuzz_target!(|data: &[u8]| {
    // 4 words of 8 bytes, a badge, an account and a label: 43 bytes of header, then the body.
    if data.len() < 43 {
        return;
    }
    let word = |i: usize| u64::from_le_bytes(data[i * 8..i * 8 + 8].try_into().expect("eight bytes"));
    let words = [word(0), word(1), word(2), word(3)];
    let badge = word(4);
    let account = u64::from(data[40]);
    let labels = if data[41] & 1 == 1 { Labels::from_slice(&[u64::from(data[42])]).unwrap() } else { Labels::new() };
    let body = &data[43..];

    let image = Image::new(SECTORS, &[FIRST, SECOND]);
    let device = FakeDevice::with_image(image.bytes);
    device.set_policy(Policy { defer: true, ..Policy::default() });
    let mut disk = Disk::new(&device).expect("the honest device comes up");
    let roots = read_partitions(&mut disk).expect("the image carries a table");
    let mut server =
        BlockServer::new(disk, roots, LIMITS, &COST, BUDGET, 0x1234_5678).expect("limits that fit");
    let mut kernel = FakeKernel { next_handle: 100, rng: 0x2545_f491_4f6c_dd1d };

    // The lend the caller made: 64 KiB, `MAX_LEND_PAGES` (WIRE.md).
    let mut buf = vec![0u8; 64 * 1024];
    let n = body.len().min(buf.len());
    buf[..n].copy_from_slice(&body[..n]);
    let caller = Caller { badge, account, labels };
    let _ = answer_with(&mut server, &caller, &words, &ReceivedHandles::new(), &mut buf, &mut kernel);

    // Whatever it answered, it stayed inside its own region, and it granted at most the one
    // capability a single request can grant.
    assert_eq!(device.strayed(), 0);
    assert!(server.granted() <= 1);
});
