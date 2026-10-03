//! The worst walk, measured (`tests/worst-walk.toml`; kernel/ipc.md and kernel/timer.md,
//! "Residual risks"). This program is `init`, PID 2. It starts a child in a budget of its own
//! for every other PID, each holding `MAX_THREADS` threads, so every thread the PIDs allow
//! exists at once. Every child's first thread then waits until one shared deadline, so one timer
//! interrupt ends them all and one reconcile wakes every child's budget. Then this program makes
//! one receive's pump and destroys one child's budget. The times are the kernel's trace's, read by
//! the bench (`walk-trace`); what this program checks is that the threads were all there.
#![no_std]
#![no_main]
use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};

use redoubt_sys::MAX_THREADS;
use test_programs::rd::{self, Cause, Received, ResetKind};
use test_programs::spawn;
use uart_16550::MmioSerialPort;

const WAIT: u64 = 5_000_000;
/// How long a holder's start, or one walk of every thread, may take: under icount, seconds.
const REPORT: u64 = 600_000_000;
/// `system`'s and `users'` processes at boot (kernel/budgets.md, "The tree from the boot
/// manifest"): with `init`'s one in `root`, every PID but the kernel's.
const SYSTEM: usize = 127;
const USERS: usize = 382;
/// Holders whose report is held until every holder is in, and then answered with one deadline:
/// what one process may hold open (`MAX_OPEN_CALLS`, 256), less a margin.
const HELD: usize = 250;
/// How far ahead the deadline is, µs: after every held report has its reply.
const DEADLINE: u64 = 1_000_000;
/// Pages a holder's budget has above the probe's usage, for what its start takes and gives back.
const SPARE: u64 = 16;
/// The holder's send right to this program, in its slot 1.
const PARENT: u32 = 1;
static UART: AtomicUsize = AtomicUsize::new(0);

extern "C" fn parked(_arg: usize) -> ! { test_programs::park() }

/// One send to this program's own endpoint, whose receive is the pump measured.
fn sender(to: usize) { rd::send(to as u32, &rd::body([0; 4]), None, REPORT).expect("send") }

/// A child: every thread it may have, then a report (its threads, and the error that stopped it,
/// 0 for none: it has no console); its first thread then waits until the deadline the reply
/// names (0 for none).
extern "C" fn holder(_arg: usize) -> ! {
    let mut threads = 1;
    let mut error = 0;
    match rd::map_anon((MAX_THREADS - 1) * rd::PAGE_SIZE, rd::rw()) {
        Ok(stacks) => {
            while threads < MAX_THREADS {
                let stack = stacks + threads * rd::PAGE_SIZE - 16;
                match rd::thread_create(parked as *const () as usize, stack, 0) {
                    Ok(_) => threads += 1,
                    Err(e) => {
                        error = e as usize;
                        break;
                    }
                }
            }
        }
        Err(e) => error = e as usize,
    }
    let deadline = match rd::call(PARENT, &rd::body([threads, error, 0, 0]), None, rd::FOREVER) {
        Ok(reply) => reply.words[0] as u64,
        Err(e) => rd::process_exit(1000 + e as u32),
    };
    loop {
        let now = rd::time_now().expect("time");
        if now >= deadline {
            break;
        }
        let _ = rd::receive(None, deadline - now, 0);
    }
    test_programs::park()
}

/// Start a holder in `budget` and take its report, unanswered: its threads, and the call's id.
fn start(image: &spawn::Image, budget: u32, exit: u32, calls: u32, to: u32) -> (usize, u64) {
    let c = spawn::spawn(image, budget, exit, holder as *const () as usize, &[], &[to]).expect("a child");
    rd::close(c.process).expect("close the child's handle");
    let Ok(Received::Message(m)) = rd::receive(Some(calls), REPORT, 0) else {
        // No report: the holder's end says why (1000 + its call's error, or a fault).
        let n = rd::receive(Some(exit), WAIT, 0);
        panic!("no report from a holder; its end: {n:?}")
    };
    let [threads, error, ..] = m.body.words;
    assert!(threads == MAX_THREADS, "a holder has {threads} threads, stopped by error {error}");
    (threads, m.msg_id.get())
}

/// Answer a holder's report with `deadline`.
fn answer(call: u64, deadline: u64) {
    rd::reply(call, &rd::body([deadline as usize, 0, 0, 0])).expect("reply")
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
    let calls = rd::endpoint_create().expect("the holders' endpoint");
    let to = rd::mint_from_handle(calls, 1, None).expect("a send right");

    // What one holder costs its budget, measured on a probe.
    let probe = rd::create(rd::USERS, &rd::spec(4096, 1, 1)).expect("probe budget");
    let (_, call) = start(&image, probe, exit, calls, to);
    answer(call, 0);
    let each = rd::usage(probe).expect("probe usage").pages_usage;
    rd::destroy(probe).expect("destroy the probe");
    let Received::Exit(n) = rd::receive(Some(exit), WAIT, 0).expect("the probe's notice") else {
        panic!("expected the probe's exit notice")
    };
    assert_eq!(n.cause, Cause::Killed);

    let mut threads = 1;
    let mut victim = 0;
    // The last `HELD` holders' reports wait; the others are answered at once, with no deadline.
    let mut held = [0u64; HELD];
    let mut i = 0;
    for (parent, n) in [(rd::SYSTEM, SYSTEM), (rd::USERS, USERS)] {
        for _ in 0..n {
            let budget = rd::create(parent, &rd::spec(each + SPARE, 1, 1)).expect("a holder's budget");
            let (t, call) = start(&image, budget, exit, calls, to);
            if i < SYSTEM + USERS - HELD {
                answer(call, 0);
            } else {
                held[i - (SYSTEM + USERS - HELD)] = call;
            }
            (threads, victim, i) = (threads + t, budget, i + 1);
        }
    }
    writeln!(out, "[worst-walk] {} holders of {each} pages, {threads} threads live", SYSTEM + USERS).ok();
    // Every holder is in: the held ones are answered with one deadline, so one timer interrupt
    // ends all their waits and one reconcile wakes all their budgets.
    let deadline = rd::time_now().expect("time") + DEADLINE;
    for call in held {
        answer(call, deadline);
    }
    let sent = rd::time_now().expect("time");
    writeln!(out, "[worst-walk] {HELD} holders waited for one deadline: {}", sent < deadline).ok();

    // The deadline's expiry, then one receive's pump: a thread's send, and the receive here that
    // takes it.
    let mine = rd::endpoint_create().expect("an endpoint");
    let wait = (deadline + 50_000).saturating_sub(rd::time_now().expect("time")).max(1);
    let _ = rd::receive(Some(mine), wait, 0);
    let to_mine = rd::mint_from_handle(mine, 2, None).expect("a send right");
    rd::thread(sender, to_mine as usize).expect("the sender");
    let Received::Message(_) = rd::receive(Some(mine), REPORT, 0).expect("receive") else {
        panic!("expected the message")
    };

    // One destruction: the last holder's budget, its 255 threads with it.
    rd::destroy(victim).expect("destroy a holder's budget");
    let Received::Exit(n) = rd::receive(Some(exit), REPORT, 0).expect("the holder's notice") else {
        panic!("expected the holder's exit notice")
    };
    writeln!(out, "[worst-walk] one holder destroyed, killed: {}", n.cause == Cause::Killed).ok();
    writeln!(out, "WORST-WALK DONE").ok();
    rd::system_reset(rd::RESET, ResetKind::PowerOff).unwrap();
    test_programs::park()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let uart = UART.load(Ordering::Relaxed);
    if uart != 0 {
        // SAFETY: this program mapped the UART for its whole life; a panic ends normal printing.
        let mut out = unsafe { MmioSerialPort::new(uart) };
        writeln!(out, "[worst-walk] FAIL: {} at {:?}", info.message(), info.location()).ok();
    }
    rd::process_exit(255)
}
