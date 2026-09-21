//! Owns the UART and prints on behalf of every other test program. Also the server side of
//! the IPC test, and, when it holds the console's device objects, an echo of UART input.
//!
//! It takes the UART through `map_device` on the console MMIO handle and waits for input in
//! `receive` on the console IRQ handle (KERNEL-SPEC.md, R5: the kernel masks the source when
//! it fires and the next receive unmasks it; there is no acknowledge call). Those handles are
//! the kernel's gift to the bundle's **first** program (INTERIM, kernel `device.rs`); in a case
//! where this server is not first it falls back to the legacy grant path and does not echo.
//!
//! Attack cases take their verdict from lines an attacker cannot write (docs/testbench.md,
//! "Writing an attack case"). So everything this server prints goes through `console::Console`,
//! whose only way to print a client's bytes is `relay`, which starts every line with the
//! sender's PID as the kernel reported it. The server's own lines are a closed set of templates
//! (`Line`) with numbers in them and nothing a client wrote.

#![no_std]
#![no_main]

use test_programs::{op, rd, SERVER_ADDRESS};
use redoubt_abi::{MemoryAddress, MemoryFlags, MemoryMessage, Message};

use crate::console::{Console, Line};

/// The ns16550 on QEMU's `virt` machine: only for the legacy fallback, where a grant, not a
/// device handle, is the authority.
const UART_BASE: usize = 0x1000_0000;

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
        ConsoleIrq,
        Listening(u8),
        Scalars([usize; 4]),
        MovedPage(usize),
        Unexpected(usize),
        /// A byte that arrived on the UART (input from the bench), printed escaped.
        Received(char),
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
                Line::ConsoleIrq => writeln!(out, "[server] holding the console irq"),
                Line::Listening(pid) => writeln!(out, "[server] PID {pid} listening"),
                Line::Scalars([a, b, c, d]) => writeln!(out, "[server] scalars: {a} {b} {c} {d}"),
                Line::MovedPage(at) => writeln!(out, "[server] moved page at {at:#x}; its text follows"),
                Line::Unexpected(id) => writeln!(out, "[server] unexpected message, id {id}"),
                Line::Received(byte) => writeln!(out, "[server] irq: received {byte:?}"),
            }
            .ok();
        }

        /// The one way to print a client's bytes: each line starts `[pid N] `, N being the
        /// sender's PID from the kernel. A client cannot choose N, and every newline in its
        /// text starts a new prefixed line, so it can never begin a line of its own. Control
        /// characters become '?' only to keep the log readable.
        pub fn relay(&mut self, sender: redoubt_abi::MessageSender, text: &str) {
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

/// A thread of its own, blocked in `receive` on the console's IRQ handle (R5). It echoes
/// every byte the bench types, as the server's own line, which is the evidence the attack
/// cases take their verdict from.
fn uart_irq(arg: usize) -> ! {
    // SAFETY: `arg` is the `Console` in `_start`'s frame, which never returns, so the pointer
    // stays valid. The main thread may be mid-line when a byte arrives, so a line can be
    // split; cases send input only while nothing else is printing.
    let console = unsafe { &mut *(arg as *mut Console) };
    loop {
        match rd::receive(Some(rd::CONSOLE_IRQ), rd::FOREVER, 0) {
            // One byte per interrupt, deliberately: the FIFO is left with data in it, so
            // every byte needs its own interrupt and the next `receive` must unmask the
            // source again. (It does not catch a kernel that forgets to *mask* a fired
            // source: QEMU's 16550 raises the controller once per byte pushed rather than
            // from a continuing level, so nothing storms. `uart-irq` says so.)
            Ok(rd::Received::Interrupt) => {
                if let Some(byte) = console.receive() {
                    console.say(Line::Received(byte as char));
                }
            }
            // Nothing else can arrive on an IRQ handle; a refusal means the handle is gone.
            _ => test_programs::park(),
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // One call decides both: holding the console's MMIO object means this server is the
    // bundle's first program, so it holds the console's IRQ object too (INTERIM until `init`
    // hands each program its own handles, WP-R3). A case that runs this server later in the
    // bundle -- the budget cases do -- falls back to the legacy grant path, which lives until
    // WP-K6, and echoes nothing.
    let device = rd::map_device(rd::CONSOLE_MMIO).map(|(at, _len)| at);
    let uart = device.unwrap_or_else(|_| {
        redoubt_abi::map_memory(MemoryAddress::new(UART_BASE), None, 4096, MemoryFlags::R | MemoryFlags::W)
            .expect("couldn't claim the UART")
            .as_ptr() as usize
    });
    let mut console = Console::new(uart);
    let pid = redoubt_abi::current_pid().unwrap().get();
    console.say(Line::Up(pid));
    // With the console's IRQ handle, echo what arrives on the UART as this server's own lines:
    // grant-attack, irq-attack and uart-irq take their verdict from those. The thread starts
    // before the server exists, so before any client can run its first request.
    if device.is_ok() {
        let arg = &mut console as *mut Console as usize;
        redoubt_abi::create_thread_1(uart_irq, arg).expect("couldn't spawn the console's irq thread");
        console.say(Line::ConsoleIrq);
    }
    let sid = redoubt_abi::create_server_with_address(SERVER_ADDRESS).expect("couldn't create server");
    console.say(Line::Listening(pid));

    loop {
        // Dropping the envelope returns lent memory to the sender and frees moved memory.
        let mut envelope = redoubt_abi::receive_message(sid).expect("couldn't receive message");
        let sender = envelope.sender;
        match &mut envelope.body {
            Message::Scalar(m) if m.id == op::PRINT_SCALARS => {
                console.say(Line::Scalars([m.arg1, m.arg2, m.arg3, m.arg4]));
            }
            Message::BlockingScalar(m) if m.id == op::SUM => {
                redoubt_abi::return_scalar(sender, m.arg1 + m.arg2 + m.arg3 + m.arg4).expect("couldn't reply");
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
