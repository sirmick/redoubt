//! The console's changes of size (docs/userland/beamlet.md, "beamlet on Redoubt"): one thread keeps a
//! `consol` `resize` call out at the console's server, which parks it until the window changes size
//! (servers/consoled.md, "The `consol` protocol"), and calls again for the next change.
//!
//! - **Started by the first console read**, so a VM that never reads its console asks nothing. It lives as
//!   long as the VM: its stack is spent once ([`redoubt_rt::thread::spawn`]), and its one call out holds one
//!   of the console's parked calls in the connection's share, the cost the console server states.
//! - **The size goes through an atomic** the two share, the newest one only: a change the VM has not taken
//!   yet is replaced by the next, since only the size now matters. The thread then wakes the VM on its own
//!   endpoint through a badge minted for it ([`BADGE`]), and [`Platform::idle`] returns.
//! - **It ends** when the call fails: a server that does not serve `consol` (malformed), or one that has
//!   gone. The console then never resizes, as on a UART.
//!
//! [`Platform::idle`]: beamlet_vm::platform::Platform::idle

use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};

use redoubt_client::{Lend, typed};
use redoubt_rt::abi::{FOREVER, Handle};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Delivery;
use redoubt_rt::wire::proto::consol::{self, Message, Reply, Resize};

use crate::system::worker;

/// The badge of the resize thread's wake-ups: above every call thread's (`pool::BADGE` and up).
pub const BADGE: u64 = 0x200;

/// The word that says the size has changed.
const RESIZED: u64 = 0x5_12e;

/// The thread's stack, in pages.
const STACK_PAGES: usize = 4;

/// The console's changes of size, as the thread reports them.
#[derive(Default)]
pub(crate) struct Resizes {
    /// The newest size the thread has seen, `cols << 16 | rows`; 0 until it has seen one.
    size: Option<Arc<AtomicU32>>,
    /// A change the VM has not taken yet.
    pending: Option<(u16, u16)>,
}

impl Resizes {
    /// Starts the thread, once, on the console's server `console`. One that cannot start leaves a
    /// console that never resizes.
    pub(crate) fn start(&mut self, wake: &Endpoint, console: Handle) {
        if self.size.is_some() {
            return;
        }
        let size = Arc::new(AtomicU32::new(0));
        let shared = Arc::clone(&size);
        if worker(wake, BADGE, STACK_PAGES, move |_, done| watch(console, &shared, &done)).is_ok() {
            self.size = Some(size);
        }
    }

    /// Takes the thread's wake-up; anything else is handed back.
    pub(crate) fn deliver(&mut self, delivery: Delivery) -> Option<Delivery> {
        if delivery.caller.badge != BADGE || delivery.words[0] != RESIZED {
            return Some(delivery);
        }
        let packed = self.size.as_ref().map_or(0, |s| s.load(Ordering::SeqCst));
        if packed != 0 {
            self.pending = Some(((packed >> 16) as u16, packed as u16));
        }
        None
    }

    /// Whether a change waits for the VM.
    pub(crate) fn pending(&self) -> bool { self.pending.is_some() }

    /// The change that waits, if any, taken.
    pub(crate) fn take(&mut self) -> Option<(u16, u16)> { self.pending.take() }
}

/// The thread: a `resize` call at a time on `console`, each new size put in `size` and said on
/// `done`, until a call or a wake-up fails.
fn watch(console: Handle, size: &AtomicU32, done: &Endpoint) {
    let Ok(mut lend) = Lend::new(1) else { return };
    let console = Endpoint::from_handle(console);
    loop {
        let resized = typed::call::<consol::Protocol, _>(
            &console,
            &mut lend,
            &Message::Resize(Resize {}),
            &[],
            |r, _| match r {
                Reply::Resize(r) => Some((r.cols, r.rows)),
                _ => None,
            },
        );
        let Ok(Some((cols, rows))) = resized else { return };
        // A side of 0 is no size, and 0 is "none yet": a server never answers one.
        if cols == 0 || rows == 0 {
            return;
        }
        size.store(u32::from(cols) << 16 | u32::from(rows), Ordering::SeqCst);
        if done.send(&[RESIZED, 0, 0, 0], &[], None, FOREVER).is_err() {
            return;
        }
    }
}
