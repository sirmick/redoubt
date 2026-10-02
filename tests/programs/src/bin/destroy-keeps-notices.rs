//! A destruction feeds no receiver it is about to kill (R4b; kernel/ipc.md, kernel/processes.md
//! "Exit notices").
//!
//! This program runs first and judges. It makes budget B and starts two children in it, P1 and
//! P2, each reporting to the judge's exit endpoint E, which the judge's own budget owns, and each
//! holding E's receive right. Each child tells the judge it is ready and then calls `receive` on
//! E. A wake never preempts (kernel/scheduling.md), so each child is blocked in `receive` before
//! the judge, woken by its signal, runs again.
//!
//! Then the judge destroys B. Its processes end one at a time, and the first to end has its
//! notice, `killed`, pumped to E while the other child is still receiving there. That child is
//! about to be killed by the same destruction, so it must not take the notice: both notices
//! must be left on E for the judge. Every verdict is what the kernel answered: the notices the
//! judge's `receive` returns, their PIDs and causes. The creator is not told a PID at
//! `process_create`, so two notices from two distinct PIDs is the check.

#![no_std]
#![no_main]

use test_programs::rd::{self, Cause, ExitNotice, Received};
use test_programs::sched::Bench;
use test_programs::spawn;

const WAIT: u64 = 5_000_000;
/// How long the judge waits for a notice once B is gone: both are pending by then, if kept.
const NOTICE_WAIT: u64 = 1_000_000;
/// The message tag a child sends before it receives.
const READY: usize = 1;

// --- A child, with E in slot 1 and the judge's inbox in slot 2 ---------------------------------

/// Say it is ready, then wait on E. On a correct kernel the `receive` never returns.
extern "C" fn child(_: usize) -> ! {
    if rd::send_waiting(2, &rd::body([READY, 0, 0, 0]), None, WAIT).is_err() {
        rd::process_exit(1);
    }
    let _ = rd::receive(Some(1), rd::FOREVER, 0);
    test_programs::park()
}

// --- The judge ----------------------------------------------------------------------------------

fn ready(inbox: u32) -> bool {
    matches!(rd::receive(Some(inbox), WAIT, 0), Ok(Received::Message(m)) if m.body.words[0] == READY)
}

fn notice(exit: u32, timeout: u64) -> Option<ExitNotice> {
    match rd::receive(Some(exit), timeout, 0) {
        Ok(Received::Exit(n)) => Some(n),
        _ => None,
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("destroy-keeps-notices");
    let image = spawn::image();
    let exit = b.exit_endpoint();
    let inbox = rd::endpoint_create().expect("the inbox");

    let budget = rd::create(rd::SYSTEM, &rd::spec(1600, 2, 10)).expect("B");
    let entry = child as *const () as usize;
    spawn::spawn(&image, budget, exit, entry, &[], &[exit, inbox]).expect("P1");
    let first = ready(inbox);
    spawn::spawn(&image, budget, exit, entry, &[], &[exit, inbox]).expect("P2");
    let second = ready(inbox);
    b.check(first && second, format_args!("P1 and P2 are both receiving on E"));

    rd::destroy(budget).expect("destroying B");

    let n1 = notice(exit, NOTICE_WAIT);
    let n2 = notice(exit, NOTICE_WAIT);
    let killed = |n: &Option<ExitNotice>| n.as_ref().is_some_and(|n| n.cause == Cause::Killed);
    let pid = |n: &Option<ExitNotice>| n.as_ref().map_or(0, |n| n.pid);
    b.check(killed(&n1), format_args!("E holds a first notice, cause killed (pid {})", pid(&n1)));
    b.check(
        killed(&n2) && pid(&n2) != pid(&n1),
        format_args!("E holds a second notice, cause killed, for the other child (pid {})", pid(&n2)),
    );
    b.finish("DESTROY-KEEPS-NOTICES")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("destroy-keeps-notices", info) }
