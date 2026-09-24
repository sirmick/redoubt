//! The receive thread's loop, here rather than in the program so that host tests drive it: wait
//! for the interrupt, acknowledge it, drain the receive queue, and hand each frame on.
//!
//! **Every way out stops the device and says so** (IO-ARCHITECTURE.md, `netd`: either thread
//! leaving its loop resets). A lie, a fault reading the rings, or the interrupt handle failing all
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
    let stopped = loop {
        match t.wait_irq(FOREVER) {
            Ok(()) | Err(Fault::Timeout) => {}
            Err(fault) => break Stopped::Interrupt(fault),
        }
        let drained = virtio::ack_interrupt(t).and_then(|()| rx.drain(t, scratch, &mut forward));
        if let Err(error) = drained {
            break Stopped::Device(error);
        }
    };
    // A device that does not reset is still reported: the serving thread's own reset is tried
    // again, and it answers `failed` whatever happens.
    let _ = virtio::reset(t);
    report();
    stopped
}
