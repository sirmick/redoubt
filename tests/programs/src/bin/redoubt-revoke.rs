//! R10's reach into messages (WP-K2), and `mint`'s narrowing rule (R9, I3). Must run as the
//! loader's first program, the one that holds `root`, `system` and `users`, because only a
//! budget handle can revoke anything.
//!
//! Everything here happens inside one process, across threads, because a Redoubt handle cannot
//! reach a second process until `process_start` carries one (WP-K4). That is enough: R10 looks
//! at the *stamp* of the handle a message was sent through, and at the handles a message
//! carries, neither of which cares which address space they are in.
//!
//! What it shows:
//! - a budget handle only narrows: minting into a budget that is not the default stamp or below
//!   it is `NotPermitted` (I3);
//! - destroying the stamping budget fails a **queued** message with `Dead` (R10);
//! - and fails a **taken** call with `Dead` at once, abandoning it, so the server's reply --
//!   handles and all -- reaches nobody (R3, R10);
//! - a handle **inside** a queued message that the same destruction revoked arrives as 0,
//!   keeping its slot (R10, ABI).
//!
//! See `tests/redoubt-revoke.toml`.

#![no_std]
#![no_main]
#![allow(unused_must_use)] // `expect!` returns what it checked, for the steps that use it

use test_programs::rd::{self, Error, FOREVER, MessageKind, Received};
use test_programs::{Logger, log};

/// An endpoint nothing ever receives on: a message sent here stays queued.
static mut SILENT: u32 = 0;
/// A handle to `SILENT` stamped with the budget that will be destroyed.
static mut STAMPED_SILENT: u32 = 0;
/// An endpoint a thread of this process receives on.
static mut SERVED: u32 = 0;
/// A handle to `SERVED` stamped with the budget that will be destroyed.
static mut STAMPED_SERVED: u32 = 0;
/// A handle stamped with a second doomed budget, to travel inside a queued message.
static mut CARRIED: u32 = 0;
/// What the queued sender and the taken caller were told, plus 1 (0 = still waiting).
static mut QUEUED_RESULT: usize = 0;
static mut TAKEN_RESULT: usize = 0;
/// Whether the taken caller got a reply body rather than an error.
static mut TAKEN_REPLIED: usize = 0;

fn get(at: *const u32) -> u32 {
    // SAFETY: the main thread writes each of these once, before creating the thread that reads
    // it; afterwards they are read-only.
    unsafe { core::ptr::read_volatile(at) }
}

fn put(at: *mut usize, value: usize) {
    // SAFETY: one writer per slot, and the main thread only reads it.
    unsafe { core::ptr::write_volatile(at, value) };
}

fn code(result: Result<rd::ReceivedBody, Error>) -> usize {
    match result {
        Ok(_) => 1,
        Err(error) => 2 + error as usize,
    }
}

/// A caller whose message stays queued: nothing receives on `SILENT`.
fn queued_caller(_arg: usize) -> ! {
    let handle = get(&raw const STAMPED_SILENT);
    let result = rd::call(handle, &rd::body([1, 0, 0, 0]), None, FOREVER);
    put(&raw mut QUEUED_RESULT, code(result));
    test_programs::park()
}

/// A caller whose call a server takes and parks, so that revocation abandons it (R3).
fn taken_caller(_arg: usize) -> ! {
    let handle = get(&raw const STAMPED_SERVED);
    let result = rd::call(handle, &rd::body([2, 0, 0, 0]), None, FOREVER);
    if let Ok(reply) = &result {
        put(&raw mut TAKEN_REPLIED, 1 + reply.handles.as_slice().len());
    }
    put(&raw mut TAKEN_RESULT, code(result));
    test_programs::park()
}

/// A sender whose message carries a handle that revocation will take away before anyone
/// receives it.
fn carrier(_arg: usize) -> ! {
    let silent = get(&raw const SILENT);
    let carried = get(&raw const CARRIED);
    rd::send(silent, &rd::body_with([3, 0, 0, 0], &[carried]), None, FOREVER).ok();
    test_programs::park()
}

/// The server thread: it takes one call, parks it, and answers the abandoned-call notice that
/// revocation produces, replying with a handle that must reach nobody.
fn server(_arg: usize) -> ! {
    let mut logger = Logger::connect();
    let served = get(&raw const SERVED);
    loop {
        match rd::receive(Some(served), FOREVER, 0) {
            Ok(Received::Message(m)) => {
                log!(logger, "[revoke] server took call {} badge {}", m.msg_id.get(), m.badge);
            }
            Ok(Received::Abandoned(id)) => {
                // R3: the call is still open, and its reply is discarded, handles and all.
                let minted = rd::mint_from_handle(served, 99, None).unwrap_or(0);
                let body = rd::body_with([4, 0, 0, 0], &[minted]);
                let replied = rd::reply(id.get(), &body);
                log!(logger, "[revoke] server: call {} abandoned, reply {:?}", id.get(), replied);
            }
            other => {
                log!(logger, "[revoke] server: {:?}", other);
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
            self.logger.log(format_args!("[revoke] FAIL: {}", what));
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
    let logger = Logger::connect();
    let mut t = T { logger, failed: false };
    log!(t.logger, "[revoke] starting");

    // Two revocation scopes: budgets with no limits at all, made only to be destroyed
    // (CAPABILITIES.md). They are children of `system`, this process's own budget.
    let scope = rd::create(rd::SYSTEM, &rd::spec(0, 0, 0)).expect("a revocation scope");
    let scope2 = rd::create(rd::SYSTEM, &rd::spec(0, 0, 0)).expect("a second scope");
    let silent = rd::endpoint_create().expect("an endpoint");
    let served = rd::endpoint_create().expect("another endpoint");

    // --- I3: a budget handle only narrows ----------------------------------------------------
    // An endpoint handle is stamped with its creator's budget, here `system`. Minting into
    // `root` (above it) or `users` (beside it) is refused; into a child of `system`, allowed.
    expect!(t, rd::mint_from_handle(silent, 5, Some(rd::ROOT)).err(), Some(Error::NotPermitted));
    expect!(t, rd::mint_from_handle(silent, 5, Some(rd::USERS)).err(), Some(Error::NotPermitted));
    let stamped_silent = rd::mint_from_handle(silent, 5, Some(scope)).expect("mint into the scope");
    let stamped_served = rd::mint_from_handle(served, 6, Some(scope)).expect("mint into the scope");
    let carried = rd::mint_from_handle(silent, 7, Some(scope2)).expect("mint into the second scope");
    // A handle minted into a budget is a handle like any other while that budget lives.
    expect!(t, rd::close(rd::mint_from_handle(silent, 8, Some(scope)).expect("mint")), Ok(()));

    // SAFETY: written here, before any thread that reads them is created.
    unsafe {
        core::ptr::write_volatile(&raw mut SILENT, silent);
        core::ptr::write_volatile(&raw mut SERVED, served);
        core::ptr::write_volatile(&raw mut STAMPED_SILENT, stamped_silent);
        core::ptr::write_volatile(&raw mut STAMPED_SERVED, stamped_served);
        core::ptr::write_volatile(&raw mut CARRIED, carried);
    }
    redoubt_abi::create_thread_1(server, 0).expect("the server thread");
    redoubt_abi::create_thread_1(queued_caller, 0).expect("the queued caller");
    redoubt_abi::create_thread_1(taken_caller, 0).expect("the taken caller");
    redoubt_abi::create_thread_1(carrier, 0).expect("the carrier");
    // Let each of them reach its call; none of them can return until the destruction below.
    test_programs::wait_ms(100);
    // SAFETY: each slot has one writer, and this thread only reads.
    let (queued, taken) = unsafe {
        (
            core::ptr::read_volatile(&raw const QUEUED_RESULT),
            core::ptr::read_volatile(&raw const TAKEN_RESULT),
        )
    };
    expect!(t, (queued, taken), (0, 0));

    // --- R10: destroying the stamp fails both messages with `Dead` ---------------------------
    expect!(t, rd::destroy(scope), Ok(()));
    test_programs::wait_ms(100);
    // SAFETY: as above.
    let (queued, taken, replied) = unsafe {
        (
            core::ptr::read_volatile(&raw const QUEUED_RESULT),
            core::ptr::read_volatile(&raw const TAKEN_RESULT),
            core::ptr::read_volatile(&raw const TAKEN_REPLIED),
        )
    };
    let dead = 2 + Error::Dead as usize;
    expect!(t, queued, dead);
    expect!(t, taken, dead);
    // The server's reply to the abandoned call reached nobody, handles and all.
    expect!(t, replied, 0);

    // --- R10: a handle inside a queued message, revoked before anyone received it ------------
    expect!(t, rd::destroy(scope2), Ok(()));
    match rd::receive(Some(silent), 1_000_000, 0) {
        Ok(Received::Message(m)) => {
            let slots = m.body.handles.as_slice();
            log!(t.logger, "[revoke] the carried handle arrived as {:?}", slots);
            expect!(t, m.body.words[0], 3);
            expect!(t, slots.len(), 1);
            expect!(t, slots.first().copied().flatten().is_none(), true);
            expect!(t, matches!(m.kind, MessageKind::Send { transfer: None }), true);
        }
        other => t.check(false, format_args!("the carrier's message: {:?}", other)),
    }

    if t.failed {
        log!(t.logger, "REDOUBT-REVOKE TEST FAILED");
    } else {
        log!(t.logger, "REDOUBT-REVOKE TEST PASSED");
    }
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
