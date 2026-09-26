//! The driver against a hostile virtio-net device (servers/netd.md).
//!
//! Every test runs the real bring-up and the real queues against `redoubt_netd::fake::FakeNic`,
//! which lies on demand. Asserted throughout:
//!
//! 1. **No panic.** A panicking test fails, so every case is a no-panic case, and the randomized sweep at the
//!    end is many thousands more.
//! 2. **No address outside the regions, and no crossing.** `strayed` counts an address the driver wrote that
//!    is outside both regions, `crossed` one in the other queue's region.
//! 3. **A lie is a refusal; a bad frame is not.** A device that breaks the ring protocol gets an error; a
//!    frame of a length `netd` does not carry is dropped and counted, and the queue goes on working.

use redoubt_netd::fake::{FAKE_MAC, FakeNic, Policy, Scribble, exercise};
use redoubt_netd::receiver::{Stopped, receive};
use redoubt_netd::ring::{QUEUE_SIZE, SLOT_LEN};
use redoubt_netd::rxq::{Frame, RxQueue};
use redoubt_netd::txq::Sent;
use redoubt_netd::virtio::{DeviceError, MAX_FRAME, NET_HDR_LEN, TX_TIMEOUT_US, bit, feature, status};
use redoubt_netd::{Fault, Up, bring_up};

fn clean(nic: &FakeNic) {
    assert_eq!(nic.strayed(), 0, "an address outside both regions");
    assert_eq!(nic.crossed(), 0, "a queue named the other queue's region");
    assert_eq!(nic.overread(), 0, "a transmit named bytes past its slot");
}

fn up(nic: &FakeNic) -> Up { bring_up(&nic.rx_view(), &nic.tx_view()).expect("an honest device comes up") }

fn with(policy: Policy) -> FakeNic {
    let nic = FakeNic::new();
    nic.set_policy(policy);
    nic
}

/// A frame of `len` bytes whose content says which it is.
fn frame(len: usize, tag: u8) -> Vec<u8> { (0..len).map(|i| tag.wrapping_add(i as u8)).collect() }

/// Drains everything, returning the frames delivered.
fn drain(nic: &FakeNic, rx: &mut RxQueue) -> Result<Vec<Vec<u8>>, DeviceError> {
    let mut scratch: Frame = [0; SLOT_LEN];
    let mut got = Vec::new();
    rx.drain(&nic.rx_view(), &mut scratch, |f| got.push(f.to_vec()))?;
    Ok(got)
}

#[test]
fn an_honest_device_comes_up_with_two_features_and_its_mac() {
    let nic = FakeNic::new();
    let up = up(&nic);
    // Exactly VERSION_1 and MAC, whatever else was offered.
    assert_eq!(nic.driver_features(), bit(feature::VERSION_1) | bit(feature::MAC));
    let expected = FAKE_MAC.iter().rev().fold(0u64, |m, o| (m << 8) | u64::from(*o));
    assert_eq!(up.mac, expected);
    assert_ne!(nic.status() & status::DRIVER_OK, 0);
    assert_eq!(up.rx.outstanding(), u32::from(QUEUE_SIZE), "every receive slot offered");
    clean(&nic);
}

#[test]
fn bring_up_refuses_and_resets() {
    let lies: [(Policy, DeviceError); 14] = [
        (Policy { magic: Some(0), ..Default::default() }, DeviceError::NotNetDevice),
        (Policy { version: Some(1), ..Default::default() }, DeviceError::NotNetDevice),
        (Policy { device_id: Some(2), ..Default::default() }, DeviceError::NotNetDevice),
        (Policy { features: Some(bit(feature::VERSION_1)), ..Default::default() }, DeviceError::Features),
        (Policy { features: Some(bit(feature::MAC)), ..Default::default() }, DeviceError::Features),
        (Policy { never_resets: true, ..Default::default() }, DeviceError::NotNetDevice),
        (Policy { status_drops_bits: true, ..Default::default() }, DeviceError::NotNetDevice),
        (Policy { queue_num_max: Some(8), ..Default::default() }, DeviceError::Queue),
        (Policy { queue_ready_before: true, ..Default::default() }, DeviceError::Queue),
        (Policy { queue_ready_stuck: true, ..Default::default() }, DeviceError::Queue),
        (Policy { config_never_settles: true, ..Default::default() }, DeviceError::Config),
        (Policy { mac: Some([0; 6]), ..Default::default() }, DeviceError::Config),
        (Policy { mac: Some([0x01, 0, 0x5e, 0, 0, 1]), ..Default::default() }, DeviceError::Config),
        (Policy { mac: Some([0xff; 6]), ..Default::default() }, DeviceError::Config),
    ];
    for (policy, expected) in lies {
        let nic = with(policy);
        let got = bring_up(&nic.rx_view(), &nic.tx_view());
        assert_eq!(got.err(), Some(expected), "{policy:?}");
        // A device that would not come up is left reset (unless it will not reset at all).
        if !policy.never_resets {
            assert_eq!(nic.status(), 0, "{policy:?}");
        }
        clean(&nic);
    }
}

#[test]
fn frames_arrive_exactly_and_every_slot_comes_back() {
    let nic = FakeNic::new();
    let Up { mut rx, .. } = up(&nic);
    // Many more frames than slots, so the indices wrap and every slot is reused.
    for n in 0..300u32 {
        let len = 14 + (n as usize * 37) % (MAX_FRAME - 13);
        let sent = frame(len, n as u8);
        nic.arrive(&sent);
        assert_eq!(drain(&nic, &mut rx).unwrap(), vec![sent]);
        assert_eq!(rx.outstanding(), u32::from(QUEUE_SIZE));
    }
    // A burst of sixteen at once, drained together.
    let burst: Vec<Vec<u8>> = (0..16).map(|i| frame(60 + i, i as u8)).collect();
    for f in &burst {
        nic.arrive(f);
    }
    assert_eq!(drain(&nic, &mut rx).unwrap(), burst);
    assert_eq!(rx.delivered, 316);
    clean(&nic);
}

/// A frame of a length `netd` does not carry is content from some sender, not a lie: dropped,
/// counted, and the queue keeps working.
#[test]
fn a_bad_frame_from_the_wire_is_dropped_never_a_lie() {
    let nic = FakeNic::new();
    let Up { mut rx, .. } = up(&nic);
    for len in [0, 1, 13, 1515, 1518, SLOT_LEN - NET_HDR_LEN] {
        nic.arrive(&frame(len, 7));
        assert_eq!(drain(&nic, &mut rx).unwrap(), Vec::<Vec<u8>>::new(), "len {len}");
    }
    assert_eq!(rx.dropped, 6);
    let good = frame(64, 9);
    nic.arrive(&good);
    assert_eq!(drain(&nic, &mut rx).unwrap(), vec![good]);
    clean(&nic);
}

/// A device that says it wrote more than it did hands back zeros, never an earlier frame.
#[test]
fn an_inflated_length_reads_zeros_not_old_frames() {
    let nic = FakeNic::new();
    let Up { mut rx, .. } = up(&nic);
    // Fill every slot once with 0xee bytes, so any stale byte would show.
    for _ in 0..QUEUE_SIZE {
        nic.arrive(&[0xee; 1000]);
        drain(&nic, &mut rx).unwrap();
    }
    nic.set_policy(Policy { rx_used_len: Some((NET_HDR_LEN + 1000) as u32), ..Default::default() });
    nic.arrive(&[0x11; 20]);
    let got = drain(&nic, &mut rx).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(&got[0][..20], &[0x11; 20]);
    assert!(got[0][20..].iter().all(|b| *b == 0), "stale bytes from an earlier frame");
    clean(&nic);
}

#[test]
fn receive_lies_are_refused() {
    let lies = [
        Policy { rx_used_idx_delta: Some(2), ..Default::default() },
        Policy { rx_used_idx_delta: Some(u16::MAX), ..Default::default() },
        Policy { rx_used_idx_delta: Some(17), ..Default::default() },
        Policy { rx_extra_used: 1, ..Default::default() },
        Policy { rx_used_id: Some(u32::from(QUEUE_SIZE)), ..Default::default() },
        Policy { rx_used_id: Some(u32::MAX), ..Default::default() },
        Policy { rx_used_len: Some((NET_HDR_LEN - 1) as u32), ..Default::default() },
        Policy { rx_used_len: Some(SLOT_LEN as u32 + 1), ..Default::default() },
        Policy { header_flags: Some(1), ..Default::default() },
        Policy { header_gso: Some(1), ..Default::default() },
    ];
    for policy in lies {
        let nic = FakeNic::new();
        let Up { mut rx, .. } = up(&nic);
        nic.set_policy(policy);
        nic.arrive(&frame(64, 1));
        assert_eq!(drain(&nic, &mut rx), Err(DeviceError::Lie), "{policy:?}");
        clean(&nic);
    }
}

/// A buffer completed twice in one batch is a lie: its bit is clear the second time.
#[test]
fn a_buffer_completed_twice_is_a_lie() {
    let nic = FakeNic::new();
    let Up { mut rx, .. } = up(&nic);
    nic.set_policy(Policy { rx_duplicate_id: true, ..Default::default() });
    nic.arrive(&frame(64, 1));
    nic.arrive(&frame(64, 2));
    assert_eq!(drain(&nic, &mut rx), Err(DeviceError::Lie));
    clean(&nic);
}

/// Rewriting the descriptor table or the available ring changes nothing `netd` believes, and a
/// device that goes on writing a slot after completing it cannot change a frame already copied.
#[test]
fn scribbling_devices_change_nothing_netd_believes() {
    for scribble in [Scribble::Descriptors, Scribble::Avail] {
        let nic = FakeNic::new();
        let Up { mut rx, mut tx, .. } = up(&nic);
        nic.set_policy(Policy { scribble: Some(scribble), keep_writing: true, ..Default::default() });
        let sent = frame(100, 3);
        nic.arrive(&sent);
        // Whatever the device did, what was delivered is what was sent, or it was refused.
        match drain(&nic, &mut rx) {
            Ok(got) => assert!(got.is_empty() || got == vec![sent.clone()], "{scribble:?}"),
            Err(e) => assert_eq!(e, DeviceError::Lie, "{scribble:?}"),
        }
        let _ = tx.transmit(&nic.tx_view(), &frame(60, 4));
        clean(&nic);
    }
}

#[test]
fn a_transmit_sends_exactly_its_header_and_frame() {
    let nic = FakeNic::new();
    let Up { mut tx, .. } = up(&nic);
    let t = nic.tx_view();
    // A long frame, then short ones in the same slots: nothing of the long one may follow them.
    for (n, len) in [1514usize, 14, 60, 1000, 15].into_iter().cycle().take(50).enumerate() {
        let f = frame(len, n as u8);
        assert_eq!(tx.transmit(&t, &f), Ok(Sent::Queued));
        let wire = nic.wire();
        let last = wire.last().unwrap();
        assert_eq!(last.len(), NET_HDR_LEN + len, "the descriptor names exactly header and frame");
        assert_eq!(&last[..NET_HDR_LEN], &[0; NET_HDR_LEN]);
        assert_eq!(&last[NET_HDR_LEN..], &f[..]);
    }
    assert_eq!(tx.transmit(&t, &frame(13, 0)), Ok(Sent::BadLength));
    assert_eq!(tx.transmit(&t, &frame(1515, 0)), Ok(Sent::BadLength));
    clean(&nic);
}

#[test]
fn a_device_that_keeps_every_slot_is_busy_then_broken() {
    let nic = with(Policy { tx_never_complete: true, ..Default::default() });
    let Up { mut tx, .. } = up(&nic);
    let t = nic.tx_view();
    for _ in 0..QUEUE_SIZE {
        assert_eq!(tx.transmit(&t, &frame(60, 0)), Ok(Sent::Queued));
    }
    assert_eq!(tx.transmit(&t, &frame(60, 0)), Ok(Sent::Busy));
    nic.advance(TX_TIMEOUT_US);
    assert_eq!(tx.transmit(&t, &frame(60, 0)), Err(DeviceError::Timeout));
    clean(&nic);
}

#[test]
fn transmit_lies_are_refused() {
    let lies = [
        Policy { tx_used_id: Some(u32::from(QUEUE_SIZE)), ..Default::default() },
        Policy { tx_used_id: Some(5), ..Default::default() },
        Policy { tx_used_len: Some(SLOT_LEN as u32 + 1), ..Default::default() },
        Policy { tx_used_idx_delta: Some(3), ..Default::default() },
        Policy { tx_used_idx_delta: Some(u16::MAX), ..Default::default() },
    ];
    for policy in lies {
        let nic = FakeNic::new();
        let Up { mut tx, .. } = up(&nic);
        nic.set_policy(policy);
        let t = nic.tx_view();
        // The lie is published with the first completion and found when reclaiming.
        let first = tx.transmit(&t, &frame(60, 0));
        let second = tx.transmit(&t, &frame(60, 0));
        assert!(first == Err(DeviceError::Lie) || second == Err(DeviceError::Lie), "{policy:?}");
        clean(&nic);
    }
}

/// Spurious interrupts wake the receive thread for nothing, and nothing is delivered.
#[test]
fn spurious_interrupts_deliver_nothing() {
    let nic = with(Policy { spurious_interrupts: 3, ..Default::default() });
    let Up { mut rx, .. } = up(&nic);
    assert_eq!(drain(&nic, &mut rx).unwrap(), Vec::<Vec<u8>>::new());
    clean(&nic);
}

/// A small, fast, seeded generator: enough for a sweep, with no dependency.
struct Rng(u64);

impl Rng {
    fn byte(&mut self) -> u8 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 24) as u8
    }
}

#[test]
fn randomized_hostile_devices() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..20_000 {
        let nic = exercise(&mut || rng.byte());
        clean(&nic);
    }
}

/// A device that scribbles on the descriptor of each buffer it completes cannot break the queue:
/// every offer rewrites the descriptor whole, so once the device behaves again every frame
/// arrives, in both directions. (A driver that wrote descriptors only at bring-up would lose every
/// frame after the first sixteen here.)
#[test]
fn a_scribbled_descriptor_is_rewritten_when_offered_again() {
    let nic = with(Policy { scribble: Some(Scribble::Completed), ..Default::default() });
    let Up { mut rx, mut tx, .. } = up(&nic);
    let t = nic.tx_view();
    // Every slot of both queues is completed once, and scribbled on as it is.
    for n in 0..QUEUE_SIZE {
        let sent = frame(64, n as u8);
        nic.arrive(&sent);
        assert_eq!(drain(&nic, &mut rx).unwrap(), vec![sent]);
        assert_eq!(tx.transmit(&t, &frame(60, n as u8)), Ok(Sent::Queued));
    }
    nic.set_policy(Policy::default());
    for n in 0..4 * QUEUE_SIZE {
        let sent = frame(64 + usize::from(n), n as u8);
        nic.arrive(&sent);
        assert_eq!(drain(&nic, &mut rx).unwrap(), vec![sent], "frame {n} after the scribbling");
        let out = frame(60 + usize::from(n), n as u8);
        assert_eq!(tx.transmit(&t, &out), Ok(Sent::Queued));
        assert_eq!(&nic.wire().last().unwrap()[NET_HDR_LEN..], &out[..], "transmit {n} after the scribbling");
    }
    assert_eq!(nic.backlog(), 0, "no frame left waiting for a buffer the device could not use");
    assert_eq!(rx.outstanding(), u32::from(QUEUE_SIZE));
    assert_eq!(nic.wire().len(), 5 * usize::from(QUEUE_SIZE));
    clean(&nic);
}

/// Every ring counter is a `u16` that wraps (16 divides 65,536). More than 65,536 + 16 frames each
/// way take every counter, the driver's and the device's, past the wrap, with overflow checks on
/// (the test profile), so a counter that did not wrap would panic here.
#[test]
fn the_ring_counters_wrap() {
    const FRAMES: u32 = 65_536 + 16 + 100;
    let nic = FakeNic::new();
    let Up { mut rx, mut tx, .. } = up(&nic);
    let t = nic.tx_view();
    let mut scratch: Frame = [0; SLOT_LEN];
    for n in 0..FRAMES {
        let sent = frame(60, n as u8);
        nic.arrive(&sent);
        let mut got = 0;
        rx.drain(&nic.rx_view(), &mut scratch, |f| {
            assert_eq!(f, &sent[..], "frame {n}");
            got += 1;
        })
        .unwrap();
        assert_eq!(got, 1, "frame {n}");
        assert_eq!(tx.transmit(&t, &sent), Ok(Sent::Queued), "transmit {n}");
    }
    assert_eq!(rx.delivered, u64::from(FRAMES));
    assert_eq!(tx.sent, u64::from(FRAMES));
    assert_eq!(nic.wire().len(), FRAMES as usize);
    clean(&nic);
}

/// Runs the receive loop over `nic` until it stops, returning why, the frames it forwarded and
/// how many times it reported.
fn receive_until_stopped(nic: &FakeNic, rx: &mut RxQueue) -> (Stopped, Vec<Vec<u8>>, u32) {
    let mut scratch: Frame = [0; SLOT_LEN];
    let (mut got, mut reports) = (Vec::new(), 0);
    let stopped = receive(&nic.rx_view(), rx, &mut scratch, |f| got.push(f.to_vec()), || reports += 1);
    (stopped, got, reports)
}

/// The interrupt handle failing ends the receive thread, but never silently: the device is reset,
/// so no receive slot stays armed with nobody draining it, and the serving thread is told.
#[test]
fn the_receive_loop_resets_and_reports_when_the_interrupt_fails() {
    let nic = FakeNic::new();
    let Up { mut rx, .. } = up(&nic);
    let sent: Vec<Vec<u8>> = (0..3).map(|n| frame(80, n)).collect();
    for f in &sent {
        nic.arrive(f);
    }
    // One wait finds the frames, one finds nothing (a timeout: it waits again), then it fails.
    nic.fail_irq_after(2);
    let (stopped, got, reports) = receive_until_stopped(&nic, &mut rx);
    assert_eq!(stopped, Stopped::Interrupt(Fault::Kernel));
    assert_eq!(got, sent);
    assert_eq!(reports, 1, "the serving thread is told exactly once");
    assert_eq!(nic.status(), 0, "the device is reset: no receive slot is left armed");
    clean(&nic);
}

/// A buffer the device completed without an interrupt, before the receive loop first waits (the
/// first edge lost, as once on QEMU: todo/irq-level-latch.md), is still delivered: the loop drains
/// before it waits. The interrupt handle then fails at once, so without that drain nothing would
/// arrive.
#[test]
fn a_frame_completed_before_the_first_wait_without_an_interrupt_is_delivered() {
    let nic = FakeNic::new();
    let Up { mut rx, .. } = up(&nic);
    nic.complete_silently(1);
    let sent = frame(70, 9);
    nic.arrive(&sent);
    nic.fail_irq_after(0);
    let (stopped, got, reports) = receive_until_stopped(&nic, &mut rx);
    assert_eq!(stopped, Stopped::Interrupt(Fault::Kernel));
    assert_eq!(got, vec![sent]);
    assert_eq!(reports, 1);
    clean(&nic);
}

/// A frame completed without an interrupt while the loop drains after one (its edge lost) is
/// delivered before the loop waits again: it drains until the used ring is empty.
#[test]
fn a_frame_completed_silently_during_a_drain_is_delivered_before_the_next_wait() {
    let nic = FakeNic::new();
    let Up { mut rx, .. } = up(&nic);
    let (first, second) = (frame(64, 1), frame(66, 2));
    let mut scratch: Frame = [0; SLOT_LEN];
    let (mut got, mut reports) = (Vec::new(), 0);
    // The first frame arrives with its interrupt; one wait answers, the next fails.
    nic.arrive(&first);
    nic.fail_irq_after(1);
    let mut injected = false;
    let stopped = receive(
        &nic.rx_view(),
        &mut rx,
        &mut scratch,
        |f| {
            got.push(f.to_vec());
            if !injected {
                injected = true;
                nic.complete_silently(1);
                nic.arrive(&second);
            }
        },
        || reports += 1,
    );
    assert_eq!(stopped, Stopped::Interrupt(Fault::Kernel));
    assert_eq!(got, vec![first, second], "the silent completion was left behind");
    assert_eq!(reports, 1);
    clean(&nic);
}

/// A lie ends the receive thread the same way: reset, and reported once.
#[test]
fn the_receive_loop_resets_and_reports_a_lie() {
    for policy in [
        Policy { header_flags: Some(1), ..Default::default() },
        Policy { rx_used_idx_delta: Some(9), ..Default::default() },
        Policy { rx_duplicate_id: true, ..Default::default() },
    ] {
        let nic = FakeNic::new();
        let Up { mut rx, .. } = up(&nic);
        nic.set_policy(policy);
        nic.arrive(&frame(64, 1));
        nic.arrive(&frame(64, 2));
        // Bounded, should the lie ever go unnoticed.
        nic.fail_irq_after(8);
        let (stopped, _, reports) = receive_until_stopped(&nic, &mut rx);
        assert_eq!(stopped, Stopped::Device(DeviceError::Lie), "{policy:?}");
        assert_eq!(reports, 1, "{policy:?}");
        assert_eq!(nic.status(), 0, "{policy:?}");
        clean(&nic);
    }
}
