//! Review regressions: one-way notice flow, zero-PC allocation and unchecked native stacks.
#![no_std]
#![no_main]
use core::{
    fmt::Write,
    sync::atomic::{AtomicUsize, Ordering},
};

use test_programs::{
    rd::{self, Cause, Error, Received, ResetKind},
    spawn,
};
use uart_16550::MmioSerialPort;
const WAIT: u64 = 2_000_000;
static UART: AtomicUsize = AtomicUsize::new(0);
fn message(endpoint: u32) -> rd::Message {
    let Received::Message(m) = rd::receive(Some(endpoint), WAIT, 0).unwrap() else { panic!("message") };
    m
}
fn notice(endpoint: u32) -> rd::ExitNotice {
    let Received::Exit(n) = rd::receive(Some(endpoint), WAIT, 0).unwrap() else { panic!("notice") };
    n
}
extern "C" fn exit_child(_: usize) -> ! { rd::process_exit(41) }
extern "C" fn notice_receiver(arg: usize) -> ! {
    let exit = rd::endpoint_create().unwrap();
    rd::send(1, &rd::body_with([0; 4], &[exit]), None, WAIT).unwrap();
    let delivered = spawn::startup_byte(arg, 0) == 1;
    if delivered {
        let n = notice(exit);
        assert_eq!((n.cause, n.code), (Cause::Exited, 41));
    } else {
        assert_eq!(rd::receive(Some(exit), 100_000, 0), Err(Error::Timeout));
    }
    rd::process_exit(42)
}
extern "C" fn zero_entries(_: usize) -> ! {
    let stack = rd::map_anon(4 * rd::PAGE_SIZE, rd::rw()).unwrap();
    let before = rd::usage(1).unwrap();
    // Neither allocation yields: both zero PCs remain live until this process exits.
    let first = rd::thread_create(0, stack + 4 * rd::PAGE_SIZE - 16, 0).unwrap();
    let second = rd::thread_create(0, stack + 4 * rd::PAGE_SIZE - 16, 0).unwrap();
    assert_ne!(first, second);
    assert_eq!(rd::usage(1).unwrap().pages_usage, before.pages_usage + 2);
    rd::process_exit(43)
}
extern "C" fn touch_stack(_: usize) -> ! {
    let data = [17usize; 32];
    core::hint::black_box(&data);
    rd::process_exit(99)
}
extern "C" fn bad_stack(arg: usize) -> ! {
    let sp = spawn::startup_byte(arg, 0) as usize;
    rd::thread_create(touch_stack as *const () as usize, sp, 0).unwrap();
    loop {
        rd::receive(None, rd::FOREVER, 0).ok();
    }
}
#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).unwrap();
    UART.store(uart, Ordering::Relaxed);
    // SAFETY: this trusted parent exclusively owns its UART mapping.
    let mut out = unsafe { MmioSerialPort::new(uart) };
    out.init();
    writeln!(out).ok();
    let image = spawn::image();
    let root_exit = rd::endpoint_create().unwrap();
    let control = rd::endpoint_create().unwrap();
    for allow in [true, false] {
        let mut dest = rd::spec(250, 1, 20);
        if allow {
            dest.labels.push(7).unwrap();
            dest.labels.push(8).unwrap();
        }
        let receiver = rd::create(rd::USERS, &dest).unwrap();
        spawn::spawn(
            &image,
            receiver,
            root_exit,
            notice_receiver as *const () as usize,
            &[u8::from(allow)],
            &[control],
        )
        .unwrap();
        let endpoint = message(control).body.handles.as_slice()[0].unwrap().index();
        let mut source = rd::spec(250, 1, 20);
        source.labels.push(7).unwrap();
        let source = rd::create(if allow { rd::USERS } else { rd::SYSTEM }, &source).unwrap();
        let empty = rd::usage(source).unwrap();
        let actor =
            spawn::spawn(&image, source, endpoint, exit_child as *const () as usize, &[], &[]).unwrap();
        let n = notice(root_exit);
        assert_eq!((n.cause, n.code), (Cause::Exited, 42));
        assert_eq!(rd::usage(actor.process), Err(Error::BadHandle));
        assert_eq!(rd::usage(source).unwrap(), empty);
        rd::destroy(source).unwrap();
        rd::destroy(receiver).unwrap();
    }
    writeln!(out, "[proc-review] notice labels permit read-up and reject system-source write-down").ok();
    let budget = rd::create(rd::USERS, &rd::spec(300, 1, 20)).unwrap();
    let empty = rd::usage(budget).unwrap();
    spawn::spawn(&image, budget, root_exit, zero_entries as *const () as usize, &[], &[budget]).unwrap();
    let n = notice(root_exit);
    assert_eq!((n.cause, n.code), (Cause::Exited, 43));
    assert_eq!(rd::usage(budget).unwrap(), empty);
    writeln!(out, "[proc-review] zero-entry threads keep distinct IDs and refund exact resources").ok();
    for sp in [0, 1, 16] {
        spawn::spawn(&image, budget, root_exit, bad_stack as *const () as usize, &[sp], &[]).unwrap();
        let n = notice(root_exit);
        assert_eq!((n.cause, n.code), (Cause::Faulted, 15));
        assert_eq!(rd::usage(budget).unwrap(), empty);
    }
    writeln!(out, "[proc-review] invalid native stacks are accepted then fault when executed").ok();
    rd::system_reset(rd::RESET, ResetKind::PowerOff).unwrap();
    test_programs::park()
}
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let uart = UART.load(Ordering::Relaxed);
    if uart != 0 {
        // SAFETY: the parent mapped this UART; panic stops its normal printing.
        let mut out = unsafe { MmioSerialPort::new(uart) };
        writeln!(out, "[proc-review] FAIL: {info}").ok();
    }
    rd::process_exit(255)
}
