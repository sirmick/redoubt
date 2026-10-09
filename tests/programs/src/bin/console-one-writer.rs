//! The console's one writer (kernel/devices.md, "The console's one writer"; `bench:console-one-writer`),
//! at 2 harts under plain TCG, where the harts run at once.
//!
//! A writer thread prints long lines through the console without a pause, on one hart, each
//! inside the console's hold. On the other, the main thread starts and kills [`KILLS`] children,
//! and the kernel prints `[!] Terminating process with PID n` for each. A kernel line that arrives
//! while the writer holds the console waits, whole, until the writer's line ends. So every kernel
//! line and every writer line comes out whole: the bench counts the kernel's lines by an anchored
//! pattern, and refuses a writer's line with anything inside it.
#![no_std]
#![no_main]

use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use test_programs::console::Console;
use test_programs::rd::{self, Cause, Received};
use test_programs::sched::Bench;
use test_programs::spawn;

/// Children started and killed.
const KILLS: usize = 40;
/// The `#`s of each writer line: a line takes a while at the UART, so kills land inside many.
const WIDTH: usize = 96;
const WAIT: u64 = 5_000_000;

static STOP: AtomicBool = AtomicBool::new(false);
static WRITTEN: AtomicUsize = AtomicUsize::new(0);
static STOPPED: AtomicBool = AtomicBool::new(false);

/// [`WIDTH`] `#`s.
struct Wide;

impl fmt::Display for Wide {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for _ in 0..WIDTH {
            f.write_char('#')?;
        }
        Ok(())
    }
}

/// The writer: lines until told to stop, then how many.
fn writer(_: usize) {
    let mut n = 0;
    while !STOP.load(Ordering::Acquire) {
        let _ = writeln!(Console, "[console] line {} {}", n, Wide);
        n += 1;
    }
    WRITTEN.store(n, Ordering::Release);
    STOPPED.store(true, Ordering::Release);
}

/// A child: it waits to be killed.
extern "C" fn victim(_: usize) -> ! { test_programs::park() }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut b = Bench::new("console");
    let image = spawn::image();
    let exit = b.exit_endpoint();
    rd::thread(writer, 0).expect("the writer");
    // The writer is writing on the other hart meanwhile.
    test_programs::wait_ms(5);
    let mut killed = 0;
    for _ in 0..KILLS {
        let budget = rd::create(rd::SYSTEM, &rd::spec(200, 1, 10)).expect("a child's budget");
        spawn::spawn(&image, budget, exit, victim as *const () as usize, &[], &[]).expect("a child");
        rd::destroy(budget).expect("destroying the child's budget");
        if matches!(rd::receive(Some(exit), WAIT, 0), Ok(Received::Exit(n)) if n.cause == Cause::Killed) {
            killed += 1;
        }
    }
    STOP.store(true, Ordering::Release);
    let mut waited = 0;
    while !STOPPED.load(Ordering::Acquire) && waited < 5_000 {
        test_programs::wait_ms(1);
        waited += 1;
    }
    let written = WRITTEN.load(Ordering::Acquire);
    b.check(killed == KILLS, format_args!("{} of {} children killed", killed, KILLS));
    b.check(written > 0, format_args!("the writer wrote {} lines beside the kills", written));
    b.finish("CONSOLE-ONE-WRITER")
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! { test_programs::sched::panicked("console", info) }
