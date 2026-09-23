//! Native exit/lend composition. Only the parent owns the console/reset; verdicts use kernel
//! outcomes, notices and budget accounting. Children receive exactly the handles listed below.
#![no_std]
#![no_main]
use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};

use redoubt_sys::{Call, LendDisposition, Return};
use test_programs::{
    rd::{self, Cause, Error, Received, ResetKind},
    spawn,
};
use uart_16550::MmioSerialPort;
const WAIT: u64 = 2_000_000;
const MAGIC: u64 = 0x1234_5678;
static UART: AtomicUsize = AtomicUsize::new(0);
static THREAD_DONE: AtomicUsize = AtomicUsize::new(0);
fn message(endpoint: u32) -> redoubt_sys::Message {
    match rd::receive(Some(endpoint), WAIT, 0).expect("receive") {
        Received::Message(m) => m,
        _ => panic!("expected call"),
    }
}
fn loan_pages(m: &redoubt_sys::Message) -> redoubt_sys::Pages {
    match m.kind {
        redoubt_sys::MessageKind::Call { lend: Some(pages) } => pages,
        _ => panic!("expected lend"),
    }
}
fn notice(endpoint: u32) -> redoubt_sys::ExitNotice {
    match rd::receive(Some(endpoint), WAIT, 0).expect("exit notice") {
        Received::Exit(n) => n,
        _ => panic!("expected exit"),
    }
}
extern "C" fn child(arg: usize) -> ! {
    match spawn::startup_byte(arg, 0) {
        8 => {
            let stack = rd::map_anon(4 * rd::PAGE_SIZE, rd::rw()).unwrap();
            rd::thread_create(returning_server as *const () as usize, stack + 4 * rd::PAGE_SIZE - 16, 1)
                .unwrap();
            rd::thread_exit().unwrap();
            panic!("initial thread_exit returned");
        }
        1 | 7 => {
            // Native server exits holding parent's live lend.
            let m = message(1);
            rd::poke(loan_pages(&m).addr, MAGIC);
            if spawn::startup_byte(arg, 0) == 7 {
                rd::thread_exit().expect("final server thread exits");
                panic!("final thread_exit returned");
            }
            rd::process_exit(17)
        }
        2 | 5 => {
            // Caller is killed, or a sibling exits its process while the parent holds a loan.
            if spawn::startup_byte(arg, 0) == 5 {
                let stack = rd::map_anon(4 * rd::PAGE_SIZE, rd::rw()).expect("exit stack");
                rd::thread_create(exit_sibling as *const () as usize, stack + 4 * rd::PAGE_SIZE - 16, 0)
                    .unwrap();
            }
            let page = rd::map_anon(rd::PAGE_SIZE, rd::rw()).expect("lend page");
            rd::poke(page, MAGIC);
            rd::call_outcome(1, &rd::body([0; 4]), rd::pages(page, 1), WAIT).ok();
            rd::process_exit(91)
        }
        3 => {
            // Same-process caller and receiver; kill both while lend is live.
            let stack = rd::map_anon(4 * rd::PAGE_SIZE, rd::rw()).expect("stack");
            rd::thread_create(local_caller as *const () as usize, stack + 4 * rd::PAGE_SIZE - 16, 1)
                .expect("caller");
            let m = message(1);
            assert_eq!(rd::peek(loan_pages(&m).addr), MAGIC);
            rd::process_exit(19)
        }
        _ => rd::process_exit(0),
    }
}
extern "C" fn returning_server(endpoint: usize) -> usize {
    let m = message(endpoint as u32);
    rd::poke(loan_pages(&m).addr, MAGIC);
    171
}
extern "C" fn exit_sibling(_: usize) -> ! {
    rd::receive(Some(2), WAIT, 0).expect("exit signal");
    rd::process_exit(23)
}
extern "C" fn local_caller(endpoint: usize) -> ! {
    let page = rd::map_anon(rd::PAGE_SIZE, rd::rw()).expect("page");
    rd::poke(page, MAGIC);
    rd::call_outcome(endpoint as u32, &rd::body([0; 4]), rd::pages(page, 1), WAIT).ok();
    rd::thread_exit().ok();
    test_programs::park()
}
extern "C" fn exiting_server(endpoint: usize) -> ! {
    let m = message(endpoint as u32);
    rd::poke(loan_pages(&m).addr, MAGIC);
    THREAD_DONE.store(1, Ordering::SeqCst);
    rd::thread_exit().ok();
    test_programs::park()
}
fn assert_returned(out: redoubt_sys::CallOutcome) {
    assert_eq!(out.status, Err(Error::Dead));
    assert_eq!(out.lend, LendDisposition::Returned);
    assert!(!out.reply_present);
}
#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("uart");
    UART.store(uart, Ordering::Relaxed);
    // SAFETY: the parent exclusively owns this mapped UART device.
    let mut out = unsafe { MmioSerialPort::new(uart) };
    out.init();
    let image = spawn::image();
    let exit = rd::endpoint_create().expect("exit endpoint");
    let work = rd::endpoint_create().expect("work endpoint");
    let budget = rd::create(rd::USERS, &rd::spec(600, 4, 100)).expect("budget");
    let empty = rd::usage(budget).expect("usage");
    let page = rd::map_anon(rd::PAGE_SIZE, rd::rw()).expect("parent lend");
    spawn::spawn(&image, budget, exit, child as *const () as usize, &[1], &[work]).expect("server");
    let (result, reply) = rd::call_outcome(work, &rd::body([0; 4]), rd::pages(page, 1), WAIT).expect("call");
    assert_returned(result);
    assert!(reply.is_none());
    assert_eq!(rd::peek(page), MAGIC);
    let n = notice(exit);
    assert_eq!((n.cause, n.code), (Cause::Faulted, 17));
    assert_eq!(rd::usage(budget).unwrap(), empty);
    writeln!(out, "[lifecycle] native server exit returns live lend and all child resources").ok();

    spawn::spawn(&image, budget, exit, child as *const () as usize, &[7], &[work]).unwrap();
    let (result, reply) = rd::call_outcome(work, &rd::body([0; 4]), rd::pages(page, 1), WAIT).unwrap();
    assert_returned(result);
    assert!(reply.is_none());
    assert_eq!(rd::peek(page), MAGIC);
    let n = notice(exit);
    assert_eq!((n.cause, n.code), (Cause::Faulted, 0));
    assert_eq!(rd::usage(budget).unwrap(), empty);
    writeln!(out, "[lifecycle] final thread_exit returns live lend and all child resources").ok();

    spawn::spawn(&image, budget, exit, child as *const () as usize, &[8], &[work]).unwrap();
    let (result, reply) = rd::call_outcome(work, &rd::body([0; 4]), rd::pages(page, 1), WAIT).unwrap();
    assert_returned(result);
    assert!(reply.is_none());
    assert_eq!(rd::peek(page), MAGIC);
    let n = notice(exit);
    assert_eq!((n.cause, n.code), (Cause::Faulted, 0));
    assert_eq!(rd::usage(budget).unwrap(), empty);
    writeln!(out, "[lifecycle] returning final worker returns live lend and all child resources").ok();

    let doomed = rd::create(rd::USERS, &rd::spec(600, 2, 100)).expect("doomed");
    spawn::spawn(&image, doomed, exit, child as *const () as usize, &[2], &[work]).expect("caller");
    let m = message(work);
    let loan = loan_pages(&m);
    assert_eq!(rd::peek(loan.addr), MAGIC);
    rd::destroy(doomed).expect("kill caller");
    assert_eq!(notice(exit).cause, Cause::Killed);
    assert_eq!(rd::peek(loan.addr), MAGIC);
    assert_eq!(rd::receive(Some(work), WAIT, 0), Ok(Received::Abandoned(m.msg_id)));
    let rec = rd::body([0; 4]).encode();
    let before = rd::usage(rd::SYSTEM).unwrap();
    let result =
        redoubt_sys::syscall(&Call::Reply { msg_id: m.msg_id, body_rec: rec.as_ptr() as usize }).unwrap();
    assert_eq!(result, Return::Reply(redoubt_sys::ReplyOutcome { delivered: false, installed: 0 }));
    let after = rd::usage(rd::SYSTEM).unwrap();
    assert_eq!(before.pages_usage - after.pages_usage, 2); // one open call, one consumed lend
    assert_eq!(rd::unmap(loan.addr, rd::PAGE_SIZE), Err(Error::InvalidArgument));
    writeln!(out, "[lifecycle] caller death abandons once and discarded reply frees loan and call").ok();

    // A sibling calls process_exit while this native caller is blocked on its lend.
    let control = rd::endpoint_create().unwrap();
    spawn::spawn(&image, budget, exit, child as *const () as usize, &[5], &[work, control]).unwrap();
    let m = message(work);
    let lent = loan_pages(&m);
    rd::send(control, &rd::body([0; 4]), None, WAIT).unwrap();
    let n = notice(exit);
    assert_eq!((n.cause, n.code), (Cause::Exited, 23));
    assert_eq!(rd::usage(budget).unwrap(), empty);
    assert_eq!(rd::peek(lent.addr), MAGIC);
    assert_eq!(rd::receive(Some(work), WAIT, 0), Ok(Received::Abandoned(m.msg_id)));
    let rec = rd::body([0; 4]).encode();
    let result =
        redoubt_sys::syscall(&Call::Reply { msg_id: m.msg_id, body_rec: rec.as_ptr() as usize }).unwrap();
    assert_eq!(result, Return::Reply(redoubt_sys::ReplyOutcome { delivered: false, installed: 0 }));
    assert_eq!(rd::unmap(lent.addr, rd::PAGE_SIZE), Err(Error::InvalidArgument));
    writeln!(out, "[lifecycle] native caller process_exit abandons live lend without a stale PID").ok();

    // A sibling's native thread_exit closes its call while the caller thread survives.
    let stack = rd::map_anon(4 * rd::PAGE_SIZE, rd::rw()).expect("stack");
    let before = rd::usage(rd::SYSTEM).unwrap();
    rd::thread_create(exiting_server as *const () as usize, stack + 4 * rd::PAGE_SIZE - 16, work as usize)
        .unwrap();
    let (result, _) = rd::call_outcome(work, &rd::body([0; 4]), rd::pages(page, 1), WAIT).unwrap();
    assert_returned(result);
    assert_eq!(THREAD_DONE.load(Ordering::SeqCst), 1);
    assert_eq!(rd::peek(page), MAGIC);
    assert_eq!(rd::usage(rd::SYSTEM).unwrap(), before);
    writeln!(out, "[lifecycle] thread exit preserves siblings and returns same-process lend exactly").ok();

    spawn::spawn(&image, budget, exit, child as *const () as usize, &[3], &[work]).expect("self-lend child");
    let n = notice(exit);
    assert_eq!((n.cause, n.code), (Cause::Faulted, 19));
    assert_eq!(rd::usage(budget).unwrap(), empty);
    writeln!(out, "[lifecycle] whole-process exit cleans both sides of same-process loan").ok();

    // Warm the parent's scratch mappings before its exact accounting baseline. More completed
    // children than the PID space guarantees a reused PID, without predicting random allocation.
    spawn::spawn(&image, budget, exit, child as *const () as usize, &[0], &[]).unwrap();
    notice(exit);
    let baseline = rd::usage(rd::SYSTEM).unwrap();
    let mut seen = [false; 256];
    let mut reused = false;
    for _ in 0..260 {
        spawn::spawn(&image, budget, exit, child as *const () as usize, &[0], &[]).unwrap();
        let n = notice(exit);
        assert_eq!(n.cause, Cause::Exited);
        reused |= seen[n.pid as usize];
        seen[n.pid as usize] = true;
        assert_eq!(rd::usage(budget).unwrap(), empty);
        assert_eq!(rd::usage(rd::SYSTEM).unwrap(), baseline);
    }
    assert!(reused);
    writeln!(out, "[lifecycle] 260 exits restore exact budgets and reuse freed PIDs").ok();
    rd::system_reset(rd::RESET, ResetKind::PowerOff).unwrap();
    test_programs::park()
}
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let uart = UART.load(Ordering::Relaxed);
    if uart != 0 {
        // SAFETY: the parent mapped this UART for its lifetime; panic ends normal printing.
        let mut out = unsafe { MmioSerialPort::new(uart) };
        writeln!(out, "[lifecycle] FAIL: {info}").ok();
    }
    rd::process_exit(255)
}
