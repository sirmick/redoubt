//! The bundle's first program by default (docs/testbench.md, rule F): it owns the console,
//! echoes UART input, and serves the log endpoint (`test_programs::logsrv`) for every other
//! program. Beyond logging it answers `TAKE_GIFTS` (the budgets, once, never a device) and
//! `DONE` (the attack checker: it names the reporter and powers off).
//!
//! It takes the UART through `map_device` on the console MMIO handle and waits for input in
//! `receive` on the console IRQ handle (KERNEL-SPEC.md, R5: the kernel masks the source when
//! it fires and the next receive unmasks it; there is no acknowledge call).
//!
//! Attack cases take their verdict from lines an attacker cannot write (docs/testbench.md,
//! "Writing an attack case"): a client's bytes print only through `logsrv`'s relay, prefixed
//! with its badge, and this server's own lines are `logsrv::Line`'s closed set of templates.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicBool, Ordering};

use test_programs::logsrv::{self, Line};
use test_programs::rd::{self, MessageKind, ResetKind};
use test_programs::{console, op};

/// Set by the echo thread just before its first `receive`: the source is masked until then (R5).
static ECHOING: AtomicBool = AtomicBool::new(false);

/// A thread of its own, blocked in `receive` on the console's IRQ handle (R5). It echoes
/// every byte the bench types, as the server's own line, which is the evidence the attack
/// cases take their verdict from.
extern "C" fn uart_irq(_: usize) -> ! {
    ECHOING.store(true, Ordering::Release);
    loop {
        match rd::receive(Some(rd::CONSOLE_IRQ), rd::FOREVER, 0) {
            // One byte per interrupt, deliberately: the FIFO is left with data in it, so
            // every byte needs its own interrupt and the next `receive` must unmask the
            // source again. (It does not catch a kernel that forgets to *mask* a fired
            // source: QEMU's 16550 raises the controller once per byte pushed rather than
            // from a continuing level, so nothing storms. `uart-irq` says so.)
            Ok(rd::Received::Interrupt) => {
                if let Some(byte) = console::receive() {
                    logsrv::say(Line::Received(byte as char));
                }
            }
            // Nothing else can arrive on an IRQ handle; a refusal means the handle is gone.
            _ => test_programs::park(),
        }
    }
}

extern "C" fn legacy(_: usize) -> ! { logsrv::serve_legacy() }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    logsrv::start();
    logsrv::say(Line::Up);
    // Echo what arrives on the UART as this server's own lines: irq-attack and uart-irq take
    // their verdict from those. The thread starts before any client can run its first request.
    rd::thread(uart_irq, 0).expect("couldn't spawn the console's irq thread");
    // The bench types only after `Listening`, so the echo thread must be in `receive` by then.
    while !ECHOING.load(Ordering::Acquire) {
        test_programs::wait_ms(1);
    }
    logsrv::say(Line::ConsoleIrq);
    rd::thread(legacy, 0).expect("couldn't spawn the legacy server");
    logsrv::say(Line::Listening);
    let mut given = false;
    logsrv::serve(|m| {
        let MessageKind::Call { .. } = m.kind else { return false };
        let id = m.msg_id.get();
        match m.body.words[0] {
            op::TAKE_GIFTS if given => {
                rd::reply(id, &rd::body([rd::Error::Refused as usize, 0, 0, 0])).ok();
            }
            op::TAKE_GIFTS => {
                // The budgets only, never a device (R2). Message handles are copied
                // (KERNEL-SPEC.md), so this server then closes its own.
                rd::reply(id, &rd::body_with([0; rd::WORDS], &[rd::ROOT, rd::SYSTEM, rd::USERS])).ok();
                for budget in [rd::ROOT, rd::SYSTEM, rd::USERS] {
                    rd::close(budget).ok();
                }
                given = true;
                logsrv::say(Line::GiftsGiven(m.badge));
            }
            op::DONE => {
                // The badge is the kernel's: nobody can report as another.
                logsrv::say(Line::Done(m.badge));
                rd::reply(id, &rd::body([0; rd::WORDS])).ok();
                rd::system_reset(rd::RESET, ResetKind::PowerOff).ok();
                test_programs::park()
            }
            _ => return false,
        }
        true
    })
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
