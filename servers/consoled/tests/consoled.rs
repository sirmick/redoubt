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

use std::time::{Duration, Instant};

use redoubt_consoled::uart::FIFO;
use redoubt_consoled::{HOLD_CHUNK, MAX_INPUT};
use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{FOREVER, Handle, MAX_THREADS};
use redoubt_rt::client::{ClientError, Connection, Lend};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Buffer;
use redoubt_rt::server::ninep::{COLLECT_WAIT, IN_WORDS, OPENED, collect_words, mode};
use redoubt_rt::startup::{Startup, StartupBuilder};
use redoubt_rt::wire::ninep::{Body, ErrorName, Message};
use redoubt_rt::wire::proto::consol;

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
/// handles, with `extra` arguments after `buckets=4`.
fn block(receive: Handle, mmio: Handle, irq: Handle, extra: &[&str]) -> Vec<u8> {
    let highest = [receive, mmio, irq].iter().map(|h| h.index()).max().unwrap();
    let mut builder = StartupBuilder::new(highest);
    builder
        .handle(consoled::ENDPOINT, receive)
        .handle(consoled::UART_MMIO, mmio)
        .handle(consoled::UART_IRQ, irq)
        .arg("buckets=4");
    for arg in extra {
        builder.arg(arg);
    }
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

fn boot() -> Box_ { boot_with(&[]) }

/// [`boot`], with `extra` arguments.
fn boot_with(extra: &[&str]) -> Box_ {
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let (mmio, irq) = f.device(server, REGISTERS);
    // The transmitter has room from the start; nothing else is set.
    f.registers(server, mmio)[LSR] = LSR_THR_EMPTY;
    let thread = launch(server, block(receive, mmio, irq, extra), consoled::serve);
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
        self.holds(byte);
        fake().fire(self.server, self.irq);
    }

    /// `byte` is on the line, with no interrupt: the driver sees it only when it next looks.
    fn holds(&self, byte: u8) {
        let regs = self.regs();
        regs[RBR_THR] = byte;
        regs[LSR] |= LSR_DATA_READY;
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

    /// Stops the server, once its interrupt thread is waiting again (which unmasks the source,
    /// R5). The server may have found a key before that key's wake-up arrived; a wake-up still
    /// being sent when the endpoint dies would end the interrupt thread in `thread_exit`, which the
    /// fake kernel does not model. For a test that never fires, the source is already unmasked.
    fn shut_down(self) -> u32 {
        let f = fake();
        wait_until("the interrupt thread to wait again", || !f.masked(self.server, self.irq));
        f.destroy(self.server, self.receive);
        self.thread.join().unwrap()
    }
}

/// Waits for a condition the server reaches on its own; a deterministic wait on real state, not
/// a sleep. Failing it is a hang the test turns into a clear failure, so the guard is the runtime's
/// fake kernel's own: 60 s, far beyond any step here however loaded the machine.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if done() {
            return;
        }
        std::thread::yield_now();
    }
    panic!("timed out waiting for {what}");
}

/// The central case: **typing on the UART reaches a 9P reader**. The reader's call
/// is parked (the server is holding it open with nothing to answer), the interrupt thread wakes
/// the serving thread, and the same call is answered with the byte that was typed.
#[test]
fn typing_on_the_uart_reaches_a_ninep_reader() {
    let b = boot();
    let f = fake();
    let (reader, conn) = b.client();
    // Attached and opened first, so that the read is the reader's only call below.
    f.as_process(reader, || {
        let c = Connection::new(Endpoint::from_handle(conn));
        let mut lend = Lend::new(4).unwrap();
        c.attach(&mut lend, 0, "").unwrap();
        c.open(&mut lend, 0, mode::OREAD).unwrap();
    });

    let reading = f.run(reader, move || {
        let c = Connection::new(Endpoint::from_handle(conn));
        let mut lend = Lend::new(4).unwrap();
        let mut got = [0u8; 1];
        assert_eq!(c.read(&mut lend, 0, 0, &mut got).unwrap(), 1);
        u32::from(got[0])
    });

    // The read is parked: the server has taken the call and owes a reply it cannot give yet.
    wait_until("the read to be parked", || f.open_calls(b.server) == 1);
    // And the server is not blocked by it: another client is served while it waits.
    let (writer, wconn) = b.client();
    f.as_process(writer, || {
        let c = Connection::new(Endpoint::from_handle(wconn));
        let mut lend = Lend::new(4).unwrap();
        c.attach(&mut lend, 1, "").unwrap();
        c.open(&mut lend, 1, mode::OWRITE).unwrap();
        assert_eq!(c.write(&mut lend, 1, 0, b"!").unwrap(), 1);
    });
    assert_eq!(b.printed(), b'!', "a 9P write goes out of the UART");
    assert_eq!(f.open_calls(b.server), 1, "the read is still parked");

    // Now someone types.
    b.types(b'z');
    assert_eq!(reading.join().unwrap(), u32::from(b'z'));
    b.line_quiet();

    assert_eq!(b.shut_down(), redoubt_rt::exit::OK);
}

/// A multiplexed read with nothing to read waits in the skeleton, with no one-call read parked
/// beside it, and is answered into the parked completion call when someone types.
#[test]
fn a_multiplexed_read_waits_for_input() {
    let b = boot();
    let f = fake();
    let (reader, conn) = b.client();
    let collect = move |hold| {
        let call = Endpoint::from_handle(conn).call(
            &collect_words(hold),
            &[],
            Some(Buffer::new(1).unwrap()),
            FOREVER,
        );
        let (reply, lend) = call.into_result().unwrap();
        let lend = lend.unwrap();
        (reply.words, lend[..reply.words[1] as usize].to_vec())
    };
    f.as_process(reader, || {
        let c = Connection::new(Endpoint::from_handle(conn));
        let mut lend = Lend::new(4).unwrap();
        c.attach(&mut lend, 0, "").unwrap();
        c.open(&mut lend, 0, mode::OREAD).unwrap();
        assert_eq!(collect(0).0, [0, 0, OPENED, 0], "the session opens");
        // A read of one byte, its T-message in the send's words.
        let mut bytes = [0u8; IN_WORDS];
        let n =
            Message { tag: 7, body: Body::Tread { fid: 0, offset: 0, count: 1 } }.encode(&mut bytes).unwrap();
        assert!(n <= IN_WORDS);
        let mut words = [0u64; 4];
        for (word, chunk) in words[1..].iter_mut().zip(bytes.chunks(8)) {
            *word = u64::from_le_bytes(chunk.try_into().unwrap());
        }
        Endpoint::from_handle(conn).send(&words, &[], None, FOREVER).unwrap();
    });
    let reading = f.run(reader, move || {
        let (words, bytes) = collect(COLLECT_WAIT);
        let got = Message::decode(&bytes).map(|m| (m.tag, m.body));
        u32::from(words[0] == 0 && matches!(got, Ok((7, Body::Rread { data: b"q" }))))
    });
    wait_until("the completion call to be taken", || f.open_calls(b.server) == 1);
    // One thread serves both, so once another client's write is answered the completion call
    // has been served, and the read is waiting: what is typed now comes after it.
    let (writer, wconn) = b.client();
    f.as_process(writer, || {
        let c = Connection::new(Endpoint::from_handle(wconn));
        let mut lend = Lend::new(4).unwrap();
        c.attach(&mut lend, 1, "").unwrap();
        c.open(&mut lend, 1, mode::OWRITE).unwrap();
        assert_eq!(c.write(&mut lend, 1, 0, b"!").unwrap(), 1);
    });
    assert_eq!(f.open_calls(b.server), 1, "the completion call is still parked");
    let typed = Instant::now();
    b.types(b'q');
    assert_eq!(reading.join().unwrap(), 1, "the read was answered with what was typed");
    // When the input came, not when the completion call's hold ran out.
    assert!(typed.elapsed() < Duration::from_micros(COLLECT_WAIT / 2), "answered at the hold's end");
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
        let mut c = Connection::new(Endpoint::from_handle(conn));
        let mut lend = Lend::new(4).unwrap();
        c.timeout = 150_000;
        c.attach(&mut lend, 0, "").unwrap();
        c.open(&mut lend, 0, mode::OREAD).unwrap();
        let mut got = [0u8; 8];
        // It waited rather than answering; the wait is the client's timeout, not a short read.
        assert_eq!(
            c.read(&mut lend, 0, 0, &mut got).unwrap_err(),
            ClientError::Sys(redoubt_rt::abi::Error::Timeout)
        );
    });
    // The server was told the call was abandoned and replied, which frees it and its lend (R3).
    wait_until("the abandoned call to be freed", || f.open_calls(b.server) == 0);

    // Still serving.
    let (writer, wconn) = b.client();
    f.as_process(writer, || {
        let c = Connection::new(Endpoint::from_handle(wconn));
        let mut lend = Lend::new(4).unwrap();
        c.attach(&mut lend, 1, "").unwrap();
        c.open(&mut lend, 1, mode::OWRITE).unwrap();
        assert_eq!(c.write(&mut lend, 1, 0, b"k").unwrap(), 1);
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
        let c = Connection::new(Endpoint::from_handle(conn));
        let mut lend = Lend::new(4).unwrap();
        c.attach(&mut lend, 0, "").unwrap();
        c.open(&mut lend, 0, mode::ORDWR).unwrap();
        for (i, byte) in b"hello, world\n".iter().enumerate() {
            assert_eq!(c.write(&mut lend, 0, i as u64, &[*byte]).unwrap(), 1);
            assert_eq!(b.printed(), *byte, "byte {i}");
        }
    });
    assert_eq!(b.shut_down(), redoubt_rt::exit::OK);
}

/// Every write goes out inside the console's hold, a [`HOLD_CHUNK`] at a time, so the kernel's
/// lines land between chunks and never inside one (kernel/devices.md, "The console's one
/// writer"); and a hold the kernel refuses is a write as before, never a lost one.
#[test]
fn writes_go_out_inside_the_console_s_hold_a_chunk_at_a_time() {
    let b = boot();
    let f = fake();
    let (writer, conn) = b.client();
    let holds = || f.calls(b.server).iter().filter(|c| **c == "console_hold").count();
    let long = vec![b'x'; 2 * HOLD_CHUNK + 1];
    f.as_process(writer, || {
        let c = Connection::new(Endpoint::from_handle(conn));
        let mut lend = Lend::new(4).unwrap();
        c.attach(&mut lend, 0, "").unwrap();
        c.open(&mut lend, 0, mode::OWRITE).unwrap();
        let before = holds();
        assert_eq!(c.write(&mut lend, 0, 0, &long).unwrap(), long.len());
        // Three chunks, each taken and given back.
        assert_eq!(holds() - before, 6);
        f.refuse(b.server, "console_hold", redoubt_rt::abi::Error::Busy);
        assert_eq!(c.write(&mut lend, 0, 0, b"!").unwrap(), 1, "a refused hold still writes");
    });
    assert_eq!(b.printed(), b'!');
    assert_eq!(b.shut_down(), redoubt_rt::exit::OK);
}

/// A device that says "data ready" for ever does not hang the server: the drain is bounded by
/// the FIFO's depth, the ring by its own limit, and the console keeps serving.
#[test]
fn a_device_stuck_on_data_ready_does_not_hang_the_server() {
    let b = boot();
    let f = fake();
    // The line never goes quiet after this.
    b.holds(b'x');
    let (client, conn) = b.client();
    f.as_process(client, || {
        let c = Connection::new(Endpoint::from_handle(conn));
        let mut lend = Lend::new(4).unwrap();
        c.attach(&mut lend, 0, "").unwrap();
        c.open(&mut lend, 0, mode::ORDWR).unwrap();
        // Reads are answered (with the byte the device keeps handing over) rather than hanging.
        for _ in 0..8 {
            let mut got = [0u8; 8];
            let n = c.read(&mut lend, 0, 0, &mut got).unwrap();
            assert!(n > 0 && got[..n].iter().all(|b| *b == b'x'));
        }
        // And writes still get through.
        assert_eq!(c.write(&mut lend, 0, 0, b"#").unwrap(), 1);
    });
    assert_eq!(b.printed(), b'#');
    assert_eq!(b.shut_down(), redoubt_rt::exit::OK);
}

/// A flood of input with nobody reading keeps what was typed first: the ring takes
/// [`MAX_INPUT`] bytes and drops the rest, rather than overwriting the oldest, so a reader gets
/// the start of what was typed, in order, and the console carries on once it has been read.
///
/// This device hands over whatever its receive register holds each time the driver looks, so
/// how many copies of a byte the server takes is not the test's to choose; only their order is.
/// What the test relies on is that the server drains the UART after answering one call and
/// before receiving the next: holding each byte on the line across two answered calls puts at
/// least one whole drain of it ([`FIFO`] bytes) between them, so `MAX_INPUT / FIFO` bytes (64)
/// are enough to fill the ring, and one more has nowhere to go. The test raises no interrupt,
/// whose wake-ups would only add drains it does not control. The count of dropped bytes
/// (`Console::dropped`) is inside the server and not visible here; what shows the drop is that
/// the last byte never reaches the reader.
#[test]
fn a_flood_of_input_keeps_what_was_typed_first() {
    let b = boot();
    let f = fake();
    let (client, conn) = b.client();
    f.as_process(client, || {
        let c = Connection::new(Endpoint::from_handle(conn));
        let mut lend = Lend::new(4).unwrap();
        c.attach(&mut lend, 0, "").unwrap();
        let typed: Vec<u8> = (0..=(MAX_INPUT / FIFO) as u8).map(|i| b'0' + i).collect();
        for byte in &typed {
            b.holds(*byte);
            // Two answered calls, with the server's drain between them. Refused walks, not
            // writes: a write would put its byte in the same register.
            for _ in 0..8 {
                assert_eq!(
                    c.walk(&mut lend, 0, 1, "anything").unwrap_err(),
                    ClientError::Rerror(ErrorName::NotDir)
                );
            }
        }
        b.line_quiet();

        // Room for one byte more than the ring may hold, and exactly the ring's limit comes back.
        c.open(&mut lend, 0, mode::OREAD).unwrap();
        let mut got = vec![0u8; MAX_INPUT + 1];
        assert_eq!(c.read(&mut lend, 0, 0, &mut got).unwrap(), MAX_INPUT);
        got.truncate(MAX_INPUT);
        // A run of each byte in the order it was typed, from the very first: nothing skipped or
        // overwritten.
        got.dedup();
        assert!(typed.starts_with(&got), "{got:?}");
        // And the last, typed with the ring already full, was dropped.
        assert!(got.len() < typed.len());

        // Once read, the ring has room again.
        b.holds(b'!');
        let mut next = [0u8; 1];
        assert_eq!(c.read(&mut lend, 0, 0, &mut next).unwrap(), 1);
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
        let c = Connection::new(Endpoint::from_handle(conn));
        let mut lend = Lend::new(4).unwrap();
        c.attach(&mut lend, 0, "").unwrap();
        for bad in [mode::OEXEC, mode::OREAD | mode::OTRUNC, mode::OWRITE | mode::OTRUNC] {
            assert_eq!(
                c.open(&mut lend, 0, bad).unwrap_err(),
                ClientError::Rerror(ErrorName::Protocol),
                "{bad:#x}"
            );
        }
        // There is nothing below /dev/cons.
        assert_eq!(c.walk(&mut lend, 0, 1, "anything").unwrap_err(), ClientError::Rerror(ErrorName::NotDir));
        // A read of an unopened fid, and a write to one opened for reading.
        let mut got = [0u8; 4];
        assert_eq!(c.read(&mut lend, 0, 0, &mut got).unwrap_err(), ClientError::Rerror(ErrorName::Protocol));
        c.open(&mut lend, 0, mode::OREAD).unwrap();
        assert_eq!(c.write(&mut lend, 0, 0, b"x").unwrap_err(), ClientError::Rerror(ErrorName::Protocol));
    });
    assert_eq!(b.shut_down(), redoubt_rt::exit::OK);
}

/// `init` mints every server's console through its one root badge here, and starts at most
/// `MAX_THREADS - 1` servers: that badge's one bucket holds them all, and the next is refused.
#[test]
fn init_s_badge_mints_a_console_for_every_server_init_can_start() {
    let b = boot();
    let f = fake();
    // `init` runs in account 0, where a badge has a bucket of its own and no shares.
    let init = f.process(0, &[]);
    let conn = f.grant(b.server, b.receive, init, 1);
    f.as_process(init, || {
        let c = Connection::new(Endpoint::from_handle(conn));
        let mut lend = Lend::new(4).unwrap();
        for i in 0..MAX_THREADS - 1 {
            c.new_connection(&mut lend, "", 0).unwrap_or_else(|e| panic!("connection {i}: {e:?}"));
        }
        // One more still fits the bucket; past it, the bucket is full.
        c.new_connection(&mut lend, "", 0).unwrap();
        assert_eq!(c.new_connection(&mut lend, "", 0).unwrap_err(), ClientError::Remote);
    });
    assert_eq!(b.shut_down(), redoubt_rt::exit::OK);
}

/// The fids of those consoles are charged to the same bucket: `init`'s own console and one for
/// each of the `MAX_THREADS - 1` servers it can start each attach and open `cons`, as a program's
/// console does, and a fid beyond them is refused.
#[test]
fn every_console_init_mints_attaches_and_opens_in_its_one_bucket() {
    let b = boot();
    let f = fake();
    let init = f.process(0, &[]);
    let conn = f.grant(b.server, b.receive, init, 1);
    f.as_process(init, || {
        let mut lend = Lend::new(4).unwrap();
        let own = Connection::new(Endpoint::from_handle(conn));
        let mut consoles = vec![Connection::new(Endpoint::from_handle(conn))];
        for i in 0..MAX_THREADS - 1 {
            let (minted, _) =
                own.new_connection(&mut lend, "", 0).unwrap_or_else(|e| panic!("console {i}: {e:?}"));
            consoles.push(Connection::new(minted));
        }
        // The root, and `cons` walked from it and opened (redoubt_client's console).
        for (i, c) in consoles.iter().enumerate() {
            c.attach(&mut lend, 0, "").unwrap_or_else(|e| panic!("console {i} attaches: {e:?}"));
            c.walk(&mut lend, 0, 1, "").unwrap_or_else(|e| panic!("console {i} walks: {e:?}"));
            c.open(&mut lend, 1, mode::ORDWR).unwrap_or_else(|e| panic!("console {i} opens: {e:?}"));
        }
        assert_eq!(
            consoles[0].walk(&mut lend, 0, 2, "").unwrap_err(),
            ClientError::Rerror(ErrorName::TooMany)
        );
    });
    assert_eq!(b.shut_down(), redoubt_rt::exit::OK);
}

/// Beyond 9P, `ninep_common` and `consol`'s `size` and `resize`, `consoled` refuses every typed
/// opcode, `ended` as a call among them, and the handles such a request carries are closed with
/// the refusal: a client repeating them cannot grow the server's handle table
/// (servers/serving.md, "Authority"). So are a `consol` call's, which takes none.
#[test]
fn a_refused_typed_request_leaves_no_handle_behind() {
    let b = boot();
    let f = fake();
    let (client, conn) = b.client();
    let server = Endpoint::from_handle(conn);
    // `ended` (18) as a call, an opcode past `consol`'s, and `size` (16) carrying handles: each is
    // answered malformed (status 1).
    let refused = |carried: &[Handle]| {
        for opcode in [16, 18, 19] {
            if opcode == 16 && carried.is_empty() {
                continue;
            }
            let (reply, _) = server.call(&[opcode, 0, 0, 0], carried, None, FOREVER).into_result().unwrap();
            assert_eq!(reply.words[0], 1, "opcode {opcode}");
        }
    };
    // Counted once the server has answered a call, so its own start-up (the badge it mints for
    // its interrupt thread's wake-ups) is behind it.
    f.as_process(client, || refused(&[]));
    let before = f.held(b.server).0;
    f.as_process(client, || {
        for _ in 0..64 {
            refused(&[Endpoint::create().unwrap().handle(), Endpoint::create().unwrap().handle()]);
        }
    });
    assert_eq!(f.held(b.server).0, before, "every carried handle was closed");
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
    no_irq.handle(consoled::ENDPOINT, receive).handle(consoled::UART_MMIO, mmio).arg("buckets=4");
    let thread = launch(server, no_irq.finish().unwrap(), consoled::serve);
    assert_eq!(thread.join().unwrap(), consoled::NO_IRQ);

    // A console its block does not size, or sizes at nothing, or for more than its budget holds:
    // it does not guess a count (servers/serving.md R26). Nor a window size it cannot read: a
    // side of nothing or past 1,024, one side alone, or two sizes.
    let bad_sizes: [&[&str]; 5] = [
        &["buckets=4", "size=0,24"],
        &["buckets=4", "size=80,1025"],
        &["buckets=4", "size=80"],
        &["buckets=4", "size=80,24,1"],
        &["buckets=4", "size=80,24", "size=80,24"],
    ];
    let bad_buckets: [&[&str]; 4] = [&[], &["buckets=0"], &["buckets=4", "buckets=4"], &["buckets=32"]];
    for sizing in bad_buckets.into_iter().chain(bad_sizes) {
        let unsized_ = f.process(0, &[]);
        let receive = f.endpoint(unsized_);
        let (mmio, irq) = f.device(unsized_, REGISTERS);
        let mut builder = StartupBuilder::new(receive.index().max(mmio.index()).max(irq.index()));
        builder
            .handle(consoled::ENDPOINT, receive)
            .handle(consoled::UART_MMIO, mmio)
            .handle(consoled::UART_IRQ, irq);
        for arg in sizing {
            builder.arg(arg);
        }
        let thread = launch(unsized_, builder.finish().unwrap(), consoled::serve);
        assert_eq!(thread.join().unwrap(), consoled::BAD_LIMITS, "{sizing:?}");
    }

    // A mapping too short to be a 16550 is not one.
    let short = f.process(0, &[]);
    let short_receive = f.endpoint(short);
    let (short_mmio, short_irq) = f.device(short, 4);
    let thread = launch(short, block(short_receive, short_mmio, short_irq, &[]), consoled::serve);
    assert_eq!(thread.join().unwrap(), consoled::NO_UART);
    let _ = irq;
}

/// `consol` on the physical console: `size` is the manifest's `size=COLS,ROWS`; `resize` waits,
/// since a UART has no window, until its caller gives up, which frees it; and it is a parked call
/// in its caller's share, so a second one beside it is refused at once rather than held
/// (servers/consoled.md, "The `consol` protocol").
#[test]
fn consol_size_is_the_argument_and_a_resize_waits_until_its_caller_gives_up() {
    let size = |conn: Handle| {
        let words = consol::Message::Size(consol::Size {}).encode(&mut []).unwrap();
        let (reply, _) = Endpoint::from_handle(conn).call(&words, &[], None, FOREVER).into_result().unwrap();
        match consol::Reply::decode(16, &reply.words, &[], 0) {
            Ok(Ok(consol::Reply::Size(r))) => (r.cols, r.rows),
            other => panic!("not a size: {other:?}"),
        }
    };
    let resize = |conn: Handle, timeout: u64| {
        let words = consol::Message::Resize(consol::Resize {}).encode(&mut []).unwrap();
        Endpoint::from_handle(conn).call(&words, &[], None, timeout).into_result().map(|(r, _)| r.words[0])
    };

    let f = fake();
    let b = boot_with(&["size=132,43"]);
    let (client, conn) = b.client();
    f.as_process(client, || assert_eq!(size(conn), (132, 43)));
    // It waits: the wait is the client's own timeout, and giving up frees the call (R3).
    f.as_process(client, || assert_eq!(resize(conn, 150_000), Err(redoubt_rt::abi::Error::Timeout)));
    wait_until("the abandoned resize to be freed", || f.open_calls(b.server) == 0);
    // Parked again, it holds its caller's one parked call: a second is refused at once.
    let waiter = f.run(client, move || u32::from(resize(conn, FOREVER) != Ok(0)));
    wait_until("the server to hold the resize", || f.open_calls(b.server) == 1);
    f.as_process(client, || assert_eq!(resize(conn, FOREVER), Ok(1), "over the caller's share"));
    // The console still serves while it waits.
    f.as_process(client, || assert_eq!(size(conn), (132, 43)));
    assert_eq!(f.open_calls(b.server), 1);
    assert_eq!(b.shut_down(), redoubt_rt::exit::OK);
    // The server is gone, and the waiting call with it: refused or dead, never answered a size.
    assert_eq!(waiter.join().unwrap(), 1);
}

/// A console the manifest does not size does not guess one: a UART cannot know its far end's
/// size, so `size` and `resize` are both refused as malformed, at once, and nothing is held; the
/// caller's console is then of unknown size (servers/consoled.md, "The `consol` protocol").
#[test]
fn a_console_with_no_size_refuses_consol() {
    let call = |conn: Handle, message: consol::Message| {
        let words = message.encode(&mut []).unwrap();
        let (reply, _) = Endpoint::from_handle(conn).call(&words, &[], None, FOREVER).into_result().unwrap();
        reply.words[0]
    };

    let b = boot();
    let f = fake();
    let (client, conn) = b.client();
    f.as_process(client, || {
        assert_eq!(call(conn, consol::Message::Size(consol::Size {})), 1, "size, malformed");
        assert_eq!(call(conn, consol::Message::Resize(consol::Resize {})), 1, "resize, malformed");
    });
    assert_eq!(f.open_calls(b.server), 0);
    assert_eq!(b.shut_down(), redoubt_rt::exit::OK);
}
