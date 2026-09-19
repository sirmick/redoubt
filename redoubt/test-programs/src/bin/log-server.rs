//! Owns the UART and prints on behalf of every other test program. Also the server
//! side of the IPC test.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::{op, SERVER_ADDRESS};
use uart_16550::MmioSerialPort;
use xous::{MemoryAddress, MemoryFlags, MemoryMessage, Message};

/// The ns16550 on QEMU's `virt` machine.
const UART_BASE: usize = 0x1000_0000;

fn text(msg: &MemoryMessage) -> &str {
    let len = msg.valid.map_or(0, |v| v.get());
    let bytes = unsafe { core::slice::from_raw_parts(msg.buf.as_ptr(), len) };
    core::str::from_utf8(bytes).unwrap_or("<invalid utf-8>")
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let uart = xous::map_memory(MemoryAddress::new(UART_BASE), None, 4096, MemoryFlags::R | MemoryFlags::W)
        .expect("couldn't claim the UART");
    let mut out = unsafe { MmioSerialPort::new(uart.as_ptr() as usize) };
    out.init();

    let sid = xous::create_server_with_address(SERVER_ADDRESS).expect("couldn't create server");
    writeln!(out, "[server] PID {} listening", xous::current_pid().unwrap()).ok();

    loop {
        // Dropping the envelope returns lent memory to the sender and frees moved memory.
        let mut envelope = xous::receive_message(sid).expect("couldn't receive message");
        let sender = envelope.sender;
        match &mut envelope.body {
            Message::Scalar(m) if m.id == op::PRINT_SCALARS => {
                writeln!(out, "[server] scalars: {} {} {} {}", m.arg1, m.arg2, m.arg3, m.arg4).ok();
            }
            Message::BlockingScalar(m) if m.id == op::SUM => {
                xous::return_scalar(sender, m.arg1 + m.arg2 + m.arg3 + m.arg4).expect("couldn't reply");
            }
            Message::Borrow(m) if m.id == op::PRINT => {
                writeln!(out, "{}", text(m)).ok();
            }
            Message::MutableBorrow(m) if m.id == op::UPPERCASE => {
                let len = m.valid.map_or(0, |v| v.get());
                unsafe { core::slice::from_raw_parts_mut(m.buf.as_mut_ptr(), len) }.make_ascii_uppercase();
            }
            Message::Move(m) if m.id == op::PRINT_AND_KEEP => {
                writeln!(out, "[server] moved page at {:p}: {}", m.buf.as_ptr(), text(m)).ok();
            }
            other => {
                writeln!(out, "[server] unexpected message: {:?}", other).ok();
            }
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
