//! A destruction feeds no receiver it is about to kill, even one running in a live budget whose
//! process object the destruction frees (R4b, R10; kernel/ipc.md, kernel/processes.md "Exit
//! notices").
//!
//! This program runs first and judges. It makes budgets B and L under `system` and starts P in
//! B, reporting to the judge's exit endpoint E, with E's receive right and a handle to L. P
//! creates C in L: C runs in a live budget, but its process object is charged to P's budget, B.
//! C gets E's receive right too, tells the judge it is ready and calls `receive` on E. Only C
//! signals: the judge is blocked in `receive` from the moment it starts P, and a wake never
//! preempts (kernel/scheduling.md), so C is blocked in `receive` before the judge runs again.
//!
//! Then the judge destroys B. P runs in B, so it is killed first, and its notice, `killed`, is
//! pumped to E while C still receives there. C is about to be killed by the same destruction,
//! because its creator's budget is dying, so it must not take P's notice. Every verdict is what
//! the kernel answered: the judge's `receive` on E gets P's notice, and nothing after it, since
//! C's own notice is never made (R10: its creator's budget is destroyed).

#![no_std]
#![no_main]

use test_programs::rd::{self, Cause, ExitNotice, Received};
use test_programs::sched::Bench;
use test_programs::spawn;

const WAIT: u64 = 5_000_000;
/// How long the judge waits for a notice once B is gone: P's is pending by then, if kept.
const NOTICE_WAIT: u64 = 1_000_000;
/// The message tag C sends before it receives.
const READY: usize = 1;

// --- P, with E in slot 1, the judge's inbox in slot 2 and L in slot 3 --------------------------

/// Start C in L, reporting to E and holding E and the inbox, then sleep.
extern "C" fn creator(_: usize) -> ! {
    let image = spawn::image();
    if spawn::spawn(&image, 3, 1, child as *const () as usize, &[], &[1, 2]).is_err() {
        rd::process_exit(1);
    }
    test_programs::park()
}

// --- C, with E in slot 1 and the judge's inbox in slot 2 ---------------------------------------

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
    let mut b = Bench::new("destroy-keeps-notices-creator");
    let image = spawn::image();
    let exit = b.exit_endpoint();
    let inbox = rd::endpoint_create().expect("the inbox");

    let budget_b = rd::create(rd::SYSTEM, &rd::spec(1600, 1, 10)).expect("B");
    let budget_l = rd::create(rd::SYSTEM, &rd::spec(1600, 1, 10)).expect("L");
    let entry = creator as *const () as usize;
    spawn::spawn(&image, budget_b, exit, entry, &[], &[exit, inbox, budget_l]).expect("P");
    b.check(ready(inbox), format_args!("P started C in L, and C is receiving on E"));

    rd::destroy(budget_b).expect("destroying B");

    let n1 = notice(exit, NOTICE_WAIT);
    let n2 = notice(exit, NOTICE_WAIT);
    b.check(
        n1.as_ref().is_some_and(|n| n.cause == Cause::Killed),
        format_args!("E holds P's notice, cause killed (pid {})", n1.as_ref().map_or(0, |n| n.pid)),
    );
    b.check(n2.is_none(), format_args!("E holds nothing else: C's notice is never made"));
    b.finish("DESTROY-KEEPS-NOTICES-CREATOR")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    test_programs::sched::panicked("destroy-keeps-notices-creator", info)
}
