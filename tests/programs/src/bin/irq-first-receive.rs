//! An interrupt raised while its object is masked is delivered by the first `receive` after the
//! handle is handed over (kernel/devices.md, R5). The goldfish RTC's alarm is set to fire while
//! nothing receives on its IRQ object, which the kernel then keeps masked with `fired` set; a new
//! process is given the handle, and its first `receive` must return the interrupt at once.
//!
//! This program runs first, so it holds the RTC and its interrupt. Each round starts a child with
//! the IRQ handle, which reports its first `receive`'s result in its exit code; the judge is that
//! exit notice and the RTC's own clock.

#![no_std]
#![no_main]

use test_programs::rd::{self, Cause, Received};
use test_programs::sched::{Bench, rtc};
use test_programs::spawn;

/// Handovers per boot.
const ROUNDS: u32 = 20;
/// The child's exit codes: the interrupt, a timeout, anything else.
const GOT: u32 = 1;
const TIMED_OUT: u32 = 2;
const OTHER: u32 = 3;
/// How long the child's first `receive` waits: the interrupt is already there.
const FIRST_WAIT: u64 = 100_000;

/// The driver: its IRQ handle is slot 1; exit with what the first `receive` on it got.
extern "C" fn driver(_: usize) -> ! {
    let code = match rd::receive(Some(1), FIRST_WAIT, 0) {
        Ok(Received::Interrupt) => GOT,
        Err(rd::Error::Timeout) => TIMED_OUT,
        _ => OTHER,
    };
    rd::process_exit(code)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let devices = rd::OTHER_DEVICES..rd::log_rx();
    let mut b = Bench::new("irq-first-receive");
    let Some((_, base, irq)) = rtc::find(devices) else {
        b.check(false, format_args!("no goldfish RTC among the device handles"));
        b.finish("IRQ-FIRST-RECEIVE")
    };
    let kids = rd::create(rd::SYSTEM, &rd::spec(600, 1, 10)).expect("the drivers' budget");
    let mut got = 0;
    for round in 0..ROUNDS {
        // Half the rounds raise the line as the alarm is set (a time already past), half a
        // millisecond later; either way nothing is receiving, so the source is masked.
        let now = rtc::now_ns(base);
        rtc::alarm(base, if round % 2 == 0 { now.saturating_sub(1) } else { now + 1_000_000 });
        test_programs::wait_ms(5);
        spawn::spawn(b.image(), kids, b.exit_endpoint(), driver as *const () as usize, &[], &[irq])
            .expect("a driver");
        let code = match rd::receive(Some(b.exit_endpoint()), 5_000_000, 0) {
            Ok(Received::Exit(n)) if n.cause == Cause::Exited => n.code,
            _ => OTHER,
        };
        if code == GOT {
            got += 1;
        } else {
            b.note(format_args!("round {}: the first receive got {}", round, code));
        }
        rtc::clear(base);
    }
    // The control: nothing raised since the last round's interrupt was taken and cleared, so a
    // new process's first receive finds nothing.
    spawn::spawn(b.image(), kids, b.exit_endpoint(), driver as *const () as usize, &[], &[irq])
        .expect("a driver");
    let control = match rd::receive(Some(b.exit_endpoint()), 5_000_000, 0) {
        Ok(Received::Exit(n)) if n.cause == Cause::Exited => n.code,
        _ => OTHER,
    };
    b.check(
        control == TIMED_OUT,
        format_args!("with nothing raised, the first receive after a handover times out"),
    );
    b.check(
        got == ROUNDS,
        format_args!("a masked interrupt reaches the first receive after a handover: {got} of {ROUNDS}"),
    );
    b.finish("IRQ-FIRST-RECEIVE")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("irq-first-receive", info) }
