//! Attacker against the Redoubt IPC rules (WP-K2). It holds one badged handle to the boot
//! endpoint, which `redoubt-victim` receives on, and nothing else.
//!
//! What it tries: to receive on a badged handle (steal a receive right, I4); to mint the receive
//! right itself, badge 0 (I3); to mint from handles and message ids it does not hold; to reply
//! to and `serve` calls that are not its own; to lend memory that is not its, that is not
//! writable, and that it already transferred away; to transfer more than the victim asked for;
//! and to flood the endpoint. Each attempt's error is printed as progress, so a refusal for the
//! wrong reason fails the case; the verdict is the victim's and the checker's power-off, which
//! a broken kernel never reaches.
//!
//! See `redoubt/tests/redoubt-ipc-attack.toml`.

#![no_std]
#![no_main]

use test_programs::rd::{self, Error, FOREVER};
use test_programs::redoubt_ipc::op;
use test_programs::{Logger, log};

const E: u32 = rd::BOOT_ENDPOINT;

fn echo(arg: usize) -> Result<usize, Error> {
    rd::call_waiting(E, &rd::body([op::ECHO, arg, 0, 0]), None, FOREVER).map(|r| r.words[0])
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    log!(logger, "[attacker] starting");
    // The victim must be listening before the rest means anything.
    let hello = echo(1);
    log!(logger, "[attacker] the victim answers: {:?}", hello);

    // --- The receive right (I4) ---------------------------------------------------------------
    let rights = [
        // Receiving on a badged handle: the one thing a badge can never buy.
        rd::receive(Some(E), 0, 0).err(),
        // Minting the receive right itself: a badge of 0. It goes in raw registers, because
        // `Call::Mint`'s badge is a `NonZeroU64` and a typed call would never leave this
        // program. The kernel refuses it while decoding, and again when it mints (I3).
        rd::mint_raw(2, E as usize, 0, 0, 0),
        // Minting at all from a handle that is not a receive right.
        rd::mint_from_handle(E, 1, None).err(),
        // Minting from handles this program does not hold.
        rd::mint_from_handle(2, 1, None).err(),
        rd::mint_from_handle(u32::MAX, 1, None).err(),
        // Narrowing into a budget it does not hold either.
        rd::mint_from_handle(E, 1, Some(2)).err(),
    ];
    log!(logger, "[attacker] receive rights -> {:?}", rights);

    // --- Messages that are not this thread's (R4a) --------------------------------------------
    let ids = [
        rd::mint_from_message(1, 1, None).err(),
        rd::mint_from_message(u64::MAX, 1, None).err(),
        rd::reply(1, &rd::body([0; rd::WORDS])).err(),
        rd::reply(u64::MAX, &rd::body([0; rd::WORDS])).err(),
        rd::serve(1).err(),
        rd::serve(u64::MAX).err(),
    ];
    log!(logger, "[attacker] foreign message ids -> {:?}", ids);

    // --- Buffers (R4, R11) ---------------------------------------------------------------------
    let page = rd::page();
    let text = _start as *const () as usize & !0xfff;
    let gone = rd::page();
    rd::send(E, &rd::body([0, 1, 0, 0]), rd::pages(gone, 1), FOREVER).ok();
    // One round trip, so the victim has served and reported that send before this program says
    // anything more: it answers messages in order, and its report is a blocking lend.
    echo(2).ok();
    let buffers = [
        // Memory that is not this program's at all.
        rd::call(E, &rd::body([op::ECHO, 0, 0, 0]), rd::pages(0x4000, 1), FOREVER).err(),
        // This program's code: mapped, but a lend must be writable.
        rd::call(E, &rd::body([op::ECHO, 0, 0, 0]), rd::pages(text, 1), FOREVER).err(),
        // Not page-aligned.
        rd::call(E, &rd::body([op::ECHO, 0, 0, 0]), rd::pages(page + 8, 1), FOREVER).err(),
        // Over `MAX_LEND_PAGES`.
        rd::call(E, &rd::body([op::ECHO, 0, 0, 0]), rd::pages(page, rd::MAX_LEND_PAGES + 1), FOREVER).err(),
        // A page it gave away with the transfer above.
        rd::call(E, &rd::body([op::ECHO, 0, 0, 0]), rd::pages(gone, 1), FOREVER).err(),
        // More pages than the victim's `receive` asked for: the range is this program's own
        // and its budget could pay, so only R4's `max_transfer` refuses it.
        rd::send(E, &rd::body([0, 2, 0, 0]), rd::pages(rd::many_pages(2), 2), FOREVER).err(),
    ];
    log!(logger, "[attacker] buffers -> {:?}", buffers);

    // --- A flood: every call polled, none of it holding the victim up -------------------------
    let mut refused = 0;
    for _ in 0..2000 {
        if rd::call(E, &rd::body([op::ECHO, 0, 0, 0]), None, 0).is_err() {
            refused += 1;
        }
    }
    log!(logger, "[attacker] flood: {} of 2000 refused", refused);
    // The victim still answers this program, after all of it.
    log!(logger, "[attacker] the victim still answers: {:?}", echo(5));
    log!(logger, "[attacker] attempts done");
    rd::call_waiting(E, &rd::body([op::DONE, 0, 0, 0]), None, FOREVER).ok();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
