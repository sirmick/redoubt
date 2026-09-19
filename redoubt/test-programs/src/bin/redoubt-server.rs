//! The server side of the Redoubt IPC case (WP-K2): it holds the boot endpoint's receive right
//! (badge 0, handle 1) and answers `redoubt-client`, which drives the script and prints the
//! verdicts. Everything this program reports about a message -- the badge, the account, the
//! labels, the message id -- is what the kernel attached, which is the point: no client can
//! forge any of it.
//!
//! See `redoubt/tests/redoubt-ipc.toml`.

#![no_std]
#![no_main]

use test_programs::rd::{self, FOREVER, MessageKind, Received};
use test_programs::redoubt_ipc::op;
use test_programs::{Logger, log};

/// The most pages this server takes in a transfer. `op::MAX_TRANSFER` sets it, so that the
/// client can offer a transfer this server never asked for (R4).
static mut MAX_TRANSFER: usize = 4;
/// A handle to this server's own endpoint, badged, so its filler threads can call it.
static mut SELF_HANDLE: u32 = 0;

fn max_transfer() -> usize {
    // SAFETY: only this program's main thread reads or writes it; the filler threads do not.
    unsafe { core::ptr::read_volatile(&raw const MAX_TRANSFER) }
}

/// A filler thread: one call this server will park, so that `MAX_OPEN_CALLS` can be reached
/// (each blocked caller holds exactly one open call).
fn filler(_arg: usize) -> ! {
    // SAFETY: written by the main thread before any filler thread is created.
    let handle = unsafe { core::ptr::read_volatile(&raw const SELF_HANDLE) };
    rd::call_waiting(handle, &rd::body([op::KEEP, 0, 0, 0]), None, FOREVER).ok();
    test_programs::park()
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = Logger::connect();
    // The kernel gave this program, the bundle's second, the receive right (badge 0).
    log!(logger, "[server] holding the boot endpoint");
    let mut parked = [0u64; rd::MAX_OPEN_CALLS];
    let mut nparked = 0usize;
    let mut notices = 0usize;
    // I15 says a notice is delivered *once*. Replying at once would hide a second one, since
    // the reply frees the call; so the first notice is held back over one more `receive`, and
    // a repeat of it is a failure this server can see.
    let (mut deferred, mut deferred_seen) = (0u64, 0usize);
    let mut sends = 0usize;
    let mut last_id = 0u64;
    // The `MAX_OPEN_CALLS` step: one caller thread of this server's own at a time, until its
    // process holds as many open calls as it may. One process cannot hold that many threads, so
    // `redoubt-client` and `redoubt-filler` supply the rest.
    let (mut filling, mut pending, mut announced) = (false, false, false);
    loop {
        if filling && !pending && nparked < rd::MAX_OPEN_CALLS {
            // SAFETY: only this thread writes it, and only before any filler is created.
            unsafe { core::ptr::write_volatile(&raw mut SELF_HANDLE, self_handle(&mut logger)) };
            if xous::create_thread_1(filler, 0).is_ok() {
                pending = true;
            } else {
                log!(logger, "[server] out of threads with {} parked", nparked);
                filling = false;
            }
        }
        if nparked >= rd::MAX_OPEN_CALLS && !announced {
            // The verdict for R4a: this server's process holds every open call it may.
            log!(logger, "[server] MAX_OPEN_CALLS reached: {} open calls", nparked);
            announced = true;
            filling = false;
        }
        let received = match rd::receive(Some(rd::BOOT_ENDPOINT), FOREVER, max_transfer()) {
            Ok(received) => received,
            Err(error) => {
                log!(logger, "[server] receive -> {:?}", error);
                test_programs::park()
            }
        };
        let m = match received {
            Received::Abandoned(id) => {
                // R3, I15: reported once, and the call stays open until this reply frees it.
                notices += 1;
                if deferred == 0 {
                    // Held back: this call stays open, so the kernel could offer it again.
                    deferred = id.get();
                    deferred_seen = 1;
                    continue;
                }
                if id.get() == deferred {
                    deferred_seen += 1;
                    log!(logger, "[server] abandoned {} reported {} times, FAIL", deferred, deferred_seen);
                    continue;
                }
                if rd::reply(id.get(), &rd::body([0; rd::WORDS])).is_err() {
                    log!(logger, "[server] abandoned {} -> reply FAILED", id.get());
                }
                // A second reply must find nothing: the call is gone.
                if rd::reply(id.get(), &rd::body([0; rd::WORDS])).is_ok() {
                    log!(logger, "[server] abandoned {} -> replied twice, FAIL", id.get());
                }
                // It was one of the calls this server held open; it is not any more.
                if let Some(at) = parked[..nparked].iter().position(|x| *x == id.get()) {
                    parked.copy_within(at + 1..nparked, at);
                    nparked -= 1;
                } else {
                    log!(logger, "[server] abandoned {} was never parked, FAIL", id.get());
                }
                continue;
            }
            Received::Message(m) => m,
            other => {
                log!(logger, "[server] unexpected {:?}", other);
                continue;
            }
        };
        // Anything but a notice means the held-back one was offered once and no more.
        if deferred != 0 {
            log!(logger, "[server] the held-back abandoned call was reported {} time(s)", deferred_seen);
            rd::reply(deferred, &rd::body([0; rd::WORDS])).ok();
            if let Some(at) = parked[..nparked].iter().position(|x| *x == deferred) {
                parked.copy_within(at + 1..nparked, at);
                nparked -= 1;
            }
            deferred = 0;
        }
        let id = m.msg_id.get();
        let words = m.body.words;
        // A `send` never owes a reply and never becomes an open call (R4a).
        if let MessageKind::Send { transfer } = m.kind {
            sends += 1;
            let word = transfer.map(rd::peek_pages);
            log!(
                logger,
                "[server] send {} badge {} tag {} transfer {:?} word {:?}",
                sends,
                m.badge,
                words[1],
                transfer.map(|t| t.npages.get()),
                word
            );
            continue;
        }
        let MessageKind::Call { lend } = m.kind else { continue };
        match words[0] {
            op::ECHO => {
                // Ids are never reused within this process (I12).
                let fresh = id != last_id;
                last_id = id;
                log!(
                    logger,
                    "[server] echo badge {} account {} labels {} fresh id {}",
                    m.badge,
                    m.account,
                    m.labels.as_slice().len(),
                    fresh
                );
                rd::reply(id, &rd::body([words[1] + 1, m.badge as usize, m.account as usize, 0])).ok();
            }
            op::LEND => {
                let Some(pages) = lend else {
                    log!(logger, "[server] lend: no buffer");
                    rd::reply(id, &rd::body([0; rd::WORDS])).ok();
                    continue;
                };
                // The lend is the client's page, mapped here and writable; the client sees the
                // change once `reply` gives it back (R3).
                let word = rd::peek(pages.addr);
                rd::poke(pages.addr, word + 1);
                log!(logger, "[server] lend {} pages, word {}", pages.npages.get(), word);
                rd::reply(id, &rd::body([pages.npages.get(), word as usize, 0, 0])).ok();
            }
            op::KEEP => {
                // Taken and not replied to: an open call (R4a).
                if nparked < parked.len() {
                    parked[nparked] = id;
                    nparked += 1;
                }
                pending = false;
            }
            op::SELF_FILL => {
                log!(logger, "[server] filling from {} open calls", nparked);
                filling = true;
                rd::reply(id, &rd::body([op::SELF_FILL, nparked, 0, 0])).ok();
            }
            op::COUNTS => {
                log!(logger, "[server] {} abandoned notices, {} parked, {} sends", notices, nparked, sends);
                rd::reply(id, &rd::body([op::COUNTS, notices, nparked, sends])).ok();
            }
            op::MAX_TRANSFER => {
                // SAFETY: as `max_transfer`; only this thread touches it.
                unsafe { core::ptr::write_volatile(&raw mut MAX_TRANSFER, words[1]) };
                log!(logger, "[server] max_transfer {}", words[1]);
                rd::reply(id, &rd::body([op::MAX_TRANSFER, words[1], 0, 0])).ok();
            }
            op::MINT_BACK => {
                // `mint` from an open call of this thread: a handle to the endpoint the call
                // arrived on, stamped as the handle it came through (R9).
                let minted = rd::mint_from_message(id, words[1] as u64, None);
                // And what no server may do: mint the receive right.
                let zero = rd::mint(rd::MintSource::Message(m.msg_id), 0, None);
                log!(logger, "[server] mint badge {} ok {}, badge 0 -> {:?}", words[1], minted.is_ok(), zero);
                let body = match minted {
                    Ok(handle) => rd::body_with([op::MINT_BACK, 0, 0, 0], &[handle]),
                    Err(_) => rd::body([op::MINT_BACK, 1, 0, 0]),
                };
                rd::reply(id, &body).ok();
            }
            op::REPLY_HANDLES => {
                // Handles the caller may not be able to take: each arrives as 0 in its slot and
                // the caller's `call` is `OutOfMemory` (answers 107, 116).
                let mut handles = [0u32; rd::MAX_MSG_HANDLES];
                let mut n = 0;
                while n < words[1].min(rd::MAX_MSG_HANDLES) {
                    match rd::mint_from_message(id, 1000 + n as u64, None) {
                        Ok(handle) => {
                            handles[n] = handle;
                            n += 1;
                        }
                        Err(_) => break,
                    }
                }
                log!(logger, "[server] replying with {} handles", n);
                rd::reply(id, &rd::body_with([op::REPLY_HANDLES, n, 0, 0], &handles[..n])).ok();
            }
            op::SERVE_BAD => {
                // `serve` on a call this thread does not hold, and on a message id of 0.
                let stranger = rd::serve(id.wrapping_add(1_000_000));
                let zero = rd::serve(0);
                // `serve` on a call it *does* hold makes it the current call again.
                let mine = rd::serve(id);
                log!(
                    logger,
                    "[server] serve stranger {:?}, serve 0 {:?}, serve mine {:?}",
                    stranger,
                    zero,
                    mine
                );
                rd::reply(id, &rd::body([op::SERVE_BAD, 0, 0, 0])).ok();
            }
            op::DONE => {
                log!(logger, "[server] done");
                rd::reply(id, &rd::body([op::DONE, 0, 0, 0])).ok();
            }
            other => {
                log!(logger, "[server] unknown opcode {}", other);
                rd::reply(id, &rd::body([0; rd::WORDS])).ok();
            }
        }
    }
}

/// A badged handle to this server's own endpoint, for its filler threads. Minting one needs the
/// receive right, which only this program holds (I4).
fn self_handle(logger: &mut Logger) -> u32 {
    match rd::mint_from_handle(rd::BOOT_ENDPOINT, 3, None) {
        Ok(handle) => handle,
        Err(error) => {
            log!(logger, "[server] mint for myself -> {:?}", error);
            0
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
