//! A **sequence** of arbitrary typed requests against one long-lived `blkd`, from arbitrary
//! badges and label sets, over a device drawn from the same input. The claims:
//!
//! 1. every request is answered — with a reply or an error code — and none panics;
//! 2. no address outside the DMA region is ever named (`FakeDevice::strayed`);
//! 3. nothing a client sends makes `blkd` grow: it holds one buffer and one range table, and a
//!    sequence of any length leaves both the size they started.
//!
//! A sequence rather than one request, because the state that could go wrong is what one request
//! leaves for the next: the DMA buffer, the reply scratch, and whether the device has been marked
//! broken.
#![no_main]

use libfuzzer_sys::fuzz_target;
use redoubt_blkd::fake::{FakeDevice, policy_from};
use redoubt_blkd::image::{Entry, Image};
use redoubt_blkd::server::{BlockServer, answer_with};
use redoubt_blkd::{Disk, read_partitions};
use redoubt_rt::abi::{Labels, ReceivedHandles};
use redoubt_rt::ipc::Caller;

/// The disk every run starts from: two partitions, so a badge can name one, the other, an unused
/// entry, or nothing.
const FIRST: Entry = Entry { first_lba: 64, last_lba: 1063 };
const SECOND: Entry = Entry { first_lba: 2048, last_lba: 4095 };
const SECTORS: u64 = 8192;
/// The lend a caller makes: `MAX_LEND_PAGES`, 64 KiB (WIRE.md).
const LEND: usize = 64 * 1024;

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

    fn u64(&mut self) -> u64 {
        let mut v = [0; 8];
        v.iter_mut().for_each(|b| *b = self.u8());
        u64::from_le_bytes(v)
    }

    fn done(&self) -> bool { self.0.is_empty() }
}

fuzz_target!(|data: &[u8]| {
    let mut bytes = Bytes(data);
    let image = Image::new(SECTORS, &[FIRST, SECOND]);
    let device = FakeDevice::with_image(image.bytes);
    device.set_policy(policy_from(&mut || bytes.u8()));
    // Bring-up and the table are done against whatever device the input chose; a device too
    // hostile to come up leaves nothing to serve, which is itself the right answer.
    let Ok(mut disk) = Disk::new(&device) else {
        assert_eq!(device.strayed(), 0);
        return;
    };
    let Ok(roots) = read_partitions(&mut disk) else {
        assert_eq!(device.strayed(), 0);
        return;
    };
    let entries = roots.len();
    let mut server = BlockServer::new(disk, roots);

    let mut buf = vec![0u8; LEND];
    // At most 32 requests, so one input is bounded work.
    for _ in 0..32 {
        if bytes.done() {
            break;
        }
        // The device may turn between any two requests.
        device.set_policy(policy_from(&mut || bytes.u8()));
        let words = [bytes.u64(), bytes.u64(), bytes.u64(), bytes.u64()];
        let badge = match bytes.u8() % 4 {
            0 => 1,                    // GPT entry 0
            1 => 2,                    // GPT entry 1
            2 => u64::from(bytes.u8()), // often an unused entry, sometimes past the end
            _ => bytes.u64(),
        };
        let account = u64::from(bytes.u8() % 3);
        let labels = match bytes.u8() % 3 {
            0 => Labels::new(),
            n => Labels::from_slice(&[u64::from(n)]).expect("one label fits"),
        };
        // The body is whatever is left of the input, capped at the lend.
        let len = usize::from(bytes.u8()) * 256;
        buf.iter_mut().for_each(|b| *b = 0);
        for i in 0..len.min(LEND) {
            buf[i] = bytes.u8();
        }
        let caller = Caller { badge, account, labels };
        let _ = answer_with(&mut server, &caller, &words, &ReceivedHandles::new(), &mut buf);

        assert_eq!(device.strayed(), 0, "an address outside the DMA region was named");
        assert_eq!(server.roots().len(), entries, "the range table changed size");
    }
});
