//! The client side of `tests/ninep-newconn-discard.toml`: it fills its own handle table, so the
//! kernel drops the capability in every `new_connection` reply `ninep-discard-server` sends it,
//! asks for more connections that way than its bucket may hold, then frees one slot and asks once
//! more, and gives that connection back. Its own lines are only a trace; the verdict is the
//! server's, given when the connection is given back, so this line always comes before it.

#![no_std]
#![no_main]

use redoubt_rt::abi::Handle;
use redoubt_rt::client::{Connection, Lend};
use redoubt_rt::handle::Endpoint;
use test_programs::rd;
use test_programs::{Logger, log};

redoubt_rt::panic_handler!();

/// Twice the server's connections per bucket: without the rollback the bucket fills halfway.
const DISCARDED: usize = 8;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    let server = Endpoint::from_handle(Handle::new(1).expect("slot 1"));
    // The lend first: it is memory, not a handle, and there is no room for anything later.
    let client = Connection::new(server);
    let mut lend = Lend::new(1).expect("a lend");
    let first = rd::endpoint_create().expect("room for one handle");
    let mut filled = 1;
    while rd::endpoint_create().is_ok() {
        filled += 1;
    }
    log!(logger, "[ninep-client] table full after {} endpoints", filled);
    for _ in 0..DISCARDED {
        if client.new_connection(&mut lend, "", 0).is_ok() {
            log!(logger, "[ninep-client] a capability arrived with the table full");
        }
    }
    rd::close(first).expect("close one");
    match client.new_connection(&mut lend, "", 0) {
        Ok((_, id)) => {
            log!(logger, "[ninep-client] connected with one slot free");
            let _ = client.disconnect(id, client.timeout);
        }
        Err(e) => log!(logger, "[ninep-client] refused with one slot free: {:?}", e),
    }
    test_programs::park()
}
