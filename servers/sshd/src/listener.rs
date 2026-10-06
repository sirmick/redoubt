//! What the program's threads ask `ipd` while they wait: a read `ipd`'s wait ran out on, answered
//! `timeout` (servers/ipd.md, "The `/net` tree"), is asked again. Here, not in the program, so the
//! host tests drive it against the real `ipd`.

use redoubt_rt::client::{ClientError, Connection, Lend};
use redoubt_rt::wire::ninep::ErrorName;

/// Whether a read of `ipd`'s that ended with `e` is asked again: only `timeout`, the end of
/// `ipd`'s wait; any other error ends what the thread was waiting for.
pub fn asks_again(e: &ClientError) -> bool { matches!(e, ClientError::Rerror(ErrorName::Timeout)) }

/// A socket's `ctl` read at `ctl`, the socket's (state, number), asked again while `ipd`'s wait
/// runs out: on the listener, an accept. `None` when the read fails otherwise or is short.
pub fn status(ipd: &Connection, lend: &mut Lend, ctl: u32) -> Option<(u32, u32)> {
    loop {
        let mut words = [0u8; 8];
        match ipd.read(lend, ctl, 0, &mut words) {
            Ok(8) => {
                let word =
                    |i: usize| u32::from_le_bytes([words[i], words[i + 1], words[i + 2], words[i + 3]]);
                return Some((word(0), word(4)));
            }
            Err(e) if asks_again(&e) => continue,
            _ => return None,
        }
    }
}
