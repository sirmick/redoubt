//! `console` and `ns`, against the real `consoled` on a fake ns16550 and an in-test `consol`
//! server (until `consoled` serves `consol`, in M2), on the fake kernel.

mod common;

use std::time::{Duration, Instant};

use common::Served;
use redoubt_client::console::Console;
use redoubt_client::file::Connection;
use redoubt_client::ns::Namespace;
use redoubt_client::{Error, Lend, Refusal};
use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{Handle, Handles};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::server::ninep::mode;
use redoubt_rt::server::typed::{Outcome, finish};
use redoubt_rt::startup::{Startup, StartupBuilder};
use redoubt_rt::wire::proto::consol::{Message, Reply, ResizeReply, SizeReply};

#[path = "../../../servers/consoled/src/bin/consoled.rs"]
mod consoled;

const RBR_THR: usize = 0;
const IER: usize = 1;
const LSR: usize = 5;
const LSR_DATA_READY: u8 = 0x01;
const LSR_THR_EMPTY: u8 = 0x20;

/// A running `consoled` on a fake UART.
struct Uart {
    server: usize,
    receive: Handle,
    mmio: Handle,
    irq: Handle,
    thread: std::thread::JoinHandle<u32>,
}

impl Uart {
    fn start() -> Uart {
        let f = fake();
        let server = f.process(0, &[]);
        let receive = f.endpoint(server);
        let (mmio, irq) = f.device(server, 8);
        f.registers(server, mmio)[LSR] = LSR_THR_EMPTY;
        let highest = [receive, mmio, irq].iter().map(|h| h.index()).max().unwrap();
        let mut block = StartupBuilder::new(highest);
        block
            .handle(consoled::ENDPOINT, receive)
            .handle(consoled::UART_MMIO, mmio)
            .handle(consoled::UART_IRQ, irq)
            .arg("buckets=4");
        let block = block.finish().unwrap();
        let thread = f.run(server, move || consoled::serve(&Startup::parse(&block).unwrap()));
        let uart = Uart { server, receive, mmio, irq, thread };
        wait_until("the UART is initialised", || uart.regs()[IER] == 0x01);
        uart
    }

    fn regs(&self) -> &'static mut [u8] { fake().registers(self.server, self.mmio) }

    /// A process with `labels`, and the block a launcher writes for it: its connection to
    /// `consoled` bound at `/dev/cons`.
    fn session(&self, labels: &[u64]) -> (usize, Vec<u8>) {
        let f = fake();
        let session = f.process(1001, labels);
        let conn = f.grant(self.server, self.receive, session, 0x20 + session as u64);
        (session, StartupBuilder::new(conn.index()).namespace("/dev/cons", conn).finish().unwrap())
    }

    fn stop(self) {
        let f = fake();
        wait_until("the interrupt thread to wait again", || !f.masked(self.server, self.irq));
        f.destroy(self.server, self.receive);
        assert_eq!(self.thread.join().unwrap(), redoubt_rt::exit::OK);
    }
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::yield_now();
    }
}

/// A session's namespace from its startup block reaches the UART both ways: a write goes out,
/// and a read waits in the server until a key is typed. `consoled` serves no `consol` yet, so
/// `size` is `None`.
#[test]
fn a_session_writes_and_reads_the_console() {
    let uart = Uart::start();
    let (session, block) = uart.session(&[]);
    let reading = fake().run(session, move || {
        let startup = Startup::parse(&block).unwrap();
        let mut lend = Lend::new(1).unwrap();
        let ns = Namespace::from_startup(&startup, &mut lend).unwrap();
        let console = Console::open(&ns, &mut lend).unwrap();
        assert_eq!(console.write(&mut lend, b"!").unwrap(), 1);
        assert_eq!(console.size(&mut lend), Ok(None));
        let mut got = [0u8; 1];
        assert_eq!(console.read(&mut lend, &mut got).unwrap(), 1);
        console.close(&mut lend).unwrap();
        u32::from(got[0])
    });
    wait_until("the write to reach the UART", || uart.regs()[RBR_THR] == b'!');
    wait_until("the read to be parked", || fake().open_calls(uart.server) == 1);
    uart.regs()[RBR_THR] = b'z';
    uart.regs()[LSR] |= LSR_DATA_READY;
    fake().fire(uart.server, uart.irq);
    assert_eq!(reading.join().unwrap(), u32::from(b'z'));
    uart.regs()[LSR] &= !LSR_DATA_READY;
    uart.stop();
}

/// The attack (R69): a labelled session cannot open the console for writing, and the refusal is
/// `consoled`'s own; reading is still its to do.
#[test]
fn a_labelled_session_cannot_write_the_console() {
    let uart = Uart::start();
    let (session, block) = uart.session(&[7]);
    // What `consoled`'s own start left in the transmit register (its divisor latch shares it).
    let before = uart.regs()[RBR_THR];
    fake().as_process(session, || {
        let startup = Startup::parse(&block).unwrap();
        let mut lend = Lend::new(1).unwrap();
        let ns = Namespace::from_startup(&startup, &mut lend).unwrap();
        assert_eq!(Console::open(&ns, &mut lend).err(), Some(Error::Rerror));
        let (conn, rest) = ns.lookup("/dev/cons").unwrap();
        conn.open(&mut lend, rest, mode::OREAD).unwrap().close(&mut lend).unwrap();
    });
    assert_eq!(uart.regs()[RBR_THR], before, "nothing reached the UART");
    uart.stop();
}

/// `size` and `resize` through `consol`, from a server that serves it.
#[test]
fn size_and_resize_come_from_the_server() {
    let served = Served::start(|_, mut request| {
        let words = request.words;
        let reply = match Message::decode(&words, &[], request.handles.as_slice().len()) {
            Ok(Message::Size(_)) => Reply::Size(SizeReply { cols: 80, rows: 24 }),
            Ok(Message::Resize(_)) => Reply::Resize(ResizeReply { cols: 132, rows: 43 }),
            Err(_) => {
                return finish(
                    request,
                    &Outcome { words: [1, 0, 0, 0], send: Handles::new(), close: Handles::new() },
                )
                .map(|_| ());
            }
        };
        let words = reply.encode(request.lend()).unwrap();
        finish(request, &Outcome { words, send: Handles::new(), close: Handles::new() }).map(|_| ())
    });
    let (client, conn) = served.client(1001, 1);
    fake().as_process(client, || {
        let mut lend = Lend::new(1).unwrap();
        let mut ns = Namespace::new();
        ns.bind("/dev", Connection::attach(Endpoint::from_handle(conn), &mut lend).unwrap()).unwrap();
        let console = Console::open(&ns, &mut lend).unwrap();
        assert_eq!(console.size(&mut lend), Ok(Some((80, 24))));
        assert_eq!(console.resize(&mut lend), Ok((132, 43)));
        console.close(&mut lend).unwrap();
    });
    served.stop();
}

/// The namespace table: longest prefix wins, a bind replaces what was at its prefix and shares
/// the connection, a handle bound twice in a block is one connection, and a path that is not
/// clean and absolute binds nothing.
#[test]
fn the_namespace_resolves_by_longest_prefix() {
    let served = Served::start(|_, request| {
        drop(request);
        Ok(())
    });
    let (client, conn) = served.client(1001, 1);
    let block =
        StartupBuilder::new(conn.index()).namespace("/", conn).namespace("/home", conn).finish().unwrap();
    fake().as_process(client, || {
        let mut lend = Lend::new(1).unwrap();
        let before = fake().calls(client).len();
        let mut ns = Namespace::from_startup(&Startup::parse(&block).unwrap(), &mut lend).unwrap();
        assert_eq!(fake().calls(client).len(), before + 2, "one Tversion and one Tattach for both paths");
        let (root, rest) = ns.lookup("/cons").unwrap();
        let root = root.clone();
        assert_eq!(rest, "cons");
        let (home, rest) = ns.lookup("/home/a/note").unwrap();
        assert_eq!(rest, "a/note");
        assert!(home.same(&root));
        assert!(ns.lookup("relative").is_none());

        let a = ns.lookup("/home/a").unwrap().0.clone();
        ns.bind("/home", a).unwrap();
        assert_eq!(ns.list().map(|(p, _)| p).collect::<Vec<_>>(), ["/", "/home"]);
        for bad in ["home", "/home/", "/a/../b", ""] {
            assert_eq!(ns.bind(bad, root.clone()), Err(Error::Refused(Refusal::BadPath)), "{bad:?}");
        }
    });
    served.stop();
}
