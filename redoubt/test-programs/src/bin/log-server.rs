//! Owns the UART and prints on behalf of every other test program. Also the server
//! side of the IPC test.

#![no_std]
#![no_main]

use core::fmt::Write;

use test_programs::{op, SERVER_ADDRESS};
use uart_16550::MmioSerialPort;
use xous::{MemoryAddress, MemoryFlags, MemoryMessage, Message};

/// The ns16550 on QEMU's `virt` machine, and its interrupt.
const UART_BASE: usize = 0x1000_0000;
const UART_IRQ: usize = 10;

/// Runs in interrupt context whenever the UART has received data (only if the claim was granted).
fn on_uart_irq(_irq: usize, arg: *mut usize) {
    // SAFETY: `arg` is the `MmioSerialPort` in `_start`'s frame, which never returns. The main
    // loop may be mid-line when this runs, so a line can be split; cases send input only while
    // nothing else is printing.
    let port = unsafe { &mut *(arg as *mut MmioSerialPort) };
    while let Ok(byte) = port.try_receive() {
        writeln!(port, "[server] irq {}: received {:?}", UART_IRQ, byte as char).ok();
    }
}

fn text(msg: &MemoryMessage) -> &str {
    let len = msg.valid.map_or(0, |v| v.get());
    let bytes = unsafe { core::slice::from_raw_parts(msg.buf.as_ptr(), len) };
    core::str::from_utf8(bytes).unwrap_or("<invalid utf-8>")
}

/// Print a client's text with every line starting `[pid N] `, where N is the sender's PID as
/// the kernel reported it in the envelope. A client cannot choose N, cannot start a line of
/// its own (each newline gets the prefix again), and cannot hide one with control characters
/// (they become '?'). So a line that does not start with `[pid ` came from the kernel, the
/// loader or this server, and a line that does names its writer: attack cases rely on this to
/// take their verdict from anyone but the attacker (redoubt/README.md, "Writing an attack case").
fn print_attributed(out: &mut MmioSerialPort, sender: xous::MessageSender, text: &str) {
    let pid = sender.pid().map_or(0, |pid| pid.get());
    for line in text.split('\n') {
        write!(out, "[pid {pid}] ").ok();
        for c in line.chars() {
            out.write_char(if c.is_control() && c != '\t' { '?' } else { c }).ok();
        }
        writeln!(out).ok();
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let uart = xous::map_memory(MemoryAddress::new(UART_BASE), None, 4096, MemoryFlags::R | MemoryFlags::W)
        .expect("couldn't claim the UART");
    let mut out = unsafe { MmioSerialPort::new(uart.as_ptr() as usize) };
    out.init();

    writeln!(out, "[server] PID {} up", xous::current_pid().unwrap()).ok();
    // Given a grant for the UART's interrupt, also echo what arrives on the UART, as this
    // server's own lines: grant-attack uses that to show the attacker did not take the
    // interrupt. Without the grant the claim is refused and nothing changes. The claim comes
    // before the server exists, so before any client can run its first request.
    let port = &mut out as *mut MmioSerialPort as *mut usize;
    if xous::claim_interrupt(UART_IRQ, on_uart_irq, port).is_ok() {
        writeln!(out, "[server] claimed irq {UART_IRQ}").ok();
    }
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
            Message::Borrow(m) if m.id == op::PRINT => print_attributed(&mut out, sender, text(m)),
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
