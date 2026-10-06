//! Test-only, for the bench's `littlefsd-one-volume` (feature `one-volume-probe`, off in every default
//! build): R47 (one volume per instance) tried from inside `littlefsd` before it serves, as a parser
//! exploit would try it. Each answer is the kernel's or `blkd`'s, never this code's opinion: a
//! handle index other than its own is empty, its range's badge mints nothing, and the sector past
//! its range is `out_of_range`.

use alloc::format;
use alloc::string::String;
use core::num::NonZeroU64;

use redoubt_rt::abi::{Error, Handle};
use redoubt_rt::startup::Startup;
use redoubt_rt::wire::proto::blkd::ErrorCode;

use crate::blkd::Blkd;
use crate::volume::Range;

/// The handle indices tried: far past any a startup block carries.
const TRIED: u32 = 1024;

/// What the instance found, as its console line: `name` and its verdict.
pub fn verdict(startup: &Startup, name: &str, range: &mut Blkd) -> String {
    match tried(startup, name, range) {
        Ok(sectors) => format!(
            "{name} reaches only its endpoint, its range and its console: its range mints nothing, and sector {sectors} is out_of_range\n"
        ),
        Err(why) => format!("{name} FAILED R47: {why}\n"),
    }
}

/// Every handle index but its endpoint, its range and its console is closed: the kernel must
/// find nothing there. Then a badge is minted from the range, which the kernel must refuse, and
/// the sector past the range read, which `blkd` must refuse.
fn tried(startup: &Startup, name: &str, range: &mut Blkd) -> Result<u64, String> {
    let console = startup.namespace().find(|(path, _)| *path == "/dev/cons").map(|(_, h)| h);
    let own = [startup.handle(name), startup.handle("volume"), console];
    for index in 1..TRIED {
        let Some(handle) = Handle::new(index) else { continue };
        if own.contains(&Some(handle)) {
            continue;
        }
        match redoubt_rt::handle::close(handle) {
            Err(Error::BadHandle) => {}
            Ok(()) => return Err(format!("it held handle {index}")),
            Err(e) => return Err(format!("closing handle {index}: {e:?}")),
        }
    }
    for badge in 1..=4 {
        let badge = NonZeroU64::new(badge).expect("from 1");
        match range.endpoint().mint(badge, None) {
            Err(Error::NotPermitted) => {}
            other => return Err(format!("minting badge {badge} from its range: {other:?}")),
        }
    }
    let sectors = range.info().map_err(|_| String::from("its range has no size"))?.sectors;
    match range.read_one(sectors) {
        Ok(Err(ErrorCode::OutOfRange)) => Ok(sectors),
        other => Err(format!("reading sector {sectors}, past its range: {other:?}")),
    }
}
