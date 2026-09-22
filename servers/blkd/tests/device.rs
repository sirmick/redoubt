//! The driver against a hostile virtio-blk device (BUILD-PLAN.md, WP-D1: "a hostile-device model
//! (malformed rings) never corrupts other memory or panics `blkd`").
//!
//! Every test here runs the real bring-up, the real queue and the real partition parser against
//! `redoubt_blkd::fake::FakeDevice`, which lies on demand. The three things asserted throughout:
//!
//! 1. **No panic.** A test that panics fails, so every case below is also a no-panic case, and the randomized
//!    sweep at the end is 100,000 more of them.
//! 2. **No address outside the region.** `FakeDevice::strayed` counts every physical address the driver gave
//!    the device that was not inside the region the device was given. It is asserted to be 0 after every
//!    case, which is the executable form of "`blkd` never asks the device to touch anything but the pages
//!    `dma_alloc` gave it".
//! 3. **A refusal, not silence.** A device that lies gets an error and is marked broken; it never returns
//!    data as if it were good.

use redoubt_blkd::fake::{FakeDevice, Policy, Scribble, policy_from};
use redoubt_blkd::image::{Entry, Image};
use redoubt_blkd::virtio::{DeviceError, MAX_SECTORS, SECTOR_SIZE, bit, feature};
use redoubt_blkd::{Disk, read_partitions};

const SECTORS: u64 = 8192;
const SECTOR: usize = SECTOR_SIZE as usize;

/// A device whose disk carries two partitions, behaving.
fn device() -> FakeDevice {
    let image = Image::new(
        SECTORS,
        &[Entry { first_lba: 64, last_lba: 1063 }, Entry { first_lba: 2048, last_lba: 4095 }],
    );
    let device = FakeDevice::with_image(image.bytes);
    device.set_policy(Policy { defer: true, ..Policy::default() });
    device
}

/// Brings `device` up, leaving whatever policy it has in place.
fn up(device: &FakeDevice) -> Result<Disk<&FakeDevice>, DeviceError> { Disk::new(device) }

// ------------------------------------------------------------------ the happy path

#[test]
fn a_block_round_trips() {
    let device = device();
    let mut disk = up(&device).expect("bring-up");
    assert_eq!(disk.sectors(), SECTORS);
    assert!(!disk.read_only());

    let payload: Vec<u8> = (0..SECTOR).map(|i| (i % 251) as u8).collect();
    disk.write(100, &payload).expect("write");
    disk.flush().expect("flush");
    let mut back = vec![0; SECTOR];
    disk.read(100, &mut back).expect("read");
    assert_eq!(back, payload);
    // It really reached the disk, not only the DMA buffer.
    assert_eq!(device.sector(100), payload);
    assert_eq!(device.strayed(), 0);
}

/// A run of the largest request the protocol allows, so the whole data buffer is exercised.
#[test]
fn a_full_run_of_sectors_round_trips() {
    let device = device();
    let mut disk = up(&device).expect("bring-up");
    let len = MAX_SECTORS as usize * SECTOR;
    let payload: Vec<u8> = (0..len).map(|i| (i * 7 % 253) as u8).collect();
    disk.write(1000, &payload).expect("write");
    let mut back = vec![0; len];
    disk.read(1000, &mut back).expect("read");
    assert_eq!(back, payload);
    assert_eq!(device.strayed(), 0);
}


/// The completion may already be there when the driver first looks (`defer` off) or arrive while
/// it waits (`defer` on). Both paths work, and neither is the only one tested.
#[test]
fn both_completion_paths_work() {
    for defer in [false, true] {
        let device = device();
        device.set_policy(Policy { defer, ..Policy::default() });
        let mut disk = up(&device).expect("bring-up");
        let mut back = vec![0; SECTOR];
        disk.read(0, &mut back).expect("read");
        assert_eq!(device.strayed(), 0);
    }
}

/// A device that reports a failure it is entitled to report fails the request and nothing more:
/// it is still speaking the protocol, so `blkd` goes on serving.
#[test]
fn a_reported_io_error_does_not_break_the_device() {
    let device = device();
    let mut disk = up(&device).expect("bring-up");
    device.set_policy(Policy { defer: true, blk_status: Some(1), ..Policy::default() });
    let mut back = vec![0; SECTOR];
    assert_eq!(disk.read(0, &mut back), Err(DeviceError::Rejected));
    assert!(!disk.is_broken());
    device.set_policy(Policy { defer: true, ..Policy::default() });
    assert_eq!(disk.read(0, &mut back), Ok(()));
}

// ------------------------------------------------------------------ bring-up refusals

/// Every one of these is a device that must not be brought up at all.
#[test]
fn a_device_that_is_not_one_is_refused_at_bring_up() {
    let cases: &[(&str, Policy, DeviceError)] = &[
        ("no virtio magic", Policy { magic: Some(0), ..P }, DeviceError::NotBlockDevice),
        ("legacy transport", Policy { version: Some(1), ..P }, DeviceError::NotBlockDevice),
        ("a network card", Policy { device_id: Some(1), ..P }, DeviceError::NotBlockDevice),
        ("will not reset", Policy { never_resets: true, ..P }, DeviceError::NotBlockDevice),
        ("drops status bits", Policy { status_drops_bits: true, ..P }, DeviceError::NotBlockDevice),
        ("offers nothing", Policy { features: Some(0), ..P }, DeviceError::Features),
        (
            "no flush, so `sync` could not be honoured",
            Policy { features: Some(bit(feature::VERSION_1)), ..P },
            DeviceError::Features,
        ),
        ("not a 1.x device", Policy { features: Some(bit(feature::BLK_FLUSH)), ..P }, DeviceError::Features),
        ("a queue too small for one chain", Policy { queue_num_max: Some(2), ..P }, DeviceError::Queue),
        ("a queue of no descriptors", Policy { queue_num_max: Some(0), ..P }, DeviceError::Queue),
        ("a queue that was never reset", Policy { queue_ready_before: true, ..P }, DeviceError::Queue),
        ("a queue it will not ready", Policy { queue_ready_stuck: true, ..P }, DeviceError::Queue),
        ("a capacity of nothing", Policy { capacity: Some(0), ..P }, DeviceError::Config),
        (
            "a capacity whose byte length overflows",
            Policy { capacity: Some(u64::MAX), ..P },
            DeviceError::Config,
        ),
        (
            "a configuration space that never settles",
            Policy { config_never_settles: true, ..P },
            DeviceError::Config,
        ),
    ];
    for (why, policy, expected) in cases {
        let device = device();
        device.set_policy(*policy);
        assert_eq!(up(&device).err(), Some(*expected), "{why}");
        assert_eq!(device.strayed(), 0, "{why}");
    }
}

/// A device that offers every feature there is gets back only the three `blkd` needs. Accepting
/// more would mean reading a ring that is not the one it wrote: `RING_INDIRECT_DESC` and
/// `RING_EVENT_IDX` both change the layout, and a driver that echoed the offer back would turn
/// them on without implementing either.
#[test]
fn only_the_features_we_need_are_accepted() {
    let device = device();
    device.set_policy(Policy { features: Some(u64::MAX), defer: true, ..P });
    let disk = up(&device).expect("a device offering everything still works");
    assert!(disk.read_only(), "BLK_RO was offered, so it is accepted and honoured");
    let wanted = bit(feature::VERSION_1) | bit(feature::BLK_FLUSH) | bit(feature::BLK_RO);
    assert_eq!(device.driver_features(), wanted);
}

/// The property the whole queue design rests on: the descriptor table and the available ring are
/// write-only, so a device that rewrites them changes **nothing**. The request still completes,
/// and with the right bytes.
#[test]
fn rewriting_the_rings_changes_nothing_the_driver_believes() {
    let cases: &[(&str, Scribble)] = &[
        ("descriptors full of out-of-range lengths and addresses", Scribble::Descriptors),
        ("a descriptor chain that points at itself", Scribble::DescriptorLoop),
        ("an available ring full of 0xffff", Scribble::Avail),
        ("a rewritten request header", Scribble::Header),
    ];
    let payload: Vec<u8> = (0..SECTOR).map(|i| (i % 241) as u8).collect();
    for (why, what) in cases {
        let device = device();
        let mut disk = up(&device).expect("bring-up");
        disk.write(200, &payload).expect("a write before the device turns");
        device.set_policy(Policy { scribble: Some(*what), defer: true, ..P });
        let mut back = vec![0; SECTOR];
        assert_eq!(disk.read(200, &mut back), Ok(()), "{why}");
        assert_eq!(back, payload, "{why}");
        assert!(!disk.is_broken(), "{why}");
        assert_eq!(device.strayed(), 0, "{why}");
    }
}

/// `Policy::default()` in the const position the array above needs.
const P: Policy = Policy {
    magic: None,
    version: None,
    device_id: None,
    features: None,
    queue_num_max: None,
    capacity: None,
    config_never_settles: false,
    never_resets: false,
    status_drops_bits: false,
    queue_ready_stuck: false,
    queue_ready_before: false,
    used_idx_delta: None,
    extra_used_entries: 0,
    used_id: None,
    used_len: None,
    blk_status: None,
    short_write: false,
    no_interrupt: false,
    never_complete: false,
    spurious_interrupts: 0,
    defer: true,
    scribble: None,
    scribble_on_every_read: false,
    keep_writing_data: false,
};

// ------------------------------------------------------------------ hostile completions

/// The heart of the acceptance criterion. Each case is a device that has been brought up
/// honestly and then turns: it lies about a completion. Every one must fail the request, mark
/// the device broken, and touch nothing.
#[test]
fn a_device_that_lies_about_a_completion_is_refused_and_never_spoken_to_again() {
    let cases: &[(&str, Policy)] = &[
        ("a used index that did not move", Policy { used_idx_delta: Some(0), ..P }),
        ("a used index that jumped", Policy { used_idx_delta: Some(2), ..P }),
        ("a used index that went backwards", Policy { used_idx_delta: Some(u16::MAX), ..P }),
        ("a used index that wrapped the ring", Policy { used_idx_delta: Some(4), ..P }),
        ("completions for requests never sent", Policy { extra_used_entries: 3, ..P }),
        ("a descriptor id we never submitted", Policy { used_id: Some(1), ..P }),
        ("a descriptor id outside the ring", Policy { used_id: Some(u32::MAX), ..P }),
        ("a length larger than the buffers given", Policy { used_len: Some(u32::MAX), ..P }),
        ("a status byte the protocol does not define", Policy { blk_status: Some(0xff), ..P }),
        ("a status byte of 3", Policy { blk_status: Some(3), ..P }),
        ("rings rewritten between our own reads", Policy { scribble_on_every_read: true, ..P }),
        ("a device that never completes", Policy { never_complete: true, ..P }),
        ("interrupts with nothing behind them", Policy { never_complete: true, spurious_interrupts: 4, ..P }),
    ];
    for (why, policy) in cases {
        let device = device();
        let mut disk = up(&device).expect("bring-up is honest");
        device.set_policy(*policy);
        let mut back = vec![0; SECTOR];
        let first = disk.read(0, &mut back);
        assert!(first.is_err(), "{why}: the lie was accepted");
        assert!(disk.is_broken(), "{why}: the device is still trusted");
        // Broken stays broken, whatever the device does next.
        device.set_policy(Policy { defer: true, ..P });
        assert_eq!(disk.read(0, &mut back), Err(DeviceError::Broken), "{why}");
        assert_eq!(disk.write(0, &back.clone()), Err(DeviceError::Broken), "{why}");
        assert_eq!(disk.flush(), Err(DeviceError::Broken), "{why}");
        assert_eq!(device.strayed(), 0, "{why}");
    }
}

/// Some lies are not protocol violations, so they are not refused; what matters is that they
/// cannot make the driver read or write outside its own buffers.
#[test]
fn lies_that_are_not_protocol_violations_are_still_harmless() {
    let cases: &[(&str, Policy)] = &[
        ("writes half the data it was asked for", Policy { short_write: true, ..P }),
        ("goes on writing after completing", Policy { keep_writing_data: true, ..P }),
        ("rewrites the request header", Policy { scribble: Some(Scribble::Header), ..P }),
        ("sets every byte of the region to 0xff", Policy { scribble: Some(Scribble::Whole(0xff)), ..P }),
        ("zeroes every byte of the region", Policy { scribble: Some(Scribble::Whole(0)), ..P }),
        ("completes without an interrupt", Policy { no_interrupt: true, ..P }),
        ("wakes us for nothing first", Policy { spurious_interrupts: 3, ..P }),
    ];
    for (why, policy) in cases {
        let device = device();
        let mut disk = up(&device).expect("bring-up");
        device.set_policy(*policy);
        let mut back = vec![0; MAX_SECTORS as usize * SECTOR];
        // Whatever it answers, it answers something, and it did not reach outside the region.
        let _ = disk.read(0, &mut back);
        let _ = disk.write(0, &vec![7; SECTOR]);
        let _ = disk.flush();
        assert_eq!(device.strayed(), 0, "{why}");
    }
}

/// A device that writes fewer bytes than it was given hands back zeros, not what the last
/// request left in the buffer. `blkd` never passes one client's data to another, and that does
/// not rest on the device behaving.
#[test]
fn a_short_write_reads_back_as_zeros_not_as_the_last_requests_bytes() {
    let device = device();
    let mut disk = up(&device).expect("bring-up");
    // A write leaves a whole sector of 0xaa in the DMA data buffer.
    let secret = vec![0xaa; SECTOR];
    disk.write(300, &secret).expect("write");
    device.set_policy(Policy { short_write: true, ..P });
    let mut back = vec![0xff; SECTOR];
    disk.read(400, &mut back).expect("the read itself succeeds");
    assert!(!back.contains(&0xaa), "the last request's bytes came back");
    assert!(back[SECTOR / 2..].iter().all(|b| *b == 0), "the unwritten half is not zeroed");
    assert_eq!(device.strayed(), 0);
}

/// A request that cannot be one is refused before the device sees it, and does not break the
/// device: a client asking for too much must not cost anyone else the disk.
#[test]
fn impossible_requests_never_reach_the_device() {
    let device = device();
    let mut disk = up(&device).expect("bring-up");
    let before = device.notified();
    let mut nothing = vec![];
    let mut odd = vec![0; SECTOR + 1];
    let mut huge = vec![0; (MAX_SECTORS as usize + 1) * SECTOR];
    assert_eq!(disk.read(0, &mut nothing), Err(DeviceError::Range));
    assert_eq!(disk.read(0, &mut odd), Err(DeviceError::Range));
    assert_eq!(disk.read(0, &mut huge), Err(DeviceError::Range));
    assert_eq!(disk.read(SECTORS, &mut vec![0; SECTOR]), Err(DeviceError::Range));
    assert_eq!(disk.read(u64::MAX, &mut vec![0; SECTOR]), Err(DeviceError::Range));
    assert_eq!(disk.write(SECTORS - 1, &vec![0; 2 * SECTOR]), Err(DeviceError::Range));
    assert_eq!(device.notified(), before, "the device was asked to do something");
    assert!(!disk.is_broken());
}

/// A read-only device refuses writes at the door, and its flush is a no-op rather than a lie.
#[test]
fn a_read_only_device_refuses_writes() {
    let device = device();
    device.set_policy(Policy {
        features: Some(bit(feature::VERSION_1) | bit(feature::BLK_FLUSH) | bit(feature::BLK_RO)),
        defer: true,
        ..P
    });
    let mut disk = up(&device).expect("bring-up");
    assert!(disk.read_only());
    let before = device.notified();
    assert_eq!(disk.write(0, &vec![0; SECTOR]), Err(DeviceError::ReadOnly));
    assert_eq!(disk.flush(), Ok(()));
    assert_eq!(device.notified(), before);
    assert!(!disk.is_broken());
    // Reading still works.
    assert_eq!(disk.read(0, &mut vec![0; SECTOR]), Ok(()));
}

// ------------------------------------------------------------------ the randomized sweep

/// 100,000 devices that lie at random, each driven through bring-up and a handful of requests.
/// The assertion is the one the work package asks for: no panic, and no address outside the
/// region. It runs in `cargo test`, so it is not a fuzz target nobody starts; `fuzz/` has the
/// coverage-guided version of the same driver.
#[test]
fn a_hundred_thousand_random_liars_never_panic_and_never_stray() {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut word = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    // `policy_from` is the one place a hostile device is built at random; the fuzz targets draw
    // from the same function, so a lie cannot be exercised here and missing there.
    let mut bytes = || (word() & 0xff) as u8;
    for _ in 0..100_000 {
        let device = FakeDevice::new(512);
        device.set_policy(policy_from(&mut bytes));
        let Ok(mut disk) = up(&device) else {
            assert_eq!(device.strayed(), 0);
            continue;
        };
        device.set_policy(policy_from(&mut bytes));
        let mut back = vec![0; SECTOR];
        let _ = disk.read(u64::from(bytes()) * 3, &mut back);
        device.set_policy(policy_from(&mut bytes));
        let _ = disk.write(u64::from(bytes()) * 3, &vec![bytes(); SECTOR]);
        device.set_policy(policy_from(&mut bytes));
        let _ = disk.flush();
        device.set_policy(policy_from(&mut bytes));
        let _ = read_partitions(&mut disk);
        assert_eq!(device.strayed(), 0);
    }
}
