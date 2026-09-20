//! `consoled`, the program: two threads over one UART.
//!
//! - **The serving thread** owns the UART and the 9P skeleton. It answers writes by sending the bytes out; it
//!   answers a read from the input it holds, and when it holds none it **parks the call**
//!   ([`redoubt_rt::server::parked`]) instead of blocking, so every other client is still served. Nothing of
//!   a parked read is kept but the call itself: serving it again reads its T-message out of its own lend
//!   afresh (`NineServer::serve_parking`).
//! - **The interrupt thread** does nothing but `receive` on the IRQ handle (KERNEL-SPEC.md, R5: the kernel
//!   masks the source when it fires and the next receive unmasks it; there is no acknowledge) and `send` one
//!   word to the serving thread's own endpoint. It touches no register, so the UART stays on one thread and
//!   there is no shared state between the two — which is why neither needs a lock, and why the runtime's
//!   `Registers` need not be `Sync`.
//!
//! A wake-up carries no data: the serving thread drains the whole FIFO each time, so two bytes
//! that arrive between two interrupts are both read, and a wake-up that names nothing costs a
//! look at one register. The `send` waits rather than timing out, so a wake-up is never lost:
//! the serving thread always returns to `receive`.
//!
//! Everything it can do is in `redoubt-consoled`'s library and here, so host tests drive the
//! same code — both threads, a fake UART's registers and a fired interrupt — against the
//! runtime's fake kernel (`tests/consoled.rs`).

#![cfg_attr(target_os = "none", no_std, no_main)]

extern crate alloc;

use core::num::NonZeroU64;
use core::sync::atomic::{AtomicU32, Ordering};

use redoubt_consoled::server::{BUDGET, COST, Console, LIMITS};
use redoubt_consoled::uart::Uart;
use redoubt_rt::abi::{Error, FOREVER, Handle, MemFlags, PAGE_SIZE};
use redoubt_rt::handle::{Endpoint, Irq, Mmio};
use redoubt_rt::ipc::{Event, Request};
use redoubt_rt::server::ninep::{NineError, NineServer, WORDS_9P, refuse};
use redoubt_rt::server::parked::{NotParked, Parked};
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(serve);

/// The name of the endpoint `consoled` receives on, in its startup block (INIT.md: a server's
/// manifest entry names the endpoints it receives on).
pub const ENDPOINT: &str = "consoled";
/// The names of the two device handles `init` puts in the startup block: the UART's registers
/// and its interrupt (INIT.md, the manifest's `devices` and a server's `devices` list).
pub const UART_MMIO: &str = "uart";
pub const UART_IRQ: &str = "uart:irq";

/// The startup block named no endpoint to receive on.
pub const NO_ENDPOINT: u32 = 2;
/// `receive` failed for a reason other than the endpoint going away.
pub const RECEIVE_FAILED: u32 = 3;
/// The limits in this build do not fit the budget or the open-call headroom.
pub const BAD_LIMITS: u32 = 4;
/// The startup block named no UART, or `map_device` refused it, or the mapping is too short to
/// be an ns16550. A console driver with no console does not start (TENETS.md 2, fail closed).
pub const NO_UART: u32 = 5;
/// The startup block named no interrupt, or the interrupt thread could not be started. Without
/// it a read would wait for ever, so this is a refusal to start rather than a silent downgrade.
pub const NO_IRQ: u32 = 6;
/// The kernel would not give a random word, and a server's first minted badge must be
/// unpredictable (answer 126).
pub const NO_RANDOM: u32 = 7;

/// The badge the interrupt thread's wake-ups arrive with. It is the serving thread's own mint
/// off its receive right, so it is this process's and not a client's. A client that held a
/// handle with the same badge could only make the serving thread look at one register, which it
/// does on every event anyway.
const WAKE_BADGE: u64 = 1;
/// Word 0 of a wake-up. Word 0 of a 9P call is 0, so the two can never be confused.
const WAKE: u64 = 1;
/// The interrupt thread's stack.
const IRQ_STACK: usize = 4 * PAGE_SIZE;

/// The endpoint the interrupt thread sends its wake-ups to, put here by the serving thread
/// before that thread exists. An atomic rather than a `static mut`: no `unsafe`, and the
/// ordering is plain because the value is written once, before the reader is created.
static WAKE_ENDPOINT: AtomicU32 = AtomicU32::new(0);

/// The interrupt thread. `arg` is the IRQ handle's index; it never returns.
extern "C" fn irq_thread(arg: usize) -> ! {
    let irq = Handle::new(arg as u32).map(Irq::from_handle);
    let wake = Handle::new(WAKE_ENDPOINT.load(Ordering::Acquire)).map(Endpoint::from_handle);
    if let (Some(irq), Some(wake)) = (irq, wake) {
        loop {
            // The wait unmasks the source (R5); a timeout cannot happen with FOREVER, and any
            // other error means the handle is gone and nothing more will ever arrive.
            if irq.wait(FOREVER).is_err() {
                break;
            }
            // Waits rather than timing out, so a wake-up is never lost: the serving thread
            // always comes back to `receive`. A dead endpoint ends this thread.
            if wake.send(&[WAKE, 0, 0, 0], &[], None, FOREVER).is_err() {
                break;
            }
        }
    }
    redoubt_rt::handle::thread_exit()
}

/// Starts the interrupt thread with its own stack.
fn start_irq_thread(irq: Irq, wake: Endpoint) -> Result<(), Error> {
    WAKE_ENDPOINT.store(wake.handle().index(), Ordering::Release);
    let stack = redoubt_rt::handle::map_anon(IRQ_STACK, MemFlags::READ | MemFlags::WRITE)?;
    // The stack grows down from the top of the mapping, which is page-aligned and so also
    // aligned for any call frame.
    let sp = stack + IRQ_STACK;
    let entry = irq_thread as extern "C" fn(usize) -> ! as usize;
    redoubt_rt::handle::thread_create(entry, sp, irq.handle().index() as usize).map(|_| ())
}

/// Answers `request`, or parks it if the file server asked to wait. The one place a console
/// read is held.
fn serve_or_park(
    server: &mut NineServer<Console>,
    parked: &mut Parked<()>,
    request: Request,
    now: u64,
) -> Result<(), Error> {
    // `consoled` serves no typed protocol of its own: only 9P and `ninep_common`.
    let held = server.serve_parking(request, |_, request| {
        request.reply(&redoubt_rt::server::MALFORMED, &[]).map_err(|(e, _)| e)
    })?;
    let Some(request) = held else { return Ok(()) };
    let share = server.share_of(&request.caller);
    match parked.park(server.admission_mut(), request, share, (), now) {
        Ok(()) => Ok(()),
        // The caller's bucket or share is full of waiting reads, or there is no memory for one
        // more: it is told so, rather than being left to wait on a call the server cannot hold.
        Err(NotParked(request)) => refuse(request, NineError::TOO_MANY),
    }
}

/// Reads everything the UART has and answers the parked calls it satisfies, longest wait first.
fn wake_readers(server: &mut NineServer<Console>, parked: &mut Parked<()>, now: u64) {
    server.fs.drain();
    while server.fs.has_input() {
        let Some(call) = parked.resume_first(server.admission_mut(), |_| true) else { return };
        // A call this thread cannot serve is a server bug; it is gone either way.
        let Ok((request, ())) = call else { continue };
        let _ = serve_or_park(server, parked, request, now);
    }
}

/// Serves until the endpoint is destroyed.
pub fn serve(startup: &Startup) -> u32 {
    let Some(handle) = startup.handle(ENDPOINT) else { return NO_ENDPOINT };
    let endpoint = Endpoint::from_handle(handle);
    let Some(uart) = startup
        .handle(UART_MMIO)
        .map(Mmio::from_handle)
        .and_then(|mmio| mmio.registers().ok())
        .and_then(Uart::new)
    else {
        return NO_UART;
    };
    uart.init();
    if !LIMITS.fits(&COST, BUDGET) {
        return BAD_LIMITS;
    }
    let Ok(random) = redoubt_rt::handle::random_u64() else { return NO_RANDOM };
    let Ok(mut server) = NineServer::new(Console::new(uart), LIMITS, random) else { return BAD_LIMITS };
    // A console read waits on a person, so it has no deadline: what reclaims it is its caller
    // giving up, which arrives as an abandoned-call notice.
    let mut parked: Parked<()> = Parked::new(FOREVER);
    // The interrupt thread, started before any client can be served, so no key press is missed.
    let wake_badge = match NonZeroU64::new(WAKE_BADGE).ok_or(Error::InvalidArgument) {
        Ok(badge) => endpoint.mint(badge, None),
        Err(e) => Err(e),
    };
    let started = startup
        .handle(UART_IRQ)
        .map(Irq::from_handle)
        .zip(wake_badge.ok())
        .map(|(irq, wake)| start_irq_thread(irq, wake));
    if !matches!(started, Some(Ok(()))) {
        return NO_IRQ;
    }
    loop {
        let now = redoubt_rt::handle::time_now().unwrap_or(0);
        wake_readers(&mut server, &mut parked, now);
        match endpoint.receive(FOREVER, 0) {
            Ok(Event::Call(request)) => {
                // A failed reply means the caller is gone; there is nobody to tell.
                let _ = serve_or_park(&mut server, &mut parked, request, now);
            }
            // A wake-up from the interrupt thread; the draining at the top of the loop is what
            // answers it. Anything else sent one-way is dropped, and what it brought closed.
            Ok(Event::Send(delivery)) => {
                for handle in delivery.handles.as_slice().iter().flatten() {
                    let _ = redoubt_rt::handle::close(*handle);
                }
            }
            // A caller gave up on a parked read: replying frees the call and its lend, and the
            // reply reaches nobody (R3).
            Ok(Event::Abandoned(id)) => {
                parked.abandoned(server.admission_mut(), id, &WORDS_9P);
            }
            Ok(Event::Interrupt | Event::Exit(_)) => {}
            Err(Error::Dead) => return redoubt_rt::exit::OK,
            Err(_) => return RECEIVE_FAILED,
        }
    }
}
