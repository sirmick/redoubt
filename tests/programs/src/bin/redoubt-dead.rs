//! R4b and R4's `Refused`. Must run as the loader's first program, the one holding
//! `root`, `system` and `users`: the `Refused` step needs a budget handle to take `system`'s
//! free pages away and give them back.
//!
//! What it shows:
//! - a server thread that exits holding an open call gives its caller `Dead` and its lend back,
//!   intact, while the endpoint survives and a later server receives on it (R4b);
//! - a message the receiving process's budget cannot pay for is `Refused` to its sender, and
//!   the receiver is unaffected: it takes the very next message (R4).
//!
//! See `tests/redoubt-dead.toml`.

#![no_std]
#![no_main]

use test_programs::rd::{self, Error, FOREVER, MessageKind, Received};
use test_programs::{Logger, log};

/// The endpoint the short-lived server receives on.
static mut ENDPOINT: u32 = 0;
/// Set once the first server thread has taken a call and gone.
static mut SERVER_GONE: usize = 0;
/// What the second server thread saw: 1 + the page count of the lend it was refused, or 1.
static mut SECOND_SAW: usize = 0;

fn endpoint() -> u32 {
    // SAFETY: the main thread writes it once, before creating any thread that reads it.
    unsafe { core::ptr::read_volatile(&raw const ENDPOINT) }
}

/// A server thread that takes one call and then leaves without replying: its caller must be
/// told, and the lend must come back (R4b).
fn short_lived(_arg: usize) {
    let mut logger = Logger::connect();
    match rd::receive(Some(endpoint()), FOREVER, 0) {
        Ok(Received::Message(m)) => {
            let lend = match m.kind {
                MessageKind::Call { lend } => lend.map(|p| p.npages.get()),
                MessageKind::Send { .. } => None,
            };
            log!(logger, "[dead] server took a call with a lend of {:?} pages, and exits", lend);
        }
        other => log!(logger, "[dead] server: {:?}", other),
    }
    // SAFETY: this thread is the only writer.
    unsafe { core::ptr::write_volatile(&raw mut SERVER_GONE, 1) };
    // Returning from a thread function exits the thread, while it still holds the call.
}

/// A second server thread, for the `Refused` step: it receives, so that a message the budget
/// cannot pay for is refused to its *sender* and this thread keeps waiting (R4).
fn second_server(_arg: usize) {
    let mut logger = Logger::connect();
    loop {
        match rd::receive(Some(endpoint()), FOREVER, 0) {
            Ok(Received::Message(m)) => {
                let pages = match m.kind {
                    MessageKind::Call { lend } => lend.map_or(0, |p| p.npages.get()),
                    MessageKind::Send { .. } => 0,
                };
                // SAFETY: this thread is the only writer.
                unsafe { core::ptr::write_volatile(&raw mut SECOND_SAW, 1 + pages) };
                log!(logger, "[dead] second server took a message with {} lent pages", pages);
                rd::reply(m.msg_id.get(), &rd::body([7, pages, 0, 0])).ok();
            }
            other => {
                log!(logger, "[dead] second server: {:?}", other);
                test_programs::park()
            }
        }
    }
}

struct T {
    logger: Logger,
    failed: bool,
}

impl T {
    fn check(&mut self, ok: bool, what: core::fmt::Arguments) {
        if !ok {
            self.failed = true;
            self.logger.log(format_args!("[dead] FAIL: {}", what));
        }
    }
}

macro_rules! expect {
    ($t:expr, $got:expr, $want:expr) => {{
        let got = $got;
        let want = $want;
        $t.check(got == want, format_args!("{} = {:?}, want {:?}", stringify!($got), got, want));
        got
    }};
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let logger = test_programs::logsrv::start();
    let mut t = T { logger, failed: false };
    log!(t.logger, "[dead] starting");
    let endpoint = rd::endpoint_create().expect("an endpoint");
    // SAFETY: written before any thread that reads it is created.
    unsafe { core::ptr::write_volatile(&raw mut ENDPOINT, endpoint) };

    // --- R4b: the server thread goes while it holds the call ---------------------------------
    rd::thread(short_lived, 0).expect("the short-lived server");
    let page = rd::page();
    rd::poke(page, 0xD00D);
    let reply = rd::call(endpoint, &rd::body([1, 0, 0, 0]), rd::pages(page, 1), FOREVER);
    let _ = expect!(t, reply.err(), Some(Error::Dead));
    // SAFETY: the server thread is the only writer, and it has finished.
    let _ = expect!(t, unsafe { core::ptr::read_volatile(&raw const SERVER_GONE) }, 1);
    // The lend came back as it was: the caller's page is its own again, unchanged.
    let _ = expect!(t, rd::peek(page), 0xD00D);
    rd::poke(page, 0xD00E);
    let _ = expect!(t, rd::peek(page), 0xD00E);

    // --- R4: a message the receiving budget cannot pay for -----------------------------------
    // The endpoint survived its server, and a new one receives on it (R4b).
    rd::thread(second_server, 0).expect("the second server");
    test_programs::wait_ms(30);
    // A lend of `MAX_LEND_PAGES`, which the receiver must pay for while the call is open (R3),
    // with the budget it shares with this thread carved down below that.
    let big = rd::many_pages(rd::MAX_LEND_PAGES);
    let free = rd::free(rd::SYSTEM);
    let margin = 8;
    t.check(free > margin, format_args!("system has {} free pages", free));
    let hog = rd::create(rd::SYSTEM, &rd::spec(free - margin - 1, 0, 0)).expect("a hog budget");
    log!(t.logger, "[dead] system down to {} free pages", rd::free(rd::SYSTEM));
    let refused = rd::call(endpoint, &rd::body([2, 0, 0, 0]), rd::pages(big, rd::MAX_LEND_PAGES), FOREVER);
    let _ = expect!(t, refused.err(), Some(Error::Refused));
    // The refused lend is this thread's again, and the receiver never saw it.
    rd::poke(big, 1);
    // SAFETY: the second server is the only writer.
    let _ = expect!(t, unsafe { core::ptr::read_volatile(&raw const SECOND_SAW) }, 0);
    let _ = expect!(t, rd::destroy(hog), Ok(()));
    // With the pages back, the same message goes through, to the same waiting receiver.
    let through = rd::call(endpoint, &rd::body([3, 0, 0, 0]), rd::pages(big, rd::MAX_LEND_PAGES), FOREVER);
    let _ = expect!(t, through.map(|r| r.words), Ok([7, rd::MAX_LEND_PAGES, 0, 0]));

    if t.failed {
        log!(t.logger, "REDOUBT-DEAD TEST FAILED");
    } else {
        log!(t.logger, "REDOUBT-DEAD TEST PASSED");
    }
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
