//! The client side of the Redoubt IPC case, and the program that drives the script.
//! Everything here crosses an address space: this program holds a badged handle to the boot
//! endpoint, `redoubt-server` holds its receive right.
//!
//! What it shows: a `call` with words and a lend, with the reply written back over the request
//! (R3: the lend is unmapped here until the reply, and the server's change is there after it); a
//! `send` with a transfer, whose pages become the receiver's for good, and one the receiver did
//! not ask for (`Refused`, R4); the badge, account and labels the kernel attaches, which no
//! client chooses; `mint` refused on a badged handle and on a badge of 0, and a receive right
//! that cannot be stolen (I3, I4); `WAIT_CAP` queued messages per group and `Busy` beyond (R2);
//! every abandoned call reported once and freed by its reply (R3, I15); a message whose handle
//! the receiver's full table cannot take, refused to its sender, and a reply whose handles do
//! not fit, delivered without them with `OutOfMemory` (R4); `MAX_OPEN_CALLS`
//! with sends still delivered (R4a).
//!
//! The checks this program prints are its own; the lines that only the kernel or the server can
//! produce are in `tests/redoubt-ipc.toml` beside them.

#![no_std]
#![no_main]

use test_programs::rd::{self, Error, FOREVER, Received};
use test_programs::redoubt_ipc::op;
use test_programs::{Logger, log};

struct T {
    logger: Logger,
    failed: bool,
}

impl T {
    fn check(&mut self, ok: bool, what: core::fmt::Arguments) {
        if !ok {
            self.failed = true;
            self.logger.log(format_args!("[ipc] FAIL: {}", what));
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

/// This program's PID, which the kernel also made its badge on the boot endpoint.
const MY_PID: usize = 4;
const E: u32 = rd::BOOT_ENDPOINT;

/// The endpoint nobody receives on, for the `WAIT_CAP` step. Its handle is this program's.
static mut SILENT: u32 = 0;

fn words(reply: &rd::ReceivedBody) -> [usize; rd::WORDS] { reply.words }

fn ask(t: &mut T, code: usize, arg: usize) -> [usize; rd::WORDS] {
    match rd::call_waiting(E, &rd::body([code, arg, 0, 0]), None, FOREVER) {
        Ok(reply) => words(&reply),
        Err(error) => {
            t.check(false, format_args!("call {} -> {:?}", code, error));
            [0; rd::WORDS]
        }
    }
}

/// A thread that queues one message on an endpoint nobody receives on (R2's `WAIT_CAP`).
fn queue_one(_arg: usize) {
    // SAFETY: the main thread wrote it before creating any of these threads.
    let silent = unsafe { core::ptr::read_volatile(&raw const SILENT) };
    rd::call(silent, &rd::body([0; rd::WORDS]), None, FOREVER).ok();
    test_programs::park()
}

/// A thread that offers one call for the server to park (R4a).
fn parker(_arg: usize) {
    rd::call_waiting(E, &rd::body([op::KEEP, 0, 0, 0]), None, FOREVER).ok();
    test_programs::park()
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let logger = Logger::connect();
    let mut t = T { logger, failed: false };
    log!(t.logger, "[ipc] starting");

    // --- A call: the kernel attaches the badge, the account and the labels (Messages) --------
    let _ = expect!(t, ask(&mut t, op::ECHO, 41), [42, MY_PID, 0, 0]);

    // --- A lend (R3, I9): gone from here while the call runs, back with the server's change --
    let page = rd::page();
    rd::poke(page, 0x5150);
    let reply = rd::call_waiting(E, &rd::body([op::LEND, 0, 0, 0]), rd::pages(page, 1), FOREVER);
    let _ = expect!(t, reply.map(|r| words(&r)), Ok([1, 0x5150, 0, 0]));
    let _ = expect!(t, rd::peek(page), 0x5151);
    // A lend over `MAX_LEND_PAGES` is `TooLarge`, and nothing has moved when it is refused.
    let too_big = rd::pages(page, rd::MAX_LEND_PAGES + 1);
    let _ = expect!(t, rd::call(E, &rd::body([op::LEND, 0, 0, 0]), too_big, FOREVER).err(), Some(Error::TooLarge));
    let _ = expect!(t, rd::peek(page), 0x5151);
    // A lend of memory that is not this program's is refused too.
    let stranger = rd::pages(0x4000, 1);
    let _ = expect!(
        t,
        rd::call(E, &rd::body([op::LEND, 0, 0, 0]), stranger, FOREVER).err(),
        Some(Error::InvalidArgument)
    );

    // --- A transfer (R4): the receiver must have named a `max_transfer` at least its size ----
    let gift = rd::page();
    rd::poke(gift, 0xC0FFEE);
    let _ = expect!(t, rd::send_waiting(E, &rd::body([0, 7, 0, 0]), rd::pages(gift, 1), FOREVER), Ok(()));
    // The pages are the receiver's for good: this address is not ours to send again.
    let again = rd::send(E, &rd::body([0, 8, 0, 0]), rd::pages(gift, 1), FOREVER);
    let _ = expect!(t, again.err(), Some(Error::InvalidArgument));
    // A transfer the receiver never asked for: `Refused` to the sender, the receiver unaffected.
    let _ = expect!(t, ask(&mut t, op::MAX_TRANSFER, 0)[1], 0);
    let unwanted = rd::page();
    let refused = rd::send(E, &rd::body([0, 9, 0, 0]), rd::pages(unwanted, 1), FOREVER);
    let _ = expect!(t, refused.err(), Some(Error::Refused));
    rd::poke(unwanted, 1); // still ours
    let _ = expect!(t, ask(&mut t, op::ECHO, 1)[0], 2);
    let _ = expect!(t, ask(&mut t, op::MAX_TRANSFER, 4)[1], 4);

    // --- Receive rights and minting (I3, I4) -------------------------------------------------
    let _ = expect!(t, rd::receive(Some(E), 0, 0).err(), Some(Error::NotPermitted));
    let _ = expect!(t, rd::mint_from_handle(E, 7, None).err(), Some(Error::NotPermitted));
    // A badge of 0 does not even decode, offered in raw registers because the typed call cannot
    // carry one: the receive right is never minted (I3).
    let _ = expect!(t, rd::mint_raw(2, E as usize, 0, 0, 0), Some(Error::InvalidArgument));
    let _ = expect!(t, rd::mint_from_message(1, 7, None).err(), Some(Error::InvalidArgument));
    // A handle the server minted arrives in a reply, badged as the server chose (R9).
    let reply = rd::call_waiting(E, &rd::body([op::MINT_BACK, 77, 0, 0]), None, FOREVER);
    let minted = reply.ok().and_then(|r| r.handles.as_slice().first().copied().flatten()).map(|h| h.index());
    t.check(minted.is_some(), format_args!("the server's minted handle arrived: {:?}", minted));
    if let Some(handle) = minted {
        let echo = rd::call_waiting(handle, &rd::body([op::ECHO, 0, 0, 0]), None, FOREVER);
        let _ = expect!(t, echo.map(|r| words(&r)[1]), Ok(77));
        let _ = expect!(t, rd::receive(Some(handle), 0, 0).err(), Some(Error::NotPermitted));
        let _ = expect!(t, rd::close(handle), Ok(()));
    }

    // --- `serve` and `reply` refuse what the thread does not hold ----------------------------
    let _ = expect!(t, rd::serve(1).err(), Some(Error::InvalidArgument));
    let _ = expect!(t, rd::reply(1, &rd::body([0; rd::WORDS])).err(), Some(Error::InvalidArgument));
    let _ = expect!(t, ask(&mut t, op::SERVE_BAD, 0)[0], op::SERVE_BAD);

    // --- R2: `WAIT_CAP` queued messages per group, `Busy` beyond ------------------------------
    // An endpoint of this program's own that nothing ever receives on. Every bundle program
    // lives in `system` with account 0, so they are all one group; `WAIT_CAP` is that group's.
    let silent = rd::endpoint_create().expect("an endpoint of our own");
    // SAFETY: written before any thread that reads it is created.
    unsafe { core::ptr::write_volatile(&raw mut SILENT, silent) };
    for _ in 0..rd::WAIT_CAP {
        rd::thread(queue_one, 0).expect("a queueing thread");
    }
    // A `call` with a timeout of 0 queues and is withdrawn at once, so this poll adds nothing
    // to the queue; it answers `Busy` as soon as the other `WAIT_CAP` are in it.
    let mut busy = None;
    for _ in 0..200 {
        busy = rd::call(silent, &rd::body([0; rd::WORDS]), None, 0).err();
        if busy == Some(Error::Busy) {
            break;
        }
        test_programs::wait_ms(1);
    }
    let _ = expect!(t, busy, Some(Error::Busy));

    // --- R3 and I15: abandoned calls, reported once and freed by their reply ------------------
    // A timeout alone does not establish receipt: it may cancel a still-queued call. A lend's
    // outcome distinguishes that (Returned) from a taken call's abandonment (Consumed). Count
    // exactly 64 proven abandonments; do not mistake scheduler timing for a missing notice.
    let before = ask(&mut t, op::COUNTS, 0)[2];
    let never_received = rd::endpoint_create().expect("queued-cancellation endpoint");
    let (mut abandoned, mut queued) = (0, 0);
    for attempt in 0..256 {
        let page = rd::map_anon(rd::PAGE_SIZE, rd::rw()).expect("abandonment lend");
        // The first attempt deliberately exercises the Returned cleanup branch. The remaining
        // ones target the server, whose own notice count must match the Consumed outcomes.
        let (endpoint, timeout) = if attempt == 0 { (never_received, 0) } else { (E, 2_000) };
        let (outcome, reply) =
            rd::call_outcome(endpoint, &rd::body([op::KEEP, 0, 0, 0]), rd::pages(page, 1), timeout)
                .expect("call outcome");
        t.check(reply.is_none(), format_args!("a parked call has no reply"));
        match (outcome.status, outcome.lend) {
            (Err(Error::Timeout), redoubt_sys::LendDisposition::Consumed) => abandoned += 1,
            (Err(Error::Timeout), redoubt_sys::LendDisposition::Returned) => {
                queued += 1;
                rd::unmap(page, rd::PAGE_SIZE).expect("returned queued lend");
            }
            (Err(Error::Busy), redoubt_sys::LendDisposition::Returned) => {
                rd::unmap(page, rd::PAGE_SIZE).expect("returned busy lend");
                test_programs::wait_ms(1);
            }
            other => {
                t.check(false, format_args!("unexpected abandonment outcome: {:?}", other));
                break;
            }
        }
        if abandoned == rd::MAX_OPEN_CALLS {
            break;
        }
    }
    rd::close(never_received).expect("close unused endpoint handle");
    let _ = expect!(t, abandoned, rd::MAX_OPEN_CALLS);
    t.check(queued >= 1, format_args!("queued timeout exercised: {}", queued));
    let counts = ask(&mut t, op::COUNTS, 0);
    // Every one was reported exactly once, and every reply freed its call, so the server holds
    // no more open calls than the other programs' own parked ones (`redoubt-filler`'s, which
    // keep arriving while this runs, so only the abandoned ones are counted exactly).
    let _ = expect!(t, counts[1], rd::MAX_OPEN_CALLS);
    log!(
        t.logger,
        "[ipc] {} calls abandoned, {} notices, {} open before, {} after",
        rd::MAX_OPEN_CALLS,
        counts[1],
        before,
        counts[2]
    );
    t.check(counts[2] <= rd::MAX_OPEN_CALLS, format_args!("open calls after: {}", counts[2]));

    let full_notice = rd::endpoint_create().expect("capacity endpoint");
    let notify = rd::mint_from_handle(full_notice, 1, None).expect("capacity sender");

    // --- A reply whose handles do not fit the caller (R4) ------------------------------------
    // Fill this program's table to `MAX_HANDLES` with endpoints of its own.
    let mut held = 0;
    while rd::endpoint_create().is_ok() {
        held += 1;
        if held > 8192 {
            break;
        }
    }
    log!(t.logger, "[ipc] table full after {} more endpoints", held);
    let _ = expect!(t, rd::endpoint_create().err(), Some(Error::TooLarge));
    // The reply still arrives; its handles are dropped and the `call` is `OutOfMemory`.
    let reply = rd::call_waiting(E, &rd::body([op::REPLY_HANDLES, 2, 0, 0]), None, FOREVER);
    let _ = expect!(t, reply.err(), Some(Error::OutOfMemory));

    // --- Handles the receiver cannot take refuse the message (R4) -----------------------------
    // The server fills its own table to `MAX_HANDLES`; a message carrying a handle is then more
    // than it can pay for, so its *sender* is refused, exactly as for any other cost.
    let filled = ask(&mut t, op::FILL_TABLE, 0)[1];
    t.check(filled > 0, format_args!("the server filled its table: {}", filled));
    let with_handle = rd::send(E, &rd::body_with([0, 12, 0, 0], &[E]), None, FOREVER);
    let _ = expect!(t, with_handle.err(), Some(Error::Refused));
    // One without a handle costs the same table nothing, and is delivered.
    let _ = expect!(t, rd::send(E, &rd::body([0, 13, 0, 0]), None, FOREVER), Ok(()));
    let _ = expect!(t, ask(&mut t, op::FILL_TABLE, 1)[1], 0);

    // --- R4a: at `MAX_OPEN_CALLS` no call is taken, while a send still is ---------------------
    for _ in 0..12 {
        if rd::thread(parker, 0).is_err() {
            break;
        }
    }
    // Synchronize on actual saturation, not a wall-clock delay: the server's own trusted
    // check reports its 64th open call before it sends this acknowledgement.
    rd::call(E, &rd::body_with([op::SELF_FILL, 0, 0, 0], &[notify]), None, FOREVER).expect("start filling");
    let Received::Message(full) = rd::receive(Some(full_notice), FOREVER, 0).expect("capacity notice") else {
        panic!("expected capacity notice");
    };
    let _ = expect!(t, full.body.words[1], rd::MAX_OPEN_CALLS);
    // A call now stays queued and times out; a `send` is still delivered (R4a).
    let _ = expect!(t, rd::call(E, &rd::body([op::KEEP, 0, 0, 0]), None, 5_000).err(), Some(Error::Timeout));
    let _ = expect!(t, rd::send(E, &rd::body([0, 99, 0, 0]), None, FOREVER), Ok(()));
    // `send` returns once the message is taken; give the server its turn to report it, so the
    // console reads in the order the case expects.
    test_programs::wait_ms(50);

    if t.failed {
        log!(t.logger, "REDOUBT-IPC TEST FAILED");
    } else {
        log!(t.logger, "REDOUBT-IPC TEST PASSED");
    }
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
