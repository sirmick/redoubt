//! What the program's threads ask `ipd` while they wait: a read or write `ipd`'s wait ran out on,
//! answered `timeout` (servers/ipd.md, "The `/net` tree"), is asked again; one refused `too many`,
//! every in-flight call of `sshd`'s share taken (the accept and each slot's parked read), is asked
//! again after a pause, a bounded number of times. Here, not in the program, so the host tests
//! drive it against the real `ipd`.

use redoubt_rt::client::{ClientError, Connection, Lend};
use redoubt_rt::wire::ninep::ErrorName;

/// How many `too many` refusals in a row a call is asked again after, [`RETRY_US`] apart: about
/// five seconds, after which the connection is given up.
pub const TOO_MANY_TRIES: u32 = 500;
/// The pause before a call refused `too many` is asked again, in µs.
pub const RETRY_US: u64 = 10_000;

/// After an `ipd` call that ended with `e`, the `tries`-th `too many` in a row before it: what to
/// do with the call.
#[derive(Debug, PartialEq, Eq)]
pub enum Again {
    /// `timeout`: ask again at once.
    Now,
    /// `too many`, fewer than [`TOO_MANY_TRIES`] times in a row: ask again after this many µs.
    After(u64),
    /// Anything else, or `too many` once too often: give up.
    No,
}

/// What to do after an `ipd` call ended with `e`: the listener's accept, or a read or write on a
/// connection's `data`.
pub fn again(e: &ClientError, tries: u32) -> Again {
    match e {
        ClientError::Rerror(ErrorName::Timeout) => Again::Now,
        ClientError::Rerror(ErrorName::TooMany) if tries < TOO_MANY_TRIES => Again::After(RETRY_US),
        _ => Again::No,
    }
}

/// A socket's `ctl` read at `ctl`, the socket's (state, number), asked again as [`again`] says:
/// on the listener, an accept. `None` when the read fails otherwise, too often, or is short.
pub fn status(ipd: &Connection, lend: &mut Lend, ctl: u32) -> Option<(u32, u32)> {
    let mut tries = 0;
    loop {
        let mut words = [0u8; 8];
        match ipd.read(lend, ctl, 0, &mut words) {
            Ok(8) => {
                let word =
                    |i: usize| u32::from_le_bytes([words[i], words[i + 1], words[i + 2], words[i + 3]]);
                return Some((word(0), word(4)));
            }
            Err(e) => match again(&e, tries) {
                Again::Now => continue,
                Again::After(us) => {
                    tries += 1;
                    let _ = redoubt_rt::handle::sleep(us);
                }
                Again::No => return None,
            },
            _ => return None,
        }
    }
}
