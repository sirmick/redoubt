//! Callers for the `MAX_OPEN_CALLS` part of the Redoubt IPC case. Each blocked caller
//! holds exactly one open call in the server, and one process cannot hold `MAX_OPEN_CALLS`
//! threads, so the count is made up from this program, `redoubt-client` and the server's own
//! threads. These threads never return: the server never replies to `op::KEEP`.
//!
//! See `tests/redoubt-ipc.toml`.

#![no_std]
#![no_main]

use test_programs::rd::{self, FOREVER};
use test_programs::redoubt_ipc::op;

fn caller(_arg: usize) {
    rd::call_waiting(rd::BOOT_ENDPOINT, &rd::body([op::KEEP, 0, 0, 0]), None, FOREVER).ok();
    test_programs::park()
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Every thread this process can have, each offering one call.
    while rd::thread(caller, 0).is_ok() {}
    caller(0);
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
