//! The receive thread's loop, here rather than in the program so that host tests drive it: drain
//! the receive queue until it is empty, handing each frame on, then wait for the interrupt, and
//! again. The interrupt only says when to look: a lost one delays nothing, because the queue is
//! drained before every wait. (Transmit never waits on an interrupt at all: its queue is marked
//! `NO_INTERRUPT`, and the serving thread takes completed buffers back on each `transmit`.)
//!
//! **Every way out stops the device and says so** (servers/netd.md R57: either thread leaving
//! its loop resets). A lie, a fault reading the rings, or the interrupt handle failing all
//! end the loop the same way: the device is reset, so it writes nothing more into its receive
//! slots, and the serving thread is told, so it answers `failed` from then on instead of
//! transmitting on a device that nobody is receiving from. Only an interrupt that has not arrived
//! yet ([`Fault::Timeout`]) goes round again.

use redoubt_rt::abi::FOREVER;

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

/// Runs the receive thread until the device lies or its interrupt fails: each interrupt is
/// acknowledged, the queue drained into `scratch`, and every good frame given to `forward`. On the
/// way out it resets the device through `t` and calls `report` once, then returns why.
pub fn receive<T: Transport>(
    t: &T,
    rx: &mut RxQueue,
    scratch: &mut Frame,
    mut forward: impl FnMut(&[u8]),
    report: impl FnOnce(),
) -> Stopped {
    let stopped = 'run: loop {
        // Drain until the used ring is empty before every wait, the first one included (the
        // device may have completed buffers between DRIVER_OK and it), so an interrupt that was
        // never delivered cannot leave frames waiting: the last, empty drain is the re-check of
        // the used index right before blocking (on QEMU `virt` an interrupt raised while the IRQ
        // object was masked was lost once, on a driver's first receive: todo/irq-level-latch.md).
        loop {
            match virtio::ack_interrupt(t).and_then(|()| rx.drain(t, scratch, &mut forward)) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) => break 'run Stopped::Device(error),
            }
        }
        match t.wait_irq(FOREVER) {
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
