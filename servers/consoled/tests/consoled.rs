//! `consoled`, the whole program, as a fake process with a fake ns16550: both its threads run,
//! it maps the device's registers through `map_device`, waits on the IRQ handle, and serves
//! `/dev/cons` over 9P to clients that call it across "address spaces".
//!
//! The test plays the part of the hardware: it writes the receive register and sets "data
//! ready", then fires the interrupt. The registers are plain memory (nothing can intercept the
//! driver's loads and stores on the host), so this device never clears "data ready" by itself;
//! `line_quiet` puts it back, and one test leaves it set on purpose, because a device that says
//! "data ready" for ever is exactly the hostile case the drain has to survive.
//!
//! The 9P conformance vectors are in `tests/vectors.rs`.

#[path = "../../../libs/rt/tests/common/mod.rs"]
mod common;

use std::time::{Duration, Instant};

use common::fake;
use redoubt_consoled::MAX_INPUT;
use redoubt_consoled::uart::FIFO;
use redoubt_rt::abi::Handle;
use redoubt_rt::client::{Client, ClientError};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::{Startup, StartupBuilder};

#[path = "../src/bin/consoled.rs"]
mod consoled;

/// The ns16550's registers, as this test drives them.
const RBR_THR: usize = 0;
const IER: usize = 1;
const LCR: usize = 3;
const MCR: usize = 4;
const LSR: usize = 5;
const LSR_DATA_READY: u8 = 0x01;
const LSR_THR_EMPTY: u8 = 0x20;
/// What `map_device` reports: the eight registers of a 16550.
const REGISTERS: usize = 8;

/// Runs `main` as `pid` with a startup block.
fn launch(pid: usize, block: Vec<u8>, main: fn(&Startup) -> u32) -> std::thread::JoinHandle<u32> {
    fake().run(pid, move || {
        let startup = Startup::parse(&block).expect("the launcher's block parses");
        main(&startup)
    })
}

/// The block `init` writes for `consoled`: the endpoint it receives on, and its two device
/// handles.
fn block(receive: Handle, mmio: Handle, irq: Handle) -> Vec<u8> {
    let highest = [receive, mmio, irq].iter().map(|h| h.index()).max().unwrap();
    let mut builder = StartupBuilder::new(highest);
    builder
        .handle(consoled::ENDPOINT, receive)
        .handle(consoled::UART_MMIO, mmio)
        .handle(consoled::UART_IRQ, irq);
    builder.finish().expect("the block")
}

/// A `consoled` up and running: its process, its endpoint's receive right, its device handles
/// and the thread it runs on.
struct Box_ {
    server: usize,
    receive: Handle,
    mmio: Handle,
    irq: Handle,
    thread: std::thread::JoinHandle<u32>,
}

fn boot() -> Box_ {
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let (mmio, irq) = f.device(server, REGISTERS);
    // The transmitter has room from the start; nothing else is set.
    f.registers(server, mmio)[LSR] = LSR_THR_EMPTY;
    let thread = launch(server, block(receive, mmio, irq), consoled::serve);
    let b = Box_ { server, receive, mmio, irq, thread };
    // `init()` ran before anything else: 8N1, the FIFOs on, OUT2 (without which no byte ever
    // raises an interrupt) and the receive interrupt enabled.
    wait_until("the UART is initialised", || b.regs()[IER] == 0x01);
    assert_eq!((b.regs()[LCR], b.regs()[MCR]), (0x03, 0x0b));
    b
}

impl Box_ {
    fn regs(&self) -> &'static mut [u8] { fake().registers(self.server, self.mmio) }

    /// The hardware delivers `byte` on the line and raises the interrupt.
    fn types(&self, byte: u8) {
        let regs = self.regs();
        regs[RBR_THR] = byte;
        regs[LSR] |= LSR_DATA_READY;
        fake().fire(self.server, self.irq);
    }

    /// The line goes quiet again: nothing more is ready to read.
    fn line_quiet(&self) { self.regs()[LSR] &= !LSR_DATA_READY; }

    /// What was last written to the transmit register.
    fn printed(&self) -> u8 { self.regs()[RBR_THR] }

    /// A client process holding a connection of its own, as `init` would give it.
    fn client(&self) -> (usize, Handle) {
        let f = fake();
        let client = f.process(1001, &[]);
        (client, f.grant(self.server, self.receive, client, 0x20 + client as u64))
    }

    fn shut_down(self) -> u32 {
        fake().destroy(self.server, self.receive);
        self.thread.join().unwrap()
    }
}

/// Waits for a condition the server reaches on its own; a deterministic wait on real state, not
/// a sleep. Failing it is a hang the test turns into a clear failure.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if done() {
            return;
        }
        std::thread::yield_now();
    }
    panic!("timed out waiting for {what}");
}

/// The case BUILD-PLAN.md names: **typing on the UART reaches a 9P reader**. The reader's call
/// is parked (the server is holding it open with nothing to answer), the interrupt thread wakes
/// the serving thread, and the same call is answered with the byte that was typed.
#[test]
fn typing_on_the_uart_reaches_a_ninep_reader() {
    let b = boot();
    let f = fake();
    let (reader, conn) = b.client();

    let reading = f.run(reader, move || {
        let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
        c.attach(0, "").unwrap();
        c.open(0, mode::OREAD).unwrap();
        let mut got = [0u8; 1];
        assert_eq!(c.read(0, 0, &mut got).unwrap(), 1);
        u32::from(got[0])
    });

    // The read is parked: the server has taken the call and owes a reply it cannot give yet.
    wait_until("the read to be parked", || f.open_calls(b.server) == 1);
    // And the server is not blocked by it: another client is served while it waits.
    let (writer, wconn) = b.client();
    f.as_process(writer, || {
        let mut c = Client::new(Endpoint::from_handle(wconn), 4).unwrap();
        c.attach(1, "").unwrap();
        c.open(1, mode::OWRITE).unwrap();
        assert_eq!(c.write(1, 0, b"!").unwrap(), 1);
    });
    assert_eq!(b.printed(), b'!', "a 9P write goes out of the UART");
    assert_eq!(f.open_calls(b.server), 1, "the read is still parked");

    // Now someone types.
    b.types(b'z');
    assert_eq!(reading.join().unwrap(), u32::from(b'z'));
    b.line_quiet();

    assert_eq!(b.shut_down(), redoubt_rt::exit::OK);
}

/// A read with nothing to read **waits**; it does not answer "no bytes", which a client would
/// read as the end of the console. When its caller gives up, the abandoned-call notice frees
/// the call, and the server carries on serving.
#[test]
fn a_read_with_no_input_waits_and_is_freed_when_its_caller_gives_up() {
    let b = boot();
    let f = fake();
    let (reader, conn) = b.client();

    f.as_process(reader, || {
        let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
        c.timeout = 150_000;
        c.attach(0, "").unwrap();
        c.open(0, mode::OREAD).unwrap();
        let mut got = [0u8; 8];
        // It waited rather than answering; the wait is the client's timeout, not a short read.
        assert_eq!(c.read(0, 0, &mut got).unwrap_err(), ClientError::Sys(redoubt_rt::abi::Error::Timeout));
    });
    // The server was told the call was abandoned and replied, which frees it and its lend (R3).
    wait_until("the abandoned call to be freed", || f.open_calls(b.server) == 0);

    // Still serving.
    let (writer, wconn) = b.client();
    f.as_process(writer, || {
        let mut c = Client::new(Endpoint::from_handle(wconn), 4).unwrap();
        c.attach(1, "").unwrap();
        c.open(1, mode::OWRITE).unwrap();
        assert_eq!(c.write(1, 0, b"k").unwrap(), 1);
    });
    assert_eq!(b.printed(), b'k');

    assert_eq!(b.shut_down(), redoubt_rt::exit::OK);
}

/// Every byte of a sequence goes out of the UART in order, one 9P write each (the test can only
/// see the transmit register between writes, which is why each is checked on its own).
#[test]
fn writes_go_out_of_the_uart_in_order() {
    let b = boot();
    let f = fake();
    let (writer, conn) = b.client();
    f.as_process(writer, || {
        let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
        c.attach(0, "").unwrap();
        c.open(0, mode::ORDWR).unwrap();
        for (i, byte) in b"hello, world\n".iter().enumerate() {
            assert_eq!(c.write(0, i as u64, &[*byte]).unwrap(), 1);
            assert_eq!(b.printed(), *byte, "byte {i}");
        }
    });
    assert_eq!(b.shut_down(), redoubt_rt::exit::OK);
}

/// A device that says "data ready" for ever does not hang the server: the drain is bounded by
/// the FIFO's depth, the ring by its own limit, and the console keeps serving.
#[test]
fn a_device_stuck_on_data_ready_does_not_hang_the_server() {
    let b = boot();
    let f = fake();
    // The line never goes quiet after this.
    b.types(b'x');
    let (client, conn) = b.client();
    f.as_process(client, || {
        let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
        c.attach(0, "").unwrap();
        c.open(0, mode::ORDWR).unwrap();
        // Reads are answered (with the byte the device keeps handing over) rather than hanging.
        for _ in 0..64 {
            let mut got = [0u8; 8];
            let n = c.read(0, 0, &mut got).unwrap();
            assert!(n > 0 && got[..n].iter().all(|b| *b == b'x'));
        }
        // And writes still get through.
        assert_eq!(c.write(0, 0, b"#").unwrap(), 1);
    });
    assert_eq!(b.printed(), b'#');
    assert_eq!(b.shut_down(), redoubt_rt::exit::OK);
}

/// A flood of input with nobody reading keeps what was typed first: the ring takes
/// [`MAX_INPUT`] bytes and drops the rest, rather than overwriting the oldest, so a reader gets
/// the start of what was typed, in order, and the console carries on once it has been read.
///
/// This device hands over whatever its receive register holds each time the driver looks, so
/// how many copies of a byte one drain takes is not the test's to choose; only their order is.
/// Each byte is held on the line across two calls the server answers, which puts one whole drain
/// of it ([`FIFO`] bytes) between them: 64 bytes are enough to fill the ring, and the 65th has
/// nowhere to go. The count of dropped bytes (`Console::dropped`) is inside the server and not
/// visible here; what shows the drop is that the last byte never reaches the reader.
#[test]
fn a_flood_of_input_keeps_what_was_typed_first() {
    let b = boot();
    let f = fake();
    let (client, conn) = b.client();
    f.as_process(client, || {
        let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
        c.attach(0, "").unwrap();
        let typed: Vec<u8> = (0..=(MAX_INPUT / FIFO) as u8).map(|i| b'0' + i).collect();
        for byte in &typed {
            b.types(*byte);
            // Refused walks, not writes: a write would put its byte in the same register.
            for _ in 0..2 {
                assert_eq!(c.walk(0, 1, "anything").unwrap_err(), ClientError::Remote);
            }
        }
        b.line_quiet();

        // Room for one byte more than the ring may hold, and exactly the ring's limit comes back.
        c.open(0, mode::OREAD).unwrap();
        let mut got = vec![0u8; MAX_INPUT + 1];
        assert_eq!(c.read(0, 0, &mut got).unwrap(), MAX_INPUT);
        got.truncate(MAX_INPUT);
        // A run of each byte in the order it was typed, from the very first: nothing skipped or
        // overwritten.
        got.dedup();
        assert!(typed.starts_with(&got), "{got:?}");
        // And the last, typed with the ring already full, was dropped.
        assert!(got.len() < typed.len());

        // Once read, the ring has room again.
        b.types(b'!');
        let mut next = [0u8; 1];
        assert_eq!(c.read(0, 0, &mut next).unwrap(), 1);
        assert_eq!(next[0], b'!');
        b.line_quiet();
    });
    assert_eq!(b.shut_down(), redoubt_rt::exit::OK);
}

/// What `/dev/cons` refuses: modes that mean nothing on a console, walking below it, and
/// creating or removing anything.
#[test]
fn the_console_refuses_what_it_is_not() {
    let b = boot();
    let f = fake();
    let (client, conn) = b.client();
    f.as_process(client, || {
        let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
        c.attach(0, "").unwrap();
        for bad in [mode::OEXEC, mode::OREAD | mode::OTRUNC, mode::OWRITE | mode::OTRUNC] {
            assert_eq!(c.open(0, bad).unwrap_err(), ClientError::Remote, "{bad:#x}");
        }
        // There is nothing below /dev/cons.
        assert_eq!(c.walk(0, 1, "anything").unwrap_err(), ClientError::Remote);
        // A read of an unopened fid, and a write to one opened for reading.
        let mut got = [0u8; 4];
        assert_eq!(c.read(0, 0, &mut got).unwrap_err(), ClientError::Remote);
        c.open(0, mode::OREAD).unwrap();
        assert_eq!(c.write(0, 0, b"x").unwrap_err(), ClientError::Remote);
    });
    assert_eq!(b.shut_down(), redoubt_rt::exit::OK);
}

/// A startup block missing a device handle stops the server rather than leaving a console that
/// prints but never hears anything (TENETS.md 2, fail closed).
#[test]
fn a_console_with_no_device_does_not_start() {
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let (mmio, irq) = f.device(server, REGISTERS);

    let mut only_endpoint = StartupBuilder::new(receive.index());
    only_endpoint.handle(consoled::ENDPOINT, receive);
    let thread = launch(server, only_endpoint.finish().unwrap(), consoled::serve);
    assert_eq!(thread.join().unwrap(), consoled::NO_UART);

    // The UART but no interrupt: a read would wait for ever, so it does not start either.
    let highest = mmio.index().max(receive.index());
    let mut no_irq = StartupBuilder::new(highest);
    no_irq.handle(consoled::ENDPOINT, receive).handle(consoled::UART_MMIO, mmio);
    let thread = launch(server, no_irq.finish().unwrap(), consoled::serve);
    assert_eq!(thread.join().unwrap(), consoled::NO_IRQ);

    // A mapping too short to be a 16550 is not one.
    let short = f.process(0, &[]);
    let short_receive = f.endpoint(short);
    let (short_mmio, short_irq) = f.device(short, 4);
    let thread = launch(short, block(short_receive, short_mmio, short_irq), consoled::serve);
    assert_eq!(thread.join().unwrap(), consoled::NO_UART);
    let _ = irq;
}
