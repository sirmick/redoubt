//! Destroying a device object's owner budget (kernel/devices.md, "Failure and restart"; R10).
//! Every device object is `system`'s, so this program, in `init`'s place and in `root`, parks a
//! thread in `receive` on the console's IRQ handle and destroys `system`. The waiter gets `Dead`;
//! every device handle it was given is gone, the Reset right's and the console's included, and
//! `system`'s with them; the console mapping made before survives, which is how it keeps printing
//! (a mapping outlives its handle), and a byte the bench sends after the destruction reaches the
//! UART by polling, so the machine is up with its interrupt source owned by nobody. That the
//! source is masked is argued on the page: nothing a process can see tells a masked source from an
//! unowned one the kernel completes and masks on its first claim.
//!
//! It cannot power off: the Reset right dies with `system`. The case ends on its lines.

#![no_std]
#![no_main]

use core::fmt::Write;
use core::sync::atomic::{AtomicU32, Ordering};

use test_programs::rd::{self, Error, Received, ResetKind};
use uart_16550::MmioSerialPort;

/// What the waiter's `receive` got.
static WAITER: AtomicU32 = AtomicU32::new(PARKED);
const PARKED: u32 = 0;
const DEAD: u32 = 1;
const INTERRUPT: u32 = 2;
const OTHER: u32 = 3;
/// How long the main thread waits for the waiter's answer and for the bench's byte, in ms.
const WAIT_MS: u64 = 2_000;

struct Out(MmioSerialPort);

macro_rules! say {
    ($out:expr, $($arg:tt)*) => {{ writeln!($out.0, $($arg)*).ok(); }};
}

/// Prints `ok`/`FAIL`, so a check that fails says so on a line the case forbids.
macro_rules! check {
    ($out:expr, $cond:expr, $($arg:tt)*) => {{
        let ok = $cond;
        write!($out.0, "[device-owner-destroy] {}: ", if ok { "ok" } else { "FAIL" }).ok();
        writeln!($out.0, $($arg)*).ok();
    }};
}

/// The waiter: `receive` on the console's interrupt until the object dies.
fn waiter(_: usize) {
    let got = match rd::receive(Some(rd::CONSOLE_IRQ), rd::FOREVER, 0) {
        Err(Error::Dead) => DEAD,
        Ok(Received::Interrupt) => INTERRUPT,
        _ => OTHER,
    };
    WAITER.store(got, Ordering::SeqCst);
}

/// Poll `what` every 10 ms for up to `WAIT_MS`.
fn poll(mut what: impl FnMut() -> bool) -> bool {
    for _ in 0..WAIT_MS / 10 {
        if what() {
            return true;
        }
        test_programs::wait_ms(10);
    }
    what()
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    // SAFETY: `uart` is the console's register page, mapped for this process by the kernel.
    let mut out = Out(unsafe { MmioSerialPort::new(uart) });
    out.0.init();
    say!(out, "\n[device-owner-destroy] mapped the console");

    let devices = rd::RESET..rd::first_free();
    let answering = devices.clone().filter(|&h| rd::device_info(h).is_ok()).count();
    rd::thread(waiter, 0).expect("the waiter");
    // Let it park in `receive` before its object goes.
    test_programs::wait_ms(50);
    say!(
        out,
        "[device-owner-destroy] destroying system, the owner of every device object, with a thread in receive on the console's interrupt"
    );
    let destroyed = rd::destroy(rd::SYSTEM);
    let answered = poll(|| WAITER.load(Ordering::SeqCst) != PARKED);
    let got = WAITER.load(Ordering::SeqCst);
    check!(
        out,
        destroyed == Ok(()) && answered && got == DEAD,
        "the thread in receive on the console's interrupt got Dead ({:?}; waiter {})",
        destroyed,
        got
    );

    // --- Every handle naming a device object is gone, and `system` with them -------------------
    let gone = devices.clone().filter(|&h| rd::device_info(h) == Err(Error::BadHandle)).count();
    let console = rd::map_device(rd::CONSOLE_MMIO).map(|_| ());
    let irq = rd::receive(Some(rd::CONSOLE_IRQ), 1000, 0).map(|_| ());
    let reset = rd::system_reset(rd::RESET, ResetKind::PowerOff);
    let system = rd::usage(rd::SYSTEM).map(|_| ());
    check!(
        out,
        gone == answering && [console, irq, reset, system].iter().all(|r| *r == Err(Error::BadHandle)),
        "all {} device handles are gone: map_device {:?}, receive on the irq {:?}, system_reset {:?}, and system {:?}",
        gone,
        console,
        irq,
        reset,
        system
    );

    // --- The mapping made before survives, and a byte sent now reaches the UART by polling -----
    say!(out, "[device-owner-destroy] send a byte");
    let byte = poll(|| out.0.try_receive() == Ok(b'y'));
    check!(
        out,
        byte,
        "a byte sent after the destruction reached the UART through the mapping made before it"
    );

    say!(out, "[device-owner-destroy] DEVICE OWNER DESTROY PASSED");
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
