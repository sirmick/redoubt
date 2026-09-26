//! A reused PID inherits no authority (`tests/pid-reuse-authority.toml`). This program runs first
//! and alone, so it owns the console and every device handle, and it checks itself: each verdict
//! is a kernel result, a badge the kernel wrote or an exit notice.
//!
//! Child A gets a send on endpoint E badged 7, an endpoint of its own, the console IRQ, a device
//! that is not the console, and a budget. It maps the device, leaves a call open on E, and exits.
//! This program then spawns children until one draws A's PID; each is given only a send on a
//! fresh endpoint F, badged 9. That child, B, must hold nothing at any other index, send with
//! badge 9 only, and fault when it reads where A's device was mapped. Last, the console IRQ must
//! wake this program (the bench types a byte).
//!
//! A spawned child is a copy of this image, statics and all, so no child uses `Logger`: in the
//! copy, `logsrv` reads as started and would print to a console the child cannot reach.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use test_programs::rd::{self, Cause, Error, ExitNotice, Message, Received};
use test_programs::{log, logsrv, spawn};

const WAIT: u64 = 2_000_000;
/// A's exit code, so its notice cannot be taken for B's.
const A_CODE: u32 = 42;
/// The exit code of a child that read A's device address and was not stopped.
const SURVIVED: u32 = 1;
/// The exit code of a panicking process (the handler below).
const PANICKED: u32 = 101;
/// The indices B must not hold: every one but its slot 1, as far as a loader program's table goes.
const LAST_INDEX: u32 = 64;
/// PIDs are drawn at random from at most 63, so this many tries miss A's only with odds of
/// about e^-16.
const TRIES: usize = 1024;

/// A's slots, in the order `process_start` fills them.
mod a {
    pub const E_SEND: u32 = 1;
    pub const OWN_ENDPOINT: u32 = 2;
    pub const DEVICE: u32 = 4;
}

/// Set by the IRQ thread just before its `receive`: the source is masked until then (R5).
static WAITING: AtomicBool = AtomicBool::new(false);
/// The byte the console IRQ brought, once it has.
static WOKEN_BY: AtomicU8 = AtomicU8::new(0);

/// A: map the device, leave a call open on E from a thread of its own, and exit when told.
extern "C" fn holder(_: usize) -> ! {
    fn open_call(at: usize) { rd::call_waiting(a::E_SEND, &rd::body([at, 0, 0, 0]), None, rd::FOREVER).ok(); }
    let (at, _) = rd::map_device(a::DEVICE).expect("A maps the device");
    rd::thread(open_call, at).expect("A's calling thread");
    rd::receive(Some(a::OWN_ENDPOINT), rd::FOREVER, 0).expect("the signal to exit");
    rd::process_exit(A_CODE)
}

/// Whether this process holds nothing at `index`, by every call that names a handle.
fn holds_nothing(index: u32) -> bool {
    rd::usage(index).err() == Some(Error::BadHandle)
        && rd::map_device(index).err() == Some(Error::BadHandle)
        && rd::receive(Some(index), 0, 0).err() == Some(Error::BadHandle)
        && rd::close(index).err() == Some(Error::BadHandle)
}

/// Every child spawned after A, B among them: report on slot 1 whether slots 2 to `LAST_INDEX`
/// hold nothing, then read A's device address, which must fault.
extern "C" fn candidate(arg: usize) -> ! {
    let at = (0..8).fold(0u64, |at, i| at | (spawn::startup_byte(arg, i) as u64) << (8 * i));
    let held_nothing = (2..=LAST_INDEX).all(holds_nothing);
    rd::send(1, &rd::body([held_nothing as usize, 0, 0, 0]), None, WAIT).ok();
    rd::peek(at as usize);
    rd::process_exit(SURVIVED)
}

fn message(endpoint: u32) -> Message {
    match rd::receive(Some(endpoint), WAIT, 0).expect("a message") {
        Received::Message(m) => m,
        _ => panic!("expected a message"),
    }
}

fn notice(exit: u32) -> ExitNotice {
    match rd::receive(Some(exit), WAIT, 0).expect("an exit notice") {
        Received::Exit(n) => n,
        _ => panic!("expected an exit notice"),
    }
}

/// Wait on the console IRQ and keep the byte it brought.
fn irq_waiter(_: usize) {
    WAITING.store(true, Ordering::Release);
    if rd::receive(Some(rd::CONSOLE_IRQ), rd::FOREVER, 0) == Ok(Received::Interrupt) {
        let byte = test_programs::console::receive().unwrap_or(b'?');
        WOKEN_BY.store(byte, Ordering::Release);
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = logsrv::start();
    // Before this program creates any handle, while `rd::log_rx` still ends the devices.
    let device = (rd::OTHER_DEVICES..rd::log_rx())
        .find(|d| rd::map_device(*d).is_ok())
        .expect("a device that is not the console");
    let image = spawn::image();
    let exit = rd::endpoint_create().expect("exit endpoint");
    let kids = rd::create(rd::USERS, &rd::spec(600, 2, 100)).expect("the children's budget");

    // --- A ---------------------------------------------------------------------------------
    let e = rd::endpoint_create().expect("E");
    let e7 = rd::mint_from_handle(e, 7, None).expect("E badged 7");
    let own = rd::endpoint_create().expect("A's endpoint");
    let gift = rd::create(rd::USERS, &rd::spec(1, 1, 1)).expect("A's budget");
    let handles = [e7, own, rd::CONSOLE_IRQ, device, gift];
    spawn::spawn(&image, kids, exit, holder as *const () as usize, &[], &handles).expect("A");
    let open = message(e);
    assert_eq!(open.badge, 7);
    let device_at = open.body.words[0] as u64;
    rd::send(own, &rd::body([0; 4]), None, WAIT).expect("tell A to exit");
    let a = notice(exit);
    assert_eq!((a.cause, a.code), (Cause::Exited, A_CODE));
    assert_eq!(rd::receive(Some(e), WAIT, 0), Ok(Received::Abandoned(open.msg_id)));
    rd::reply(open.msg_id.get(), &rd::body([0; 4])).ok();
    log!(
        logger,
        "[pid-reuse] A (pid {}) exited holding a call, a device mapping, the irq and a budget",
        a.pid
    );

    // --- B: spawn until a child draws A's PID -----------------------------------------------
    let f = rd::endpoint_create().expect("F");
    let f9 = rd::mint_from_handle(f, 9, None).expect("F badged 9");
    let startup = device_at.to_le_bytes();
    let mut b = None;
    for _ in 0..TRIES {
        spawn::spawn(&image, kids, exit, candidate as *const () as usize, &startup, &[f9]).expect("a child");
        let report = message(f);
        let n = notice(exit);
        assert_eq!(n.cause, Cause::Faulted, "a child read A's device address and was not stopped");
        if n.pid == a.pid {
            b = Some((report.badge, report.body.words[0]));
            break;
        }
    }
    let (badge, held_nothing) = b.expect("no child drew A's PID");
    log!(logger, "[pid-reuse] B drew pid {}", a.pid);
    log!(
        logger,
        "[pid-reuse] B holds nothing at indices 2 to {}, the log handle's included: {}",
        LAST_INDEX,
        held_nothing == 1
    );
    log!(logger, "[pid-reuse] B's message carries badge {}", badge);
    log!(logger, "[pid-reuse] B faulted reading where A's device was mapped");
    let quiet = rd::receive(Some(e), 0, 0).err() == Some(Error::Timeout)
        && rd::receive(Some(exit), 0, 0).err() == Some(Error::Timeout);
    log!(logger, "[pid-reuse] nothing else reached E or the exit endpoint: {}", quiet);

    // --- The console IRQ -------------------------------------------------------------------
    rd::thread(irq_waiter, 0).expect("the irq thread");
    while !WAITING.load(Ordering::Acquire) {
        test_programs::wait_ms(1);
    }
    test_programs::wait_ms(10);
    log!(logger, "[pid-reuse] waiting for the console irq");
    while WOKEN_BY.load(Ordering::Acquire) == 0 {
        test_programs::wait_ms(1);
    }
    log!(
        logger,
        "[pid-reuse] the console irq woke this program: {:?}",
        WOKEN_BY.load(Ordering::Acquire) as char
    );
    if held_nothing == 1 && badge == 9 && quiet {
        log!(logger, "PID REUSE AUTHORITY TEST PASSED");
    }
    rd::system_reset(rd::RESET, rd::ResetKind::PowerOff).ok();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { rd::process_exit(PANICKED) }
