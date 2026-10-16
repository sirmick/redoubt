//! `crossing`, the program: one item read from, or written to, the volume connection its startup
//! block holds, for the one call the steward makes on the endpoint it handed over
//! (servers/steward.md, "Declassification and push").
//!
//! Its startup block holds `serve`, the receive right of the endpoint the steward calls (the
//! steward made it, so the labelled program never starts a flow: kernel/ipc.md R1), and
//! `volume`, a 9P connection at the volume's root, minted in the program's own budget, below
//! which the program walks to the item; its one argument is the mode, `read` or `write`. It
//! answers the call and exits; a hang ends at its budget's deadline.

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use alloc::vec::Vec;

use redoubt_crossing::{Crossing, CrossingProtocol, Mode, Refused, Volume};
use redoubt_rt::abi::{Error, FOREVER, MAX_LEND_PAGES};
use redoubt_rt::client::{Connection as Nine, Lend};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::server::close_delivery;
use redoubt_rt::server::ninep::mode;
use redoubt_rt::server::typed::serve_call;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// The names in the startup block: the endpoint served, and the volume connection.
pub const SERVE: &str = "serve";
pub const VOLUME: &str = "volume";

/// The startup block named no endpoint or no volume.
pub const NO_HANDLE: u32 = 2;
/// The argument is neither `read` nor `write`.
pub const BAD_MODE: u32 = 4;
/// The lend or the volume's attach could not be made.
pub const NO_START: u32 = 5;

/// The fid the item is walked to, from the root at 0.
const ROOT: u32 = 0;
const ITEM: u32 = 1;

/// The volume connection: plain 9P calls, one fid, no state held between the one call's steps.
struct Nine9P {
    nine: Nine,
    lend: Lend,
}

impl Volume for Nine9P {
    fn read(&mut self, path: &str, max: usize) -> Result<Vec<u8>, Refused> {
        let (nine, lend) = (&self.nine, &mut self.lend);
        nine.walk(lend, ROOT, ITEM, path).map_err(|_| Refused)?;
        let result = (|| {
            nine.open(lend, ITEM, mode::OREAD).map_err(|_| Refused)?;
            // The first `max` bytes: an item cut here is over the bound, which the core says.
            let mut out = Vec::new();
            out.try_reserve(max).map_err(|_| Refused)?;
            out.resize(max, 0);
            let mut done = 0;
            while done < out.len() {
                match nine.read(lend, ITEM, done as u64, &mut out[done..]) {
                    Ok(0) => break,
                    Ok(n) => done += n,
                    Err(_) => return Err(Refused),
                }
            }
            out.truncate(done);
            Ok(out)
        })();
        let _ = nine.clunk(lend, ITEM);
        result
    }

    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), Refused> {
        let (nine, lend) = (&self.nine, &mut self.lend);
        // The item made in its directory if it is not there, truncated if it is: the next push
        // overwrites a partial one (servers/steward.md, "Declassification and push"). A failed
        // walk leaves its fid unused (intro(5)), so the item's fid walks again to the directory.
        let (dir, name) = match path.rsplit_once('/') {
            Some((dir, name)) => (dir, name),
            None => ("", path),
        };
        let opened = match nine.walk(lend, ROOT, ITEM, path) {
            Ok(_) => nine.open(lend, ITEM, mode::OWRITE | mode::OTRUNC).map(|_| ()),
            Err(_) => nine
                .walk(lend, ROOT, ITEM, dir)
                .and_then(|_| nine.create(lend, ITEM, name, 0o644, mode::OWRITE))
                .map(|_| ()),
        };
        if opened.is_err() {
            return Err(Refused);
        }
        let mut done = 0;
        let mut result = Ok(());
        while done < bytes.len() {
            match nine.write(lend, ITEM, done as u64, &bytes[done..]) {
                Ok(0) | Err(_) => {
                    result = Err(Refused);
                    break;
                }
                Ok(n) => done += n,
            }
        }
        let _ = nine.clunk(lend, ITEM);
        result
    }
}

/// Serves the one call and exits.
pub fn serve(startup: &Startup) -> u32 {
    let (Some(serve), Some(volume)) = (startup.handle(SERVE), startup.handle(VOLUME)) else {
        return NO_HANDLE;
    };
    let Some(mode) = startup.args().next().and_then(Mode::parse) else { return BAD_MODE };
    let Ok(mut lend) = Lend::new(1) else { return NO_START };
    let nine = Nine::new(Endpoint::from_handle(volume));
    if nine.version(&mut lend).and_then(|_| nine.attach(&mut lend, ROOT, "")).is_err() {
        return NO_START;
    }
    let mut crossing = Crossing::new(mode, Nine9P { nine, lend });
    let endpoint = Endpoint::from_handle(serve);
    loop {
        match endpoint.receive(FOREVER, MAX_LEND_PAGES) {
            Ok(Event::Call(request)) => {
                // A reply that does not go says the steward is gone: nothing more to do.
                if serve_call::<CrossingProtocol, _>(&mut crossing, request).is_err() || crossing.served() {
                    return redoubt_rt::exit::OK;
                }
            }
            Ok(Event::Send(delivery)) => close_delivery(&delivery),
            Ok(_) => {}
            Err(Error::Dead) => return redoubt_rt::exit::OK,
            Err(_) => return redoubt_rt::exit::RECEIVE_FAILED,
        }
    }
}
