//! `ipd`, the whole program (`src/bin/ipd.rs`), as a fake process on the rt fake kernel: its real
//! loop, parking, `grant` through the kernel and the labelled refusal, against a fake `netd` (two
//! threads: one answering `info` and `transmit`, one carrying frames between a smoltcp peer and
//! `ipd`'s ingress badge) and clients calling across "address spaces".
//!
//! The loop checks at every poll that no call is current (a `debug_assert!`, on in these tests),
//! so a poll with a current call fails the test by panicking `ipd`'s thread.

#[path = "../../../libs/rt/tests/common/mod.rs"]
mod kernel;

#[allow(dead_code)]
#[path = "../src/bin/ipd.rs"]
mod ipd_bin;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kernel::fake;
use redoubt_ipd::scope::{Ports, Prefix, Rule, Scope};
use redoubt_rt::abi::{FOREVER, Handle};
use redoubt_rt::client::{Client, ClientError};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Buffer, Event};
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::{Startup, StartupBuilder};
use redoubt_rt::wire::proto::{ipd, net_ctl, netif};
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{self, Device, DeviceCapabilities, Medium};
use smoltcp::socket::tcp;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, Ipv4Address};

const MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
const PEER_MAC: [u8; 6] = [0x52, 0x55, 0x0a, 0x00, 0x02, 0x02];
const INGRESS: u64 = 3;
const ANY: u64 = 4;
const NARROW: u64 = 6;
const NETD_CLIENT: u64 = 7;
const PEER_ADDR: [u8; 4] = [10, 1, 9, 100];

/// The peer's device: frames from `ipd` in, frames for it out.
struct Dev {
    rx: VecDeque<Vec<u8>>,
    tx: VecDeque<Vec<u8>>,
}

struct Rx(Vec<u8>);
struct Tx<'a>(&'a mut VecDeque<Vec<u8>>);

impl phy::RxToken for Rx {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R { f(&self.0) }
}

impl phy::TxToken for Tx<'_> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let mut buf = vec![0; len];
        let r = f(&mut buf);
        self.0.push_back(buf);
        r
    }
}

impl Device for Dev {
    type RxToken<'a> = Rx;
    type TxToken<'a> = Tx<'a>;

    fn receive(&mut self, _: smoltcp::time::Instant) -> Option<(Rx, Tx<'_>)> {
        let f = self.rx.pop_front()?;
        Some((Rx(f), Tx(&mut self.tx)))
    }

    fn transmit(&mut self, _: smoltcp::time::Instant) -> Option<Tx<'_>> { Some(Tx(&mut self.tx)) }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = 1514;
        caps
    }
}

/// Every other host, answering for any address, with sockets that echo.
struct Peer {
    iface: Interface,
    dev: Dev,
    sockets: SocketSet<'static>,
    echo: Vec<SocketHandle>,
}

fn now_us(boot: Instant) -> smoltcp::time::Instant {
    smoltcp::time::Instant::from_micros(boot.elapsed().as_micros() as i64)
}

impl Peer {
    fn new(boot: Instant) -> Peer {
        let mut dev = Dev { rx: VecDeque::new(), tx: VecDeque::new() };
        let mut config = Config::new(HardwareAddress::Ethernet(EthernetAddress(PEER_MAC)));
        config.random_seed = 99;
        let mut iface = Interface::new(config, &mut dev, now_us(boot));
        iface.update_ip_addrs(|a| {
            a.push(IpCidr::new(IpAddress::Ipv4(Ipv4Address::new(10, 1, 0, 1)), 16)).unwrap()
        });
        iface.set_any_ip(true);
        Peer { iface, dev, sockets: SocketSet::new(vec![]), echo: Vec::new() }
    }

    fn echo_on(&mut self, port: u16) {
        let mut s =
            tcp::Socket::new(tcp::SocketBuffer::new(vec![0; 8192]), tcp::SocketBuffer::new(vec![0; 8192]));
        s.listen(port).unwrap();
        let h = self.sockets.add(s);
        self.echo.push(h);
    }

    fn turn(&mut self, boot: Instant) {
        self.iface.poll(now_us(boot), &mut self.dev, &mut self.sockets);
        for h in self.echo.clone() {
            let s = self.sockets.get_mut::<tcp::Socket>(h);
            let mut buf = [0u8; 2048];
            if s.can_recv() && s.can_send() {
                let n = s.recv_slice(&mut buf).unwrap();
                s.send_slice(&buf[..n]).unwrap();
            }
        }
        self.iface.poll(now_us(boot), &mut self.dev, &mut self.sockets);
    }
}

/// A booted `ipd`, its fake `netd` and the peer.
struct Net {
    ipd: usize,
    ep: Handle,
    netd: usize,
    netd_ep: Handle,
    _ingress: Handle,
    thread: std::thread::JoinHandle<u32>,
    stop: Arc<AtomicBool>,
    peer: Arc<Mutex<Peer>>,
    wire: std::thread::JoinHandle<u32>,
    netd_thread: std::thread::JoinHandle<u32>,
}

/// `netd`'s serving side: `info` and `transmit`, each frame handed to the wire.
fn serve_netd(ep: Endpoint, out: Sender<Vec<u8>>) -> u32 {
    loop {
        match ep.receive(FOREVER, 0) {
            Ok(Event::Call(mut request)) => {
                assert_eq!(request.caller.badge, NETD_CLIENT);
                let words = request.words;
                let lend = request.lend();
                let reply = match netif::Message::decode(&words, lend, 0) {
                    Ok(netif::Message::Info(_)) => {
                        let mac = MAC.iter().rev().fold(0u64, |m, o| (m << 8) | u64::from(*o));
                        netif::Reply::Info(netif::InfoReply { mac, mtu: 1500 }).encode(&mut []).unwrap()
                    }
                    Ok(netif::Message::Transmit(t)) => {
                        let _ = out.send(t.frame.to_vec());
                        netif::Reply::Transmit(netif::TransmitReply {}).encode(&mut []).unwrap()
                    }
                    Err(_) => redoubt_rt::server::MALFORMED,
                };
                let _ = request.reply(&reply, &[]);
            }
            Ok(_) => {}
            Err(_) => return 0,
        }
    }
}

/// Sends one frame to `ipd` as `netd` does: a `send` of `frame`, one page transferred.
fn send_frame(ingress: &Endpoint, frame: &[u8]) {
    let mut page = Buffer::new(1).unwrap();
    let words = ipd::Message::Frame(ipd::Frame { frame }).encode(&mut page).unwrap();
    let _ = ingress.send(&words, &[], Some(page), 1_000_000);
}

/// The wire: frames `ipd` transmitted go to the peer, the peer's go to `ipd`.
fn carry(
    ingress: Endpoint,
    from_ipd: Receiver<Vec<u8>>,
    peer: Arc<Mutex<Peer>>,
    stop: Arc<AtomicBool>,
) -> u32 {
    let boot = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if let Ok(frame) = from_ipd.recv_timeout(Duration::from_millis(2)) {
            peer.lock().unwrap().dev.rx.push_back(frame);
        }
        let out: Vec<Vec<u8>> = {
            let mut p = peer.lock().unwrap();
            p.turn(boot);
            p.dev.tx.drain(..).collect()
        };
        for f in out {
            send_frame(&ingress, &f);
        }
    }
    0
}

fn args(buckets: u32) -> Vec<String> {
    vec![
        "addr=10.1.0.2/16".into(),
        "gateway=10.1.0.1".into(),
        "self=10.1.9.102/32".into(),
        format!("ingress={INGRESS}"),
        format!("scope={ANY}:c:0.0.0.0/0:1-65535,l:1-65535"),
        format!("scope={NARROW}:c:10.1.9.110/32:7"),
        format!("buckets={buckets}"),
    ]
}

fn boot(buckets: u32) -> Net {
    let f = fake();
    let ipd = f.process(0, &[]);
    let ep = f.endpoint(ipd);
    let netd = f.process(0, &[]);
    let netd_ep = f.endpoint(netd);
    let netd_for_ipd = f.grant(netd, netd_ep, ipd, NETD_CLIENT);
    let ingress = f.grant(ipd, ep, netd, INGRESS);
    let mut b = StartupBuilder::new(ep.index().max(netd_for_ipd.index()));
    b.handle(ipd_bin::ENDPOINT, ep).handle(ipd_bin::NETD, netd_for_ipd);
    for a in args(buckets) {
        b.arg(&a);
    }
    let block = b.finish().unwrap();
    let thread = f.run(ipd, move || ipd_bin::serve(&Startup::parse(&block).unwrap()));
    let (tx, rx) = channel();
    let netd_thread = f.run(netd, move || serve_netd(Endpoint::from_handle(netd_ep), tx));
    let stop = Arc::new(AtomicBool::new(false));
    let peer = Arc::new(Mutex::new(Peer::new(Instant::now())));
    let (p, s) = (peer.clone(), stop.clone());
    let wire = f.run(netd, move || carry(Endpoint::from_handle(ingress), rx, p, s));
    Net { ipd, ep, netd, netd_ep, _ingress: ingress, thread, stop, peer, wire, netd_thread }
}

impl Net {
    /// A client process (`account`, `labels`) holding a connection with `badge`.
    fn client(&self, account: u64, labels: &[u64], badge: u64) -> (usize, Handle) {
        let f = fake();
        let pid = f.process(account, labels);
        (pid, f.grant(self.ipd, self.ep, pid, badge))
    }

    fn shut_down(self) -> u32 {
        let f = fake();
        self.stop.store(true, Ordering::Relaxed);
        self.wire.join().unwrap();
        f.destroy(self.ipd, self.ep);
        let code = self.thread.join().expect("ipd panicked");
        f.destroy(self.netd, self.netd_ep);
        self.netd_thread.join().unwrap();
        code
    }
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if done() {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("timed out waiting for {what}");
}

fn op(message: net_ctl::Message<'_>) -> Vec<u8> {
    let mut out = vec![0u8; 64];
    let n = message.encode_file(&mut out).unwrap();
    out.truncate(n);
    out
}

/// Attaches, makes a socket and opens its ctl (fid 2) and data (fid 3); returns its number.
fn socket(c: &mut Client) -> u32 {
    c.attach(0, "").unwrap();
    c.walk(0, 1, "tcp/clone").unwrap();
    c.open(1, mode::OREAD).unwrap();
    let mut n = [0u8; 4];
    assert_eq!(c.read(1, 0, &mut n).unwrap(), 4);
    c.clunk(1).unwrap();
    let n = u32::from_le_bytes(n);
    c.walk(0, 2, &format!("tcp/{n}/ctl")).unwrap();
    c.open(2, mode::ORDWR).unwrap();
    c.walk(0, 3, &format!("tcp/{n}/data")).unwrap();
    c.open(3, mode::ORDWR).unwrap();
    n
}

fn connect(c: &mut Client, addr: [u8; 4], port: u16) -> Result<(), ClientError> {
    let bytes = op(net_ctl::Message::Connect(net_ctl::Connect { addr: &addr, port }));
    c.write(2, 0, &bytes).map(|_| ())
}

/// The real program: connect (the ctl read waits for the handshake), bytes out and back (the data
/// read waits for the echo), and nothing left open.
#[test]
fn a_client_connects_and_echoes_through_the_program() {
    let net = boot(6);
    net.peer.lock().unwrap().echo_on(7);
    let f = fake();
    let (pid, conn) = net.client(1, &[], ANY);
    f.as_process(pid, || {
        let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
        socket(&mut c);
        connect(&mut c, PEER_ADDR, 7).unwrap();
        let mut status = [0u8; 8];
        assert_eq!(c.read(2, 0, &mut status).unwrap(), 8);
        assert_eq!(u32::from_le_bytes(status[..4].try_into().unwrap()), 2, "established");
        assert_eq!(c.write(3, 0, b"round trip").unwrap(), 10);
        let mut got = [0u8; 64];
        let n = c.read(3, 0, &mut got).unwrap();
        assert_eq!(&got[..n], b"round trip");
        c.write(2, 0, &op(net_ctl::Message::Close(net_ctl::Close {}))).unwrap();
    });
    wait_until("ipd to hold no call", || f.open_calls(net.ipd) == 0);
    assert_eq!(net.shut_down(), redoubt_rt::exit::OK);
}

/// A labelled caller gets nothing, on every path: attach (so no walk, no clone), `grant`,
/// `new_connection`, a typed call of any opcode, and a frame on the ingress badge. And it holds
/// no bucket: with two buckets, two unlabelled accounts still get in after it, and a third does
/// not.
#[test]
fn a_labelled_caller_gets_nothing_and_holds_no_bucket() {
    let net = boot(2);
    net.peer.lock().unwrap().echo_on(7);
    let f = fake();
    let (vault, conn) = net.client(1, &[7], ANY);
    f.as_process(vault, || {
        let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
        assert!(c.attach(0, "").is_err(), "a labelled attach");
        assert!(c.new_connection("", 0).is_err(), "a labelled new_connection");
        let ep = c.into_endpoint();
        let scope = Scope::new(&[Rule::Connect(Prefix::new(0, 0).unwrap(), Ports::new(1, 65535).unwrap())])
            .unwrap()
            .encode();
        let mut page = Buffer::new(1).unwrap();
        let words = ipd::Message::Grant(ipd::Grant { scope: &scope }).encode(&mut page).unwrap();
        let reply = ep.call(&words, &[], Some(page), 1_000_000).into_result().unwrap().0;
        assert_eq!(ipd::Reply::decode(16, &reply.words, &[], 0), Ok(Err(ipd::ErrorCode::NotPermitted)));
        let reply = ep.call(&[99, 0, 0, 0], &[], None, 1_000_000).into_result().unwrap().0;
        assert_eq!(reply.words[0], 2, "any typed opcode: not_permitted");
    });
    // A labelled process holding the ingress badge: its frames are dropped.
    let (labelled_netd, ingress) = net.client(0, &[7], INGRESS);
    f.as_process(labelled_netd, || send_frame(&Endpoint::from_handle(ingress), &[0xff; 60]));
    // Two buckets, both still free.
    for account in [11, 12] {
        let (pid, conn) = net.client(account, &[], ANY);
        f.as_process(pid, || {
            let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
            c.attach(0, "").unwrap();
        });
    }
    let (pid, conn) = net.client(13, &[], ANY);
    f.as_process(pid, || {
        let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
        assert!(c.attach(0, "").is_err(), "a third bucket with two");
    });
    assert_eq!(net.shut_down(), redoubt_rt::exit::OK);
}

/// `grant` through the kernel: a handle to a connection whose scope is the one asked for, which
/// reaches exactly that; a wider one is refused.
#[test]
fn grant_mints_a_narrower_connection() {
    let net = boot(6);
    net.peer.lock().unwrap().echo_on(7);
    let f = fake();
    let (steward, conn) = net.client(0, &[], ANY);
    let granted = f.as_process(steward, || {
        let ep = Endpoint::from_handle(conn);
        let narrow = Scope::new(&[Rule::Connect(
            Prefix::new(u32::from_be_bytes(PEER_ADDR), 32).unwrap(),
            Ports::new(7, 7).unwrap(),
        )])
        .unwrap();
        let bytes = narrow.encode();
        let mut page = Buffer::new(1).unwrap();
        let words = ipd::Message::Grant(ipd::Grant { scope: &bytes }).encode(&mut page).unwrap();
        let (reply, page) = ep.call(&words, &[], Some(page), 1_000_000).into_result().unwrap();
        let Ok(Ok(ipd::Reply::Grant(_))) = ipd::Reply::decode(16, &reply.words, &page.unwrap(), 1) else {
            panic!("no grant: {:?}", reply.words)
        };
        reply.handles.as_slice()[0].unwrap()
    });
    // The steward passes the handle on (as process_start would), to a principal's process.
    let principal = f.process(1, &[]);
    let handle = f.copy(steward, granted, principal);
    f.as_process(principal, || {
        let mut c = Client::new(Endpoint::from_handle(handle), 4).unwrap();
        socket(&mut c);
        assert!(connect(&mut c, [10, 1, 9, 101], 7).is_err(), "outside the grant");
        assert!(connect(&mut c, PEER_ADDR, 8).is_err(), "another port");
        connect(&mut c, PEER_ADDR, 7).unwrap();
        let mut status = [0u8; 8];
        c.read(2, 0, &mut status).unwrap();
        assert_eq!(status[0], 2);
        // A grant from the grant, wider than it: refused.
        let ep = c.into_endpoint();
        let wide = Scope::new(&[Rule::Connect(Prefix::new(0, 0).unwrap(), Ports::new(1, 65535).unwrap())])
            .unwrap()
            .encode();
        let mut page = Buffer::new(1).unwrap();
        let words = ipd::Message::Grant(ipd::Grant { scope: &wide }).encode(&mut page).unwrap();
        let reply = ep.call(&words, &[], Some(page), 1_000_000).into_result().unwrap().0;
        assert_eq!(ipd::Reply::decode(16, &reply.words, &[], 0), Ok(Err(ipd::ErrorCode::NotPermitted)));
    });
    assert_eq!(net.shut_down(), redoubt_rt::exit::OK);
}

/// A parked read whose caller gives up is answered at once (its open call and admission freed),
/// and parking stops at the bucket's share: a lone badge of an account parks at most half of 5.
#[test]
fn parked_calls_are_freed_when_abandoned_and_capped_by_the_share() {
    let net = boot(6);
    net.peer.lock().unwrap().echo_on(7);
    let f = fake();
    let (pid, conn) = net.client(1, &[], ANY);
    f.as_process(pid, || {
        let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
        socket(&mut c);
        connect(&mut c, PEER_ADDR, 7).unwrap();
        let mut status = [0u8; 8];
        c.read(2, 0, &mut status).unwrap();
        c.timeout = 150_000;
        let mut got = [0u8; 8];
        assert_eq!(c.read(3, 0, &mut got).unwrap_err(), ClientError::Sys(redoubt_rt::abi::Error::Timeout));
    });
    wait_until("the abandoned read to be answered", || f.open_calls(net.ipd) == 0);
    // Three readers of one badge: two park, the third is refused at once.
    let readers: Vec<_> = (0..2)
        .map(|_| {
            f.run(pid, move || {
                let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
                c.timeout = 2_000_000;
                let mut got = [0u8; 8];
                u32::from(c.read(3, 0, &mut got).is_err())
            })
        })
        .collect();
    wait_until("two reads to park", || f.open_calls(net.ipd) == 2);
    f.as_process(pid, || {
        let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
        let mut got = [0u8; 8];
        // Refused at once (an `Rerror`), not held: the client's timeout never runs out.
        c.timeout = 2_000_000;
        let started = Instant::now();
        assert_eq!(c.read(3, 0, &mut got), Err(ClientError::Remote), "a third parked read");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(f.open_calls(net.ipd), 2);
    });
    for r in readers {
        assert_eq!(r.join().unwrap(), 1, "a parked read answered with data");
    }
    wait_until("ipd to hold no call", || f.open_calls(net.ipd) == 0);
    assert_eq!(net.shut_down(), redoubt_rt::exit::OK);
}

/// Two reads parked on one socket and one byte arrives: one read gets it, the other goes on
/// waiting, and `ipd` goes on serving (a round of resumes serves each parked call once, so the
/// read that parks again is not taken up again in the same round).
#[test]
fn two_reads_one_byte_one_answer_and_ipd_goes_on() {
    let net = boot(6);
    net.peer.lock().unwrap().echo_on(7);
    let f = fake();
    let (pid, conn) = net.client(0, &[], ANY);
    f.as_process(pid, || {
        let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
        socket(&mut c);
        connect(&mut c, PEER_ADDR, 7).unwrap();
        let mut status = [0u8; 8];
        c.read(2, 0, &mut status).unwrap();
    });
    let readers: Vec<_> = (0..2)
        .map(|_| {
            f.run(pid, move || {
                let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
                c.timeout = 1_500_000;
                let mut got = [0u8; 8];
                match c.read(3, 0, &mut got) {
                    Ok(n) => n as u32,
                    Err(_) => 100,
                }
            })
        })
        .collect();
    wait_until("both reads to park", || f.open_calls(net.ipd) == 2);
    f.as_process(pid, || {
        let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
        c.timeout = 1_000_000;
        assert_eq!(c.write(3, 0, b"!").unwrap(), 1);
    });
    wait_until("one read to be answered", || f.open_calls(net.ipd) == 1);
    // ipd is not stuck: it answers another call at once.
    f.as_process(pid, || {
        let mut c = Client::new(Endpoint::from_handle(conn), 4).unwrap();
        c.timeout = 500_000;
        c.walk(0, 9, "tcp").unwrap();
    });
    let mut results: Vec<u32> = readers.into_iter().map(|r| r.join().unwrap()).collect();
    results.sort_unstable();
    assert_eq!(results, vec![1, 100], "one read got the byte, the other timed out");
    wait_until("ipd to hold no call", || f.open_calls(net.ipd) == 0);
    assert_eq!(net.shut_down(), redoubt_rt::exit::OK);
}
