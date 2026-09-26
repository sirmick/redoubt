//! Bring-up: both queues configured, every receive slot offered, and the device told it may
//! start, all by one thread **before** the receive thread exists (servers/netd.md, "Two threads,
//! reset on exit": no message ever carries an address).

use crate::rxq::RxQueue;
use crate::transport::Transport;
use crate::txq::TxQueue;
use crate::virtio::{self, DeviceError};

/// A device that has come up: its MAC and its two queues, each to be driven only through its own
/// region's transport from here on.
#[derive(Debug)]
pub struct Up {
    pub mac: u64,
    pub rx: RxQueue,
    pub tx: TxQueue,
}

/// Resets the device and brings it up: identify, negotiate, read the MAC, configure the receive
/// queue in `rx`'s region and the transmit queue in `tx`'s, offer every receive slot, set
/// `DRIVER_OK`, then tell the device receive buffers are there.
///
/// `rx` and `tx` are two views of the same registers with different DMA regions. On any refusal
/// the device is reset before the error is returned, so a device `netd` would not bring up is
/// left doing nothing.
pub fn bring_up(rx: &impl Transport, tx: &impl Transport) -> Result<Up, DeviceError> {
    let up = try_bring_up(rx, tx);
    if up.is_err() {
        let _ = virtio::reset(tx);
    }
    up
}

fn try_bring_up(rx: &impl Transport, tx: &impl Transport) -> Result<Up, DeviceError> {
    virtio::identify(tx)?;
    virtio::negotiate(tx)?;
    let mac = virtio::mac(tx)?;
    let mut rxq = RxQueue::new();
    rxq.configure(rx)?;
    let mut txq = TxQueue::new();
    txq.configure(tx)?;
    rxq.offer_all(rx)?;
    virtio::driver_ok(tx)?;
    rxq.notify(rx)?;
    Ok(Up { mac, rx: rxq, tx: txq })
}
