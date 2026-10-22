//! The receive thread's loop, here rather than in the program so that host tests drive it: drain
//! the receive queue until it is empty, handing each frame on, then wait for the interrupt, and
//! again. The last frame a drain finds goes on with the wait, in one call where the kernel has one
//! ([`Sink::forward_then_wait`]), so the receiver it readies runs on this hart. The interrupt only
//! says when to look: the queue is drained before every wait, and a frame the device completes
//! while that last frame is sent raises the interrupt the wait then takes (the source is level,
//! and a receive that unmasks it fires at once, kernel/devices.md R5). (Transmit never waits on an
//! interrupt at all: its queue is marked `NO_INTERRUPT`, and the serving thread takes completed
//! buffers back on each `transmit`.)
//!
//! **Every way out stops the device and says so** (servers/netd.md R57: either thread leaving
//! its loop resets). A lie, a fault reading the rings, or the interrupt handle failing all
//! end the loop the same way: the device is reset, so it writes nothing more into its receive
//! slots, and the serving thread is told, so it answers `failed` from then on instead of
//! transmitting on a device that nobody is receiving from. Only an interrupt that has not arrived
//! yet ([`Fault::Timeout`]) goes round again.

use redoubt_rt::abi::FOREVER;

use crate::ring::SLOT_LEN;
use crate::rxq::{Frame, RxQueue};
use crate::transport::{Fault, Transport};
use crate::virtio::{self, DeviceError};

/// Why the receive loop stopped. Either way the device has been reset and the stop reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stopped {
    /// The device broke the ring protocol, or the rings could not be read ([`RxQueue::drain`]).
    Device(DeviceError),
    /// Waiting for the interrupt failed: the handle is gone, or the kernel refused.
    Interrupt(Fault),
}

/// Where the receive thread's frames go.
pub trait Sink<T: Transport> {
    /// Hands one frame on.
    fn forward(&mut self, frame: &[u8]);

    /// Hands on `frame`, the last a drain found, then waits for the interrupt as
    /// [`Transport::wait_irq`] does. On the machine the two are one call, a send that waits
    /// (kernel/ipc.md, "A server's turn"); by default, in host tests, they are made in turn.
    fn forward_then_wait(&mut self, t: &T, frame: &[u8], timeout_us: u64) -> Result<(), Fault> {
        self.forward(frame);
        t.wait_irq(timeout_us)
    }
}

/// A closure hands each frame on; the last goes on before the wait, as two steps.
impl<T: Transport, F: FnMut(&[u8])> Sink<T> for F {
    fn forward(&mut self, frame: &[u8]) { self(frame) }
}

/// Runs the receive thread until the device lies or its interrupt fails: each interrupt is
/// acknowledged, the queue drained into `scratch`, and every good frame given to `sink`, the last
/// of each drain with the wait that follows it. On the
/// way out it resets the device through `t` and calls `report` once, then returns why.
pub fn receive<T: Transport>(
    t: &T,
    rx: &mut RxQueue,
    scratch: &mut Frame,
    mut sink: impl Sink<T>,
    report: impl FnOnce(),
) -> Stopped {
    // The frame held back until the next is found, and its length (0: none, as no frame is empty).
    let mut last: Frame = [0; SLOT_LEN];
    let stopped = 'run: loop {
        // Drain until the used ring is empty before every wait, the first one included (the
        // device may have completed buffers between DRIVER_OK and it), so an interrupt that was
        // never delivered cannot leave frames waiting: the last, empty drain is the re-check of
        // the used index right before the last frame is sent and the wait begins (on QEMU `virt`
        // an interrupt raised while the IRQ object was masked was lost once, on a driver's first
        // receive: todo/irq-level-latch.md).
        let mut held = 0;
        loop {
            let mut deliver = |frame: &[u8]| {
                if held != 0 {
                    sink.forward(&last[..held]);
                }
                last[..frame.len()].copy_from_slice(frame);
                held = frame.len();
            };
            match virtio::ack_interrupt(t).and_then(|()| rx.drain(t, scratch, &mut deliver)) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) => {
                    // The frames found before the lie go on, as they were found.
                    if held != 0 {
                        sink.forward(&last[..held]);
                    }
                    break 'run Stopped::Device(error);
                }
            }
        }
        let waited =
            if held == 0 { t.wait_irq(FOREVER) } else { sink.forward_then_wait(t, &last[..held], FOREVER) };
        match waited {
            Ok(()) | Err(Fault::Timeout) => {}
            Err(fault) => break Stopped::Interrupt(fault),
        }
    };
    // A device that does not reset is still reported: the serving thread's own reset is tried
    // again, and it answers `failed` whatever happens.
    let _ = virtio::reset(t);
    report();
    stopped
}
