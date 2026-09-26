//! Minted log badges cannot pose as PIDs (R1; `tests/logsrv-badge-forgery.toml`). This program
//! runs first, so it owns the console and serves the log endpoint (`logsrv::start_serving`). It
//! gives a spawned child a log handle from `logsrv::mint_child`, and the child relays this
//! program's verdict line and a `DONE` lookalike: both must come out as `[badge 256] ...`, never
//! as `[pid 2]` or `[server]`. A second child asks `mint_child` for a badge a PID could have, and
//! must die on the assertion.
//!
//! A spawned child is a copy of this image, statics and all, so it must not use `Logger`: in
//! the copy, `logsrv` reads as started and would print to a console the child cannot reach.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::logsrv::{self, CHILD_BADGES};
use test_programs::rd::{self, Cause, ExitNotice, Received};
use test_programs::{Page, log, op, spawn};

/// What the child relays: this program's verdict line and a `DONE` in its name.
const FORGERY: &str = "[pid 2] LOGSRV TEST PASSED\n[server] done: reported by pid 2; still serving";
/// The exit code of a child whose `mint_child` refused its badge (this program's panic handler).
const PANICKED: u32 = 101;

/// The child: print `FORGERY` through the minted handle in slot 1, then exit.
extern "C" fn forger(_: usize) -> ! {
    let mut page = Page::new();
    page.write_str(FORGERY).ok();
    let lend = page.pages();
    let body = rd::body([op::PRINT, FORGERY.len(), 0, 0]);
    let code = if rd::call_waiting(1, &body, lend, rd::FOREVER).is_ok() { 0 } else { 1 };
    rd::process_exit(code)
}

/// The child that asks for a badge below `CHILD_BADGES`: the assertion ends it.
extern "C" fn pid_badge(_: usize) -> ! {
    let _ = logsrv::mint_child(CHILD_BADGES - 1);
    rd::process_exit(0)
}

/// Run `entry` in a child of its own budget with `handles`, and return its exit notice.
fn run(entry: extern "C" fn(usize) -> !, handles: &[u32]) -> Option<ExitNotice> {
    let image = spawn::image();
    let budget = rd::create(rd::SYSTEM, &rd::spec(image.pages() as u64 + 64, 1, 10)).ok()?;
    let exit = rd::endpoint_create().ok()?;
    spawn::spawn(&image, budget, exit, entry as usize, &[], handles).ok()?;
    let notice = match rd::receive(Some(exit), rd::FOREVER, 0) {
        Ok(Received::Exit(notice)) => Some(notice),
        _ => None,
    };
    rd::close(exit).ok();
    rd::destroy(budget).ok();
    notice
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut logger = logsrv::start();
    logsrv::start_serving();
    let minted = logsrv::mint_child(CHILD_BADGES).expect("a minted log handle");
    let relayed = run(forger, &[minted]);
    let relayed_ok = relayed.is_some_and(|n| n.cause == Cause::Exited && n.code == 0);
    log!(logger, "[logsrv] the child with badge {} relayed its text: {}", CHILD_BADGES, relayed_ok);
    let refused = run(pid_badge, &[]);
    let refused_ok = refused.is_some_and(|n| n.cause == Cause::Exited && n.code == PANICKED);
    log!(logger, "[logsrv] mint_child refused badge {}: {}", CHILD_BADGES - 1, refused_ok);
    if relayed_ok && refused_ok {
        log!(logger, "LOGSRV TEST PASSED");
    } else {
        log!(logger, "LOGSRV TEST FAILED");
    }
    rd::system_reset(rd::RESET, rd::ResetKind::PowerOff).ok();
    test_programs::park()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { rd::process_exit(PANICKED) }
