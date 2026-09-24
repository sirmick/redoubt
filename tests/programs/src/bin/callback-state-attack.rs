//! The legacy `ReturnToParent` outside any interrupt callback (WP-K5; K5-code-review-2 P1-1).
//!
//! That call used to put the kernel in the state of a running interrupt callback with none
//! running: every budget deadline and slice end was held back for as long as the caller liked,
//! every Redoubt call in the system was refused, and a later legacy `Yield` could stop the
//! kernel. It is refused now. Two children in budgets of their own make the call, check it
//! failed, report that with a Redoubt `send` (which must still work), make a legacy `Yield`,
//! and spin:
//! - one in a lease with a deadline: the deadline still destroys it on time (no later than destroying a
//!   budget of the same shape by hand, plus `LATE_US`);
//! - one in a budget with none: this program, the steward's stand-in, still gets the CPU back (the spinner is
//!   preempted at its slice end) and destroys it with `budget_destroy`.
//!
//! This program is the bundle's only one: it prints on the UART itself and powers off. A kernel
//! that still honoured the call hangs here (the case times out) or panics.

#![no_std]
#![no_main]

use core::fmt::Write;

use redoubt_abi::{PID, SysCall};
use test_programs::rd::{self, Cause, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

/// The most a deadline's notice may arrive after it, in virtual time, beyond what destroying the
/// same budget by hand costs.
const LATE_US: u64 = 1_000;
/// A child's report on the `rep` endpoint: `[REPORT, refused, error code]`.
const REPORT: usize = 1;

fn now() -> u64 { rd::time_now().expect("time_now") }

fn ok(b: bool) -> &'static str { if b { "ok" } else { "FAIL" } }

fn spin() -> ! {
    let mut x = 0u64;
    loop {
        x = x.wrapping_add(1);
        core::hint::black_box(x);
    }
}

extern "C" fn child(arg: usize) -> ! {
    match spawn::startup_byte(arg, 0) {
        // The attacker: the call, a report through a Redoubt call, a legacy yield, and a spin.
        1 => {
            let pid = PID::new(1).expect("PID 1");
            let r = redoubt_abi::rsyscall(SysCall::ReturnToParent(pid, 0));
            let refused = r == Err(redoubt_abi::Error::UnhandledSyscall);
            let code = r.err().map_or(0, |e| e as usize);
            let _ = rd::send(1, &rd::body([REPORT, usize::from(refused), code, 0]), None, rd::FOREVER);
            redoubt_abi::yield_slice();
            spin()
        }
        // The calibration budget's spinner.
        _ => spin(),
    }
}

/// A child's report: whether the call was refused (and its error code), or `None` if no
/// report came.
fn report(rep: u32) -> Option<(bool, usize)> {
    match rd::receive(Some(rep), 2_000_000, 0) {
        Ok(Received::Message(m)) if m.body.words[0] == REPORT => {
            Some((m.body.words[1] == 1, m.body.words[2]))
        }
        _ => None,
    }
}

/// The next exit notice, if it came within two seconds and is a `killed` one blaming nobody.
fn killed(exit: u32) -> bool {
    matches!(rd::receive(Some(exit), 2_000_000, 0), Ok(Received::Exit(n))
        if n.cause == Cause::Killed && n.blamed_account == 0 && n.blamed_labels.as_slice().is_empty())
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("uart");
    // SAFETY: this program is the only one, and owns the UART it just mapped.
    let mut out = unsafe { MmioSerialPort::new(uart) };
    out.init();
    let image = spawn::image();
    let exit = rd::endpoint_create().unwrap();
    let rep = rd::endpoint_create().unwrap();
    let rep_client = rd::mint_from_handle(rep, 1, None).unwrap();

    // What destroying a one-spinner budget by hand costs: the deadline may add only the
    // timer's latency to it.
    let calibration = rd::create(rd::SYSTEM, &rd::spec(300, 1, 10)).unwrap();
    spawn::spawn(&image, calibration, exit, child as *const () as usize, &[2], &[]).unwrap();
    let _ = rd::receive(None, 10_000, 0);
    let started = now();
    rd::destroy(calibration).unwrap();
    let calibrated = killed(exit);
    let cost = now() - started;
    writeln!(out, "[callback-state] {}: calibration destroy took {} µs", ok(calibrated), cost).ok();

    // The lease: its process makes the call, then spins; its deadline destroys it all the same.
    let deadline = now() + 100_000;
    let lease = rd::create(rd::SYSTEM, &rd::BudgetSpec { deadline, ..rd::spec(300, 1, 10) }).unwrap();
    spawn::spawn(&image, lease, exit, child as *const () as usize, &[1], &[rep_client]).unwrap();
    let r = report(rep);
    writeln!(
        out,
        "[callback-state] {}: ReturnToParent refused ({:?}), and a Redoubt send after it delivered",
        ok(r.is_some_and(|(refused, _)| refused)),
        r
    )
    .ok();
    let good = killed(exit);
    let late = now().saturating_sub(deadline);
    writeln!(
        out,
        "[callback-state] {}: the lease was destroyed by its deadline, killed notice blaming nobody",
        ok(good)
    )
    .ok();
    writeln!(
        out,
        "[callback-state] lateness {} ({} µs after the deadline; destroying by hand took {} µs; bound {} µs more)",
        if late <= cost + LATE_US { "ok" } else { "LATE" },
        late,
        cost,
        LATE_US
    )
    .ok();

    // The steward's stand-in: the second attacker spins with no deadline over it, and this
    // program still runs (its slice ends) and destroys it by hand.
    let other = rd::create(rd::SYSTEM, &rd::spec(300, 1, 10)).unwrap();
    spawn::spawn(&image, other, exit, child as *const () as usize, &[1], &[rep_client]).unwrap();
    let r = report(rep);
    let destroyed = rd::destroy(other);
    let good = r.is_some_and(|(refused, _)| refused) && destroyed.is_ok() && killed(exit);
    writeln!(
        out,
        "[callback-state] {}: the steward stand-in destroyed the spinning caller's budget ({:?}, {:?})",
        ok(good),
        r,
        destroyed
    )
    .ok();
    // Every other Redoubt call still works for this program.
    let still = rd::usage(rd::SYSTEM).is_ok() && rd::endpoint_create().is_ok();
    writeln!(out, "[callback-state] {}: Redoubt calls still served", ok(still)).ok();

    writeln!(out, "CALLBACK-STATE-ATTACK TEST PASSED").ok();
    let _ = rd::system_reset(rd::RESET, ResetKind::PowerOff);
    test_programs::park()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // The UART is mapped at a kernel-chosen address; without it, park and let the bench time out.
    let _ = info;
    test_programs::park()
}
