//! Every PID in use at once (`tests/process-fill.toml`). This program is `init`, PID 2 and the one
//! process `root` keeps; it carves all of `system`'s processes and all of `users'` into a budget
//! each and starts a child in every one, so PIDs 2 to `MAX_PROCESS_COUNT` (511) are all in use.
//! The next `process_create` must be refused by a budget, never by a panic. Then both budgets are
//! destroyed and every page comes back. Each verdict is a kernel result: the refusals, the exit
//! notices' PIDs and `budget_usage`.
#![no_std]
#![no_main]
use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};

use test_programs::rd::{self, Cause, Error, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

const WAIT: u64 = 5_000_000;
/// The PIDs there are (`MAX_PROCESS_COUNT`, kernel/processes.md): PID 1 is the kernel's.
const PIDS: usize = 511;
/// `system`'s and `users'` processes at boot (kernel/budgets.md, "The tree from the boot
/// manifest"): with `init`'s one in `root`, every PID but the kernel's.
const SYSTEM: u32 = 127;
const USERS: u32 = 382;
static UART: AtomicUsize = AtomicUsize::new(0);

extern "C" fn child(_: usize) -> ! { test_programs::park() }

fn notice(exit: u32) -> rd::ExitNotice {
    match rd::receive(Some(exit), WAIT, 0).expect("exit notice") {
        Received::Exit(n) => n,
        _ => panic!("expected an exit notice"),
    }
}

/// Start a child in `budget` and drop its handle: the PID is the kernel's to keep in use.
fn start(image: &spawn::Image, budget: u32, exit: u32) {
    let c = spawn::spawn(image, budget, exit, child as *const () as usize, &[], &[]).expect("a child");
    rd::close(c.process).expect("close the child's handle");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("uart");
    UART.store(uart, Ordering::Relaxed);
    // SAFETY: this program alone maps the UART, for its whole life.
    let mut out = unsafe { MmioSerialPort::new(uart) };
    out.init();
    let image = spawn::image();
    let exit = rd::endpoint_create().expect("exit endpoint");

    // What one child costs its budget, measured, so the budgets below are carved to fit.
    let probe = rd::create(rd::USERS, &rd::spec(4096, 1, 100)).expect("probe budget");
    start(&image, probe, exit);
    let each = rd::usage(probe).expect("probe usage").pages_usage;
    rd::destroy(probe).expect("destroy the probe");
    assert_eq!(notice(exit).cause, Cause::Killed);
    let before = [rd::ROOT, rd::SYSTEM, rd::USERS].map(|b| rd::usage(b).expect("usage"));

    let sys = rd::create(rd::SYSTEM, &rd::spec(u64::from(SYSTEM) * each, SYSTEM, 1000)).expect("system's");
    let users = rd::create(rd::USERS, &rd::spec(u64::from(USERS) * each, USERS, 1000)).expect("users'");
    for _ in 0..SYSTEM {
        start(&image, sys, exit);
    }
    for _ in 0..USERS {
        start(&image, users, exit);
    }
    writeln!(out, "[process-fill] {} children, {} pages each", SYSTEM + USERS, each).ok();

    // Every PID is in use: each budget refuses the next process, whoever is asked.
    for budget in [sys, users, rd::SYSTEM, rd::USERS, rd::ROOT] {
        assert_eq!(rd::process_create(budget, exit), Err(Error::OutOfProcesses), "budget {budget}");
    }
    writeln!(out, "[process-fill] the next process_create is OutOfProcesses in every budget").ok();

    rd::destroy(sys).expect("destroy system's");
    rd::destroy(users).expect("destroy users'");
    let mut seen = [false; PIDS + 1];
    for _ in 0..SYSTEM + USERS {
        let n = notice(exit);
        assert_eq!(n.cause, Cause::Killed);
        let pid = n.pid as usize;
        assert!((2..=PIDS).contains(&pid) && !seen[pid], "PID {pid} out of range or twice");
        seen[pid] = true;
    }
    // 509 distinct PIDs in 2..=511, and this program's: every one.
    writeln!(out, "[process-fill] PIDs 2 to {PIDS} were all in use at once").ok();

    let after = [rd::ROOT, rd::SYSTEM, rd::USERS].map(|b| rd::usage(b).expect("usage"));
    assert_eq!(after, before);
    writeln!(out, "[process-fill] every page came back: root, system and users as before").ok();
    rd::system_reset(rd::RESET, ResetKind::PowerOff).unwrap();
    test_programs::park()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let uart = UART.load(Ordering::Relaxed);
    if uart != 0 {
        // SAFETY: this program mapped the UART for its whole life; a panic ends normal printing.
        let mut out = unsafe { MmioSerialPort::new(uart) };
        writeln!(out, "[process-fill] FAIL: {info}").ok();
    }
    rd::process_exit(255)
}
