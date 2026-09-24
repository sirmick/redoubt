//! `netd`, the program: two threads over one virtio-net device (IO-ARCHITECTURE.md, `netd`).
//!
//! - **The serving thread** maps the registers, allocates both DMA regions, brings the device up
//!   (both queues configured, every receive slot offered, `DRIVER_OK`) and only then starts the
//!   receive thread. It owns the transmit queue and answers `netif` calls from its one client.
//!   It never waits on anything but its own `receive`.
//! - **The receive thread** takes its half ([`RxPart`]) out of this process's own memory, waits on
//!   the interrupt, drains the receive queue and `send`s each frame to `ipd`, one page transferred
//!   per frame, giving up after [`SEND_TIMEOUT_US`]: a frame `ipd` cannot take is dropped, as on
//!   any wire. It tells the serving thread only that the device is broken, on a badge drawn at
//!   random above 2^63 that carries no data.
//!
//! **Every exit `netd` controls resets the device first** (status 0, read back), so the device
//! stops touching its rings before its pages can return to the pool. A kill or a fault runs none
//! of this code; closing that is the kernel's (answer 173, WP-K5b), and until then `netd` is not
//! restarted.
//!
//! **Pending on WP-R3.** `init` does not exist yet; the `tests/net` rig starts this program
//! through the stub with the startup block `init` will write.

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use core::num::NonZeroU64;

use redoubt_netd::kernel::{self, Device, Regs};
use redoubt_netd::rxq::Frame;
use redoubt_netd::{BROKEN, FIRST_MINTED_BADGE, NetServer, RxPart, bring_up, parse_client, virtio};
use redoubt_rt::abi::{Error, FOREVER, MemFlags, PAGE_SIZE};
use redoubt_rt::handle::{Endpoint, Irq, Mmio};
use redoubt_rt::ipc::{Buffer, Event};
use redoubt_rt::startup::Startup;
use redoubt_rt::wire::proto::ipd::{Frame as FrameMsg, Message as IpdMessage};

redoubt_rt::entry!(serve);

/// The startup block named no endpoint `netd` for it to receive on.
pub const NO_ENDPOINT: u32 = 2;
/// `receive` failed for a reason other than the endpoint going away.
pub const RECEIVE_FAILED: u32 = 3;
/// The startup block lacks `net`, `net-irq` or `ipd`, or the device would not map or give DMA
/// pages. A driver with no device does not start.
pub const NO_DEVICE: u32 = 5;
/// The arguments are not exactly `client=BADGE` ([`parse_client`]).
pub const BAD_ARGS: u32 = 6;
/// The kernel would give no random word for the broken badge, or no stack for the receive thread.
pub const NO_RESOURCES: u32 = 7;

/// The startup-block names of `netd`'s handles (INIT.md; question 149's `NAME` / `NAME-irq`).
pub const ENDPOINT: &str = "netd";
pub const NET: &str = "net";
pub const NET_IRQ: &str = "net-irq";
pub const IPD: &str = "ipd";

/// How long the receive thread waits for `ipd` to take a frame before dropping it.
pub const SEND_TIMEOUT_US: u64 = 50_000;
/// The receive thread's stack.
const RX_STACK: usize = 8 * PAGE_SIZE;

/// The receive thread: never returns.
extern "C" fn rx_thread(_arg: usize) -> ! {
    if let Some(part) = kernel::take_rx_part() {
        receive_frames(part);
    }
    redoubt_rt::handle::thread_exit()
}

/// Drains the receive queue on every interrupt and sends each frame to `ipd`. Returns when the
/// device has lied (having reset it and told the serving thread) or the interrupt is gone.
fn receive_frames(part: RxPart) {
    let RxPart { device, mut rx, ipd, broken } = part;
    let mut scratch: Frame = [0; redoubt_netd::ring::SLOT_LEN];
    loop {
        use redoubt_netd::Transport;
        match device.wait_irq(FOREVER) {
            Ok(()) | Err(redoubt_netd::Fault::Timeout) => {}
            Err(_) => return,
        }
        let drained = virtio::ack_interrupt(&device).and_then(|()| {
            rx.drain(&device, &mut scratch, |frame| forward(&ipd, frame))
        });
        if drained.is_err() {
            let _ = virtio::reset(&device);
            // Waits: the serving thread always comes back to `receive`, and a lost report would
            // leave it transmitting on a device that has been reset.
            let _ = broken.send(&[BROKEN, 0, 0, 0], &[], None, FOREVER);
            return;
        }
    }
}

/// Sends one frame to `ipd` as its `frame` message: a `send`, the frame in one transferred page.
/// A frame `ipd` does not take within [`SEND_TIMEOUT_US`] is dropped.
fn forward(ipd: &Endpoint, frame: &[u8]) {
    let Ok(mut page) = Buffer::new(1) else { return };
    let Ok(words) = IpdMessage::Frame(FrameMsg { frame }).encode(&mut page) else { return };
    // On failure the page comes back with the error and is unmapped when dropped.
    let _ = ipd.send(&words, &[], Some(page), SEND_TIMEOUT_US);
}

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    let Some(endpoint) = startup.handle(ENDPOINT).map(Endpoint::from_handle) else { return NO_ENDPOINT };
    let Some(client) = parse_client(startup.args()) else { return BAD_ARGS };
    let (Some(mmio), Some(irq), Some(ipd)) = (startup.handle(NET), startup.handle(NET_IRQ), startup.handle(IPD))
    else {
        return NO_DEVICE;
    };
    let mmio = Mmio::from_handle(mmio);
    let Ok(regs) = Regs::map(&mmio) else { return NO_DEVICE };
    let (Ok(rx_view), Ok(tx_view)) =
        (Device::new(regs, &mmio, Some(Irq::from_handle(irq))), Device::new(regs, &mmio, None))
    else {
        return NO_DEVICE;
    };
    // A device that will not come up leaves `netd` running and refusing: `failed` to every
    // request, and nothing for `init` to restart (IO-ARCHITECTURE.md, `netd`).
    let (mac, tx, rx) = match bring_up(&rx_view, &tx_view) {
        Ok(up) => (up.mac, up.tx, Some(up.rx)),
        Err(_) => (0, redoubt_netd::txq::TxQueue::new(), None),
    };
    let mut server = NetServer::new(tx_view, tx, mac, client);
    let broken_badge = match rx {
        Some(rx) => match start_receiving(&endpoint, rx_view, rx, Endpoint::from_handle(ipd)) {
            Ok(badge) => Some(badge),
            Err(code) => {
                server.break_device();
                return code;
            }
        },
        None => {
            server.break_device();
            None
        }
    };
    let exit = loop {
        match endpoint.receive(FOREVER, 0) {
            Ok(Event::Call(request)) => {
                // A failed reply means the caller is gone; there is nobody to tell.
                let _ = server.serve(request);
            }
            Ok(Event::Send(delivery)) => {
                for handle in delivery.handles.as_slice().iter().flatten() {
                    let _ = redoubt_rt::handle::close(*handle);
                }
                if Some(delivery.caller.badge) == broken_badge && delivery.words[0] == BROKEN {
                    server.break_device();
                }
            }
            Ok(Event::Interrupt | Event::Exit(_) | Event::Abandoned(_)) => {}
            Err(Error::Dead) => break redoubt_rt::exit::OK,
            Err(_) => break RECEIVE_FAILED,
        }
    };
    // Every exit `netd` controls stops the device first.
    server.break_device();
    exit
}

/// Starts the receive thread with its half and returns the badge its broken-device report will
/// arrive on.
fn start_receiving(endpoint: &Endpoint, device: Device, rx: redoubt_netd::rxq::RxQueue, ipd: Endpoint) -> Result<u64, u32> {
    let random = redoubt_rt::handle::random_u64().map_err(|_| NO_RESOURCES)?;
    // Above 2^63, where no manifest badge is, so it can never be the client's.
    let badge = FIRST_MINTED_BADGE | random;
    let broken = NonZeroU64::new(badge).ok_or(NO_RESOURCES).and_then(|b| endpoint.mint(b, None).map_err(|_| NO_RESOURCES))?;
    let stack = redoubt_rt::handle::map_anon(RX_STACK, MemFlags::READ | MemFlags::WRITE).map_err(|_| NO_RESOURCES)?;
    // The stack grows down from the top of the mapping, which is page-aligned.
    kernel::start_rx_thread(RxPart { device, rx, ipd, broken }, rx_thread, stack + RX_STACK).map_err(|_| NO_RESOURCES)?;
    Ok(badge)
}
