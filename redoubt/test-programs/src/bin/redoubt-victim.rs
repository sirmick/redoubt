//! The victim of the Redoubt IPC attack case (WP-K2): it holds the boot endpoint's receive
//! right, which `redoubt-attack` tries to take, and keeps serving afterwards. The verdict is
//! this program's, not the attacker's: it still receives on a right the attacker could not
//! steal, and it reports to `attack-checker`, which powers the machine off.
//!
//! It also checks what only a receiver can: a `send` is never an open call, so neither `reply`
//! nor `mint` may name its message id (R4a).
//!
//! See `redoubt/tests/redoubt-ipc-attack.toml`.

#![no_std]
#![no_main]

use test_programs::rd::{self, FOREVER, MessageKind, Received};
use test_programs::redoubt_ipc::op;
use test_programs::{Logger, checker, log};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    log!(logger, "[victim] holding the boot endpoint's receive right");
    let mut calls = 0usize;
    let mut sends = 0usize;
    loop {
        let received = match rd::receive(Some(rd::BOOT_ENDPOINT), FOREVER, 1) {
            Ok(received) => received,
            Err(error) => {
                log!(logger, "[victim] receive -> {:?}", error);
                test_programs::park()
            }
        };
        let m = match received {
            Received::Message(m) => m,
            Received::Abandoned(id) => {
                rd::reply(id.get(), &rd::body([0; rd::WORDS])).ok();
                continue;
            }
            other => {
                log!(logger, "[victim] unexpected {:?}", other);
                continue;
            }
        };
        let id = m.msg_id.get();
        if let MessageKind::Send { .. } = m.kind {
            sends += 1;
            if sends == 1 {
                // R4a: a `send` is never an open call, so its id names nothing to reply to or
                // to mint from, even here, where the message really did arrive.
                let replied = rd::reply(id, &rd::body([0; rd::WORDS]));
                let minted = rd::mint_from_message(id, 1, None);
                log!(logger, "[victim] a send's id: reply {:?}, mint {:?}", replied, minted);
            }
            continue;
        }
        calls += 1;
        match m.body.words[0] {
            op::DONE => {
                // Still receiving, on a right nobody could take, after every attempt.
                log!(logger, "[victim] served {} calls and {} sends after the attack", calls, sends);
                rd::reply(id, &rd::body([op::DONE, 0, 0, 0])).ok();
                // A last round trip proves the endpoint still works both ways.
                log!(logger, "[victim] the receive right is still mine");
                checker::done();
                test_programs::park()
            }
            _ => {
                rd::reply(id, &rd::body([m.body.words[1] + 1, m.badge as usize, 0, 0])).ok();
            }
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
