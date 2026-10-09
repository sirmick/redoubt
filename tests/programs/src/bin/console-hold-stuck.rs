//! The console's hold against its holder (kernel/devices.md, "The console's one writer";
//! `bench:console-hold-stuck`), at 2 harts.
//!
//! **A holder that dies holding.** H, a child given the console's registers, takes the hold and
//! writes `[holder] cut off` with no newline, then waits. A child killed now has its kernel line
//! wait for H. Then H is killed: the kernel ends H's line, prints the line that waited, then H's
//! own, each whole.
//!
//! **A holder that never gives it back.** S takes the hold and waits for ever. [`KILLS`] children
//! are killed meanwhile, more lines than the kernel keeps waiting (4 KiB, about 107). The kernel
//! never waits for S: every kill goes through, and every line comes out, a queue's worth at a
//! time when it fills and the rest when the machine powers off. The verdict is the kernel's own
//! lines: each kill's, and `system_reset`'s, which comes only if every kill went through.
#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::console::{self, Console};
use test_programs::rd::{self, Cause, Received};
use test_programs::sched::Bench;
use test_programs::spawn;

/// Children killed while S holds the console.
const KILLS: usize = 120;
const WAIT: u64 = 5_000_000;
const READY: usize = 1;

/// A child's handles: its inbox in slots 1 to 4 and the console's registers in slot 5, where
/// `console` looks for them (`rd::CONSOLE_MMIO`).
fn handles(inbox: u32) -> [u32; 5] { [inbox, inbox, inbox, inbox, rd::CONSOLE_MMIO] }

/// H: takes the hold for a line it never ends.
extern "C" fn holder(_: usize) -> ! {
    let Ok((uart, _)) = rd::map_device(rd::CONSOLE_MMIO) else { rd::process_exit(1) };
    console::init(uart);
    // No newline: `Console` keeps the hold until the line ends, which it never does.
    let _ = write!(Console, "[holder] cut off");
    let _ = rd::send_waiting(1, &rd::body([READY, 0, 0, 0]), None, WAIT);
    test_programs::park()
}

/// S: takes the hold and keeps it, writing nothing.
extern "C" fn stuck(_: usize) -> ! {
    if rd::console_hold(rd::CONSOLE_MMIO, rd::Hold::Take).is_err() {
        rd::process_exit(1);
    }
    let _ = rd::send_waiting(1, &rd::body([READY, 0, 0, 0]), None, WAIT);
    test_programs::park()
}

/// A child that waits to be killed.
extern "C" fn victim(_: usize) -> ! { test_programs::park() }

fn ready(inbox: u32) -> bool {
    matches!(rd::receive(Some(inbox), WAIT, 0), Ok(Received::Message(m)) if m.body.words[0] == READY)
}

/// Starts a child in a budget of its own and kills it: whether its notice says killed.
fn kill_one(image: &spawn::Image, exit: u32) -> bool {
    let budget = rd::create(rd::SYSTEM, &rd::spec(200, 1, 10)).expect("a child's budget");
    spawn::spawn(image, budget, exit, victim as *const () as usize, &[], &[]).expect("a child");
    rd::destroy(budget).expect("destroying the child's budget");
    matches!(rd::receive(Some(exit), WAIT, 0), Ok(Received::Exit(n)) if n.cause == Cause::Killed)
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("console-stuck");
    let image = spawn::image();
    let exit = b.exit_endpoint();
    let inbox = rd::endpoint_create().expect("the inbox");

    let hb = rd::create(rd::SYSTEM, &rd::spec(400, 1, 10)).expect("H's budget");
    spawn::spawn(&image, hb, exit, holder as *const () as usize, &[], &handles(inbox)).expect("H");
    let held = ready(inbox);
    let first = kill_one(&image, exit);
    rd::destroy(hb).expect("destroying H's budget");
    let h_killed =
        matches!(rd::receive(Some(exit), WAIT, 0), Ok(Received::Exit(n)) if n.cause == Cause::Killed);
    b.check(held && first && h_killed, format_args!("H held the console, a child and then H were killed"));

    let sb = rd::create(rd::SYSTEM, &rd::spec(400, 1, 10)).expect("S's budget");
    spawn::spawn(&image, sb, exit, stuck as *const () as usize, &[], &handles(inbox)).expect("S");
    // From here on nothing of this program's is printed: S holds the console.
    let s_holds = ready(inbox);
    let killed = (0..KILLS).filter(|_| kill_one(&image, exit)).count();
    if !s_holds || killed != KILLS {
        // No `system_reset` line: the case fails at its deadline.
        test_programs::park()
    }
    let _ = rd::system_reset(rd::RESET, rd::ResetKind::PowerOff);
    test_programs::park()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("console-stuck", info) }
