//! Owns the UART and prints on behalf of every other test program. Also the server side of
//! the IPC test, and, when a case grants it the UART's interrupt, an echo of UART input.
//!
//! Attack cases take their verdict from lines an attacker cannot write (redoubt/README.md,
//! "Writing an attack case"). So everything this server prints goes through `console::Console`,
//! whose only way to print a client's bytes is `relay`, which starts every line with the
//! sender's PID as the kernel reported it. The server's own lines are a closed set of templates
//! (`Line`) with numbers in them and nothing a client wrote.

#![no_std]
#![no_main]

use test_programs::{op, SERVER_ADDRESS};
use xous::{MemoryAddress, MemoryFlags, MemoryMessage, Message};

use crate::console::{Console, Line};

/// The ns16550 on QEMU's `virt` machine, and its interrupt.
const UART_BASE: usize = 0x1000_0000;
const UART_IRQ: usize = 10;

mod console {
    use core::fmt::Write;

    use uart_16550::MmioSerialPort;

    /// The UART, private to this module: nothing outside it can write bytes of its choosing.
    pub struct Console {
        port: MmioSerialPort,
    }

    /// Every line the server prints in its own name. No variant carries client text.
    pub enum Line {
        Up(u8),
        ClaimedIrq(usize),
        Listening(u8),
        Scalars([usize; 4]),
        MovedPage(usize),
        Unexpected(usize),
        /// A byte that arrived on the UART (input from the bench), printed escaped.
        Received(usize, char),
    }

    impl Console {
        pub fn new(base: usize) -> Self {
            // SAFETY: `base` is the UART's page, mapped for this process by the caller.
            let mut port = unsafe { MmioSerialPort::new(base) };
            port.init();
            Console { port }
        }

        pub fn say(&mut self, line: Line) {
            let out = &mut self.port;
            match line {
                Line::Up(pid) => writeln!(out, "[server] PID {pid} up"),
                Line::ClaimedIrq(irq) => writeln!(out, "[server] claimed irq {irq}"),
                Line::Listening(pid) => writeln!(out, "[server] PID {pid} listening"),
                Line::Scalars([a, b, c, d]) => writeln!(out, "[server] scalars: {a} {b} {c} {d}"),
                Line::MovedPage(at) => writeln!(out, "[server] moved page at {at:#x}; its text follows"),
                Line::Unexpected(id) => writeln!(out, "[server] unexpected message, id {id}"),
                Line::Received(irq, byte) => writeln!(out, "[server] irq {irq}: received {byte:?}"),
            }
            .ok();
        }

        /// The one way to print a client's bytes: each line starts `[pid N] `, N being the
        /// sender's PID from the kernel. A client cannot choose N, and every newline in its
        /// text starts a new prefixed line, so it can never begin a line of its own. Control
        /// characters become '?' only to keep the log readable.
        pub fn relay(&mut self, sender: xous::MessageSender, text: &str) {
            let pid = sender.pid().map_or(0, |pid| pid.get());
            for line in text.split('\n') {
                write!(self.port, "[pid {pid}] ").ok();
                for c in line.chars() {
                    self.port.write_char(if c.is_control() && c != '\t' { '?' } else { c }).ok();
                }
                writeln!(self.port).ok();
            }
        }

        /// Take whatever arrived on the UART.
        pub fn receive(&mut self) -> Option<u8> { self.port.try_receive().ok() }
    }
}

fn text(msg: &MemoryMessage) -> &str {
    let len = msg.valid.map_or(0, |v| v.get());
    // SAFETY: the kernel lent or moved us `msg.buf`; `valid` is at most its length.
    let bytes = unsafe { core::slice::from_raw_parts(msg.buf.as_ptr(), len.min(msg.buf.len())) };
    core::str::from_utf8(bytes).unwrap_or("<invalid utf-8>")
}

/// Runs in interrupt context whenever the UART has received data (only if the claim was granted).
fn on_uart_irq(_irq: usize, arg: *mut usize) {
    // SAFETY: `arg` is the `Console` in `_start`'s frame, which never returns. The main loop
    // may be mid-line when this runs, so a line can be split; cases send input only while
    // nothing else is printing.
    let console = unsafe { &mut *(arg as *mut Console) };
    while let Some(byte) = console.receive() {
        console.say(Line::Received(UART_IRQ, byte as char));
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let uart = xous::map_memory(MemoryAddress::new(UART_BASE), None, 4096, MemoryFlags::R | MemoryFlags::W)
        .expect("couldn't claim the UART");
    let mut console = Console::new(uart.as_ptr() as usize);
    let pid = xous::current_pid().unwrap().get();
    console.say(Line::Up(pid));
    // Given a grant for the UART's interrupt, echo what arrives on the UART, as this server's
    // own lines: grant-attack, irq-attack and uart-irq use that. Without the grant the claim is
    // refused and nothing changes. The claim comes before the server exists, so before any
    // client can run its first request.
    let arg = &mut console as *mut Console as *mut usize;
    if xous::claim_interrupt(UART_IRQ, on_uart_irq, arg).is_ok() {
        console.say(Line::ClaimedIrq(UART_IRQ));
    }
    let sid = xous::create_server_with_address(SERVER_ADDRESS).expect("couldn't create server");
    console.say(Line::Listening(pid));

    loop {
        // Dropping the envelope returns lent memory to the sender and frees moved memory.
        let mut envelope = xous::receive_message(sid).expect("couldn't receive message");
        let sender = envelope.sender;
        match &mut envelope.body {
            Message::Scalar(m) if m.id == op::PRINT_SCALARS => {
                console.say(Line::Scalars([m.arg1, m.arg2, m.arg3, m.arg4]));
            }
            Message::BlockingScalar(m) if m.id == op::SUM => {
                xous::return_scalar(sender, m.arg1 + m.arg2 + m.arg3 + m.arg4).expect("couldn't reply");
            }
            Message::Borrow(m) if m.id == op::PRINT => console.relay(sender, text(m)),
            Message::MutableBorrow(m) if m.id == op::UPPERCASE => {
                let len = m.valid.map_or(0, |v| v.get()).min(m.buf.len());
                // SAFETY: the kernel lent us `m.buf` writable until we drop the envelope.
                unsafe { core::slice::from_raw_parts_mut(m.buf.as_mut_ptr(), len) }.make_ascii_uppercase();
            }
            Message::Move(m) if m.id == op::PRINT_AND_KEEP => {
                console.say(Line::MovedPage(m.buf.as_ptr() as usize));
                console.relay(sender, text(m));
            }
            Message::Scalar(m) | Message::BlockingScalar(m) => console.say(Line::Unexpected(m.id)),
            Message::Borrow(m) | Message::MutableBorrow(m) | Message::Move(m) => {
                console.say(Line::Unexpected(m.id))
            }
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { test_programs::park() }
