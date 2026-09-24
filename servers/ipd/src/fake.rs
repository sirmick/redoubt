//! Host only: a network for `ipd`'s host tests and fuzz targets: `ipd`'s stack on one end of a recording
//! pipe, a second smoltcp stack on the other playing every other host (it answers for any address, so one
//! peer is the gateway and the whole world behind it), and a clock the test moves.
//!
//! Every frame `ipd` sends is kept ([`World::sent`]) and checked as it is sent
//! ([`check_frame`]): no SYN without ACK to one of the box's own addresses, and no ARP request for
//! one of them other than the gateway.

use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use redoubt_rt::abi::Labels;
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::ninep::NineServer;
use redoubt_rt::server::{Admission, AdmitKey, Limits, Override};
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{self, Device, DeviceCapabilities, Medium};
use smoltcp::socket::tcp;
use smoltcp::time::Instant;
use smoltcp::wire::{
    ArpOperation, ArpPacket, EthernetAddress, EthernetFrame, EthernetProtocol, HardwareAddress, IpAddress,
    IpCidr, IpProtocol, Ipv4Address, Ipv4Packet, TcpPacket,
};

use crate::fs::{NetFs, SocketCaps};
use crate::link::{Link, LinkFault, Netif};
use crate::scope::{Ports, Prefix, Rule, Scope, SelfSet, ip};
use crate::stack::{Entropy, Net, Owner, Ready, Stack, Status, Use};

/// `ipd`'s address, network and gateway in these tests.
pub const ADDR: u32 = ip(10, 1, 0, 2);
pub const LEN: u8 = 16;
pub const GATEWAY: u32 = ip(10, 1, 0, 1);
/// A host on the LAN, and one behind the gateway.
pub const LAN_HOST: u32 = ip(10, 1, 9, 100);
pub const FAR_HOST: u32 = ip(203, 0, 113, 5);
/// More of the box's own addresses (`self=`): a forwarded address, as on QEMU.
pub const SELF_EXTRA: u32 = ip(10, 1, 9, 102);
pub const IPD_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
pub const PEER_MAC: [u8; 6] = [0x52, 0x55, 0x0a, 0x00, 0x02, 0x02];

/// Frames on the wire from `ipd`, and whether its link is broken.
#[derive(Default)]
pub struct Wire {
    pub to_peer: VecDeque<Vec<u8>>,
    pub sent: Vec<Vec<u8>>,
    /// What each transmit answers, when set; `Ok` otherwise.
    pub fault: Option<LinkFault>,
    pub selfset: Option<SelfSet>,
    /// TCP to these ports is sent but never arrives: a sender that has gone.
    pub blackhole: Vec<u16>,
}

/// `ipd`'s end of the pipe.
pub struct Pipe(pub Rc<RefCell<Wire>>);

impl Netif for Pipe {
    fn transmit(&mut self, frame: &[u8]) -> Result<(), LinkFault> {
        let mut wire = self.0.borrow_mut();
        if let Some(fault) = wire.fault {
            return Err(fault);
        }
        if let Some(selfset) = wire.selfset.as_ref() {
            check_frame(frame, selfset);
        }
        wire.sent.push(frame.to_vec());
        if !tcp_dst(frame).is_some_and(|port| wire.blackhole.contains(&port)) {
            wire.to_peer.push_back(frame.to_vec());
        }
        Ok(())
    }
}

/// The destination port of a TCP segment.
pub fn tcp_dst(frame: &[u8]) -> Option<u16> {
    let eth = EthernetFrame::new_checked(frame).ok()?;
    let ipp = Ipv4Packet::new_checked(eth.payload()).ok()?;
    let t = TcpPacket::new_checked(ipp.payload()).ok()?;
    Some(t.dst_port())
}

/// The invariant on everything `ipd` sends (answer 174): no SYN without ACK to one of the box's
/// own addresses, and no ARP request for one of them other than the gateway.
pub fn check_frame(frame: &[u8], selfset: &SelfSet) {
    let eth = EthernetFrame::new_checked(frame).expect("ipd sent a frame that is not Ethernet");
    match eth.ethertype() {
        EthernetProtocol::Arp => {
            let arp = ArpPacket::new_checked(eth.payload()).expect("an ARP packet");
            if arp.operation() == ArpOperation::Request {
                let target = u32::from_be_bytes(arp.target_protocol_addr().try_into().unwrap());
                assert!(
                    target == GATEWAY || !selfset.contains(target),
                    "ipd asked ARP for its own {target:#x}"
                );
            }
        }
        EthernetProtocol::Ipv4 => {
            let ipp = Ipv4Packet::new_checked(eth.payload()).expect("an IPv4 packet");
            assert_eq!(ipp.next_header(), IpProtocol::Tcp, "ipd sent IPv4 that is not TCP");
            let tcpp = TcpPacket::new_checked(ipp.payload()).expect("a TCP segment");
            let dst = u32::from(ipp.dst_addr());
            if tcpp.syn() && !tcpp.ack() {
                assert!(!selfset.contains(dst), "ipd sent a SYN to its own {dst:#x}");
            }
        }
        other => panic!("ipd sent ethertype {other}"),
    }
}

/// The CSPRNG as the tests see it: seeds and ports from a seeded generator, each draw counted,
/// and able to fail.
pub struct Seeds {
    pub state: Rc<Cell<u64>>,
    pub seeds: Rc<RefCell<Vec<u64>>>,
    pub fail: Rc<Cell<bool>>,
}

impl Entropy for Seeds {
    fn draw(&mut self, why: Use) -> Option<u64> {
        if self.fail.get() {
            return None;
        }
        let mut x = self.state.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state.set(x);
        if why == Use::Seed {
            self.seeds.borrow_mut().push(x);
        }
        Some(x)
    }
}

/// The peer's device: frames from `ipd` in, frames for `ipd` out.
pub struct PeerDevice {
    pub rx: VecDeque<Vec<u8>>,
    pub tx: VecDeque<Vec<u8>>,
}

pub struct PeerRx(Vec<u8>);
pub struct PeerTx<'a>(&'a mut VecDeque<Vec<u8>>);

impl phy::RxToken for PeerRx {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R { f(&self.0) }
}

impl phy::TxToken for PeerTx<'_> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let mut buf = vec![0; len];
        let r = f(&mut buf);
        self.0.push_back(buf);
        r
    }
}

impl Device for PeerDevice {
    type RxToken<'a> = PeerRx;
    type TxToken<'a> = PeerTx<'a>;

    fn receive(&mut self, _: Instant) -> Option<(PeerRx, PeerTx<'_>)> {
        let frame = self.rx.pop_front()?;
        Some((PeerRx(frame), PeerTx(&mut self.tx)))
    }

    fn transmit(&mut self, _: Instant) -> Option<PeerTx<'_>> { Some(PeerTx(&mut self.tx)) }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = 1514;
        caps
    }
}

/// Every other host: the gateway, the LAN, the world.
pub struct Peer {
    pub iface: Interface,
    pub dev: PeerDevice,
    pub sockets: SocketSet<'static>,
    /// SYNs without ACK the peer received, by destination (address, port).
    pub syns: Vec<(u32, u16)>,
    /// Sequence numbers of those SYNs.
    pub syn_seqs: Vec<u32>,
}

impl Peer {
    pub fn new(now: u64) -> Peer {
        let mut dev = PeerDevice { rx: VecDeque::new(), tx: VecDeque::new() };
        let mut config = Config::new(HardwareAddress::Ethernet(EthernetAddress(PEER_MAC)));
        config.random_seed = 0x5eed;
        let mut iface = Interface::new(config, &mut dev, Instant::from_micros(now as i64));
        iface.update_ip_addrs(|a| {
            a.push(IpCidr::new(IpAddress::Ipv4(Ipv4Address::from(GATEWAY)), LEN)).unwrap();
        });
        iface.set_any_ip(true);
        Peer { iface, dev, sockets: SocketSet::new(vec![]), syns: Vec::new(), syn_seqs: Vec::new() }
    }

    /// A socket listening on `port` at any address.
    pub fn listen(&mut self, port: u16) -> SocketHandle {
        let mut s =
            tcp::Socket::new(tcp::SocketBuffer::new(vec![0; 16384]), tcp::SocketBuffer::new(vec![0; 16384]));
        s.listen(port).unwrap();
        self.sockets.add(s)
    }

    /// Connects from `from` (an address the peer answers for) to `ipd` on `port`.
    pub fn connect(&mut self, from: u32, from_port: u16, port: u16) -> SocketHandle {
        let mut s =
            tcp::Socket::new(tcp::SocketBuffer::new(vec![0; 16384]), tcp::SocketBuffer::new(vec![0; 16384]));
        s.connect(
            self.iface.context(),
            (IpAddress::Ipv4(Ipv4Address::from(ADDR)), port),
            (IpAddress::Ipv4(Ipv4Address::from(from)), from_port),
        )
        .unwrap();
        self.sockets.add(s)
    }

    pub fn socket(&mut self, h: SocketHandle) -> &mut tcp::Socket<'static> {
        self.sockets.get_mut::<tcp::Socket>(h)
    }

    fn take(&mut self, frame: Vec<u8>) {
        if let Ok(eth) = EthernetFrame::new_checked(&frame[..]) {
            if eth.ethertype() == EthernetProtocol::Ipv4 {
                if let Ok(ipp) = Ipv4Packet::new_checked(eth.payload()) {
                    if let Ok(t) = TcpPacket::new_checked(ipp.payload()) {
                        if t.syn() && !t.ack() {
                            self.syns.push((u32::from(ipp.dst_addr()), t.dst_port()));
                            self.syn_seqs.push(t.seq_number().0 as u32);
                        }
                    }
                }
            }
        }
        self.dev.rx.push_back(frame);
    }

    pub fn syns_to(&self, addr: u32, port: u16) -> usize {
        self.syns.iter().filter(|s| **s == (addr, port)).count()
    }
}

pub fn instant(us: u64) -> Instant { Instant::from_micros(us as i64) }

/// Root badges the tests' `ipd` knows: one that may connect and listen anywhere, one that may
/// connect only to 10.1.9.110 port 7, and one that may listen on 22 and 8000.
pub const ANY: u64 = 4;
pub const NARROW: u64 = 6;
pub const LISTENER: u64 = 22;
/// The badge `netd`'s frames arrive on: no scope, so no `/net`.
pub const INGRESS: u64 = 3;

/// `ipd` (its 9P server over its stack), the peer, and the clock.
pub struct World {
    pub nine: NineServer<NetFs<Pipe, Seeds>>,
    pub wire: Rc<RefCell<Wire>>,
    pub peer: Peer,
    pub now: u64,
    pub seeds: Rc<RefCell<Vec<u64>>>,
    pub fail_random: Rc<Cell<bool>>,
    /// The generator's state: a test that sets it back replays the same draws.
    pub rng: Rc<Cell<u64>>,
}

pub fn selfset() -> SelfSet {
    let extra = [Prefix::new(SELF_EXTRA, 32).unwrap()];
    SelfSet::new(ADDR, LEN, &extra)
}

impl World {
    /// A world whose stack is driven directly, not through 9P: its sockets are not metered (the
    /// admission is not in the loop).
    pub fn unmetered(max_sockets: usize) -> World {
        let mut w = World::new(max_sockets);
        w.nine.fs.stack.meter_off();
        w
    }

    /// One 9P request as `who`, answered in place as the program answers it: the caller's
    /// sockets reserved in the admission around it ([`crate::server::open_sockets`]).
    pub fn answer(&mut self, who: &Caller, buf: &mut [u8]) -> Answer {
        self.nine.fs.now = self.now;
        crate::server::open_sockets(&mut self.nine, who, &redoubt_rt::server::ninep::WORDS_9P);
        let answer = self.nine.answer_in_place(who, buf);
        crate::server::close_sockets(&mut self.nine);
        answer
    }

    /// A world whose link is up, `max_sockets` sockets at most, its sockets metered through the
    /// admission as the program's are: make them through [`World::answer`].
    pub fn new(max_sockets: usize) -> World {
        let wire = Rc::new(RefCell::new(Wire { selfset: Some(selfset()), ..Default::default() }));
        let seeds = Rc::new(RefCell::new(Vec::new()));
        let fail = Rc::new(Cell::new(false));
        let rng = Rc::new(Cell::new(0x9e37_79b9_7f4a_7c15));
        let entropy = Seeds { state: rng.clone(), seeds: seeds.clone(), fail: fail.clone() };
        let net = Net { addr: ADDR, len: LEN, gateway: Some(GATEWAY), selfset: selfset() };
        let mut stack = Stack::new(net, Link::new(Pipe(wire.clone())), entropy, max_sockets);
        let now = 1_000_000;
        assert!(stack.link_up(IPD_MAC, now));
        let roots = [
            (ANY, anywhere()),
            (NARROW, connect_scope(ip(10, 1, 9, 110), 32, 7, 7)),
            (
                LISTENER,
                Scope::new(&[
                    Rule::Listen(Ports::new(22, 22).unwrap()),
                    Rule::Listen(Ports::new(8000, 8000).unwrap()),
                ])
                .unwrap(),
            ),
        ];
        // As `crate::sizing` sets them: a bucket's `State` units (4 + 8, and the listener's 0 + 20).
        let caps = SocketCaps { default: 12, overrides: vec![(LISTENER, 20)] };
        let mut fs = NetFs::new(stack, &roots, caps);
        fs.now = now;
        // As the program sizes itself (`crate::sizing`): sockets are `State` units, so a bucket
        // holds 4 connections and 8 sockets' worth, and the listener badge 20 sockets'.
        let state = crate::sizing::state_for(4, 8);
        let limits = Limits { buckets: 8, in_flight: 5, files: 24, state };
        let listener = Override {
            badge: LISTENER,
            in_flight: 5,
            files: crate::sizing::files_for(20),
            state: crate::sizing::state_for(0, 20),
        };
        let admission = Admission::with_overrides(limits, &[listener]).unwrap();
        let nine = NineServer::with_admission(fs, admission, 0);
        World { nine, wire, peer: Peer::new(now), now, seeds, fail_random: fail, rng }
    }

    pub fn stack(&mut self) -> &mut Stack<Pipe, Seeds> { &mut self.nine.fs.stack }

    /// Moves frames both ways, and polls both stacks, until nothing moves.
    pub fn pump(&mut self) {
        self.nine.fs.now = self.now;
        for _ in 0..10_000 {
            self.nine.fs.stack.poll(self.now);
            crate::server::settle_sockets(&mut self.nine);
            let mut moved = false;
            loop {
                let next = self.wire.borrow_mut().to_peer.pop_front();
                let Some(frame) = next else { break };
                self.peer.take(frame);
                moved = true;
            }
            self.peer.iface.poll(instant(self.now), &mut self.peer.dev, &mut self.peer.sockets);
            while let Some(frame) = self.peer.dev.tx.pop_front() {
                self.nine.fs.stack.ingress(&frame, self.now);
                moved = true;
            }
            if !moved {
                return;
            }
        }
        panic!("the network never went quiet");
    }

    /// Moves the clock on by `us` in steps of `step`, pumping at each.
    pub fn run_for(&mut self, us: u64, step: u64) {
        let end = self.now + us;
        while self.now < end {
            self.now = (self.now + step).min(end);
            self.pump();
        }
    }

    /// Pumps until `done` or `us` have passed.
    pub fn until(&mut self, us: u64, mut done: impl FnMut(&mut World) -> bool) -> bool {
        let end = self.now + us;
        loop {
            self.pump();
            if done(self) {
                return true;
            }
            if self.now >= end {
                return false;
            }
            self.now += 10_000;
        }
    }

    /// Frames `ipd` sent, as (source, destination, SYN-without-ACK) of each TCP segment.
    pub fn tcp_sent(&self) -> Vec<(u32, u32, u16, bool)> {
        self.wire
            .borrow()
            .sent
            .iter()
            .filter_map(|f| {
                let eth = EthernetFrame::new_checked(&f[..]).ok()?;
                let ipp = Ipv4Packet::new_checked(eth.payload()).ok()?;
                let t = TcpPacket::new_checked(ipp.payload()).ok()?;
                Some((
                    u32::from(ipp.src_addr()),
                    u32::from(ipp.dst_addr()),
                    t.dst_port(),
                    t.syn() && !t.ack(),
                ))
            })
            .collect()
    }
}

/// A caller: badge, account and labels.
pub fn caller(badge: u64, account: u64, labels: &[u64]) -> Caller {
    Caller { badge, account, labels: Labels::from_slice(labels).unwrap() }
}

pub fn owner(c: &Caller) -> Owner { Owner { badge: c.badge, key: AdmitKey::of(c) } }

/// A scope of one connect rule.
pub fn connect_scope(addr: u32, len: u8, lo: u16, hi: u16) -> Scope {
    Scope::new(&[Rule::Connect(Prefix::new(addr, len).unwrap(), Ports::new(lo, hi).unwrap())]).unwrap()
}

pub fn listen_scope(lo: u16, hi: u16) -> Scope {
    Scope::new(&[Rule::Listen(Ports::new(lo, hi).unwrap())]).unwrap()
}

pub fn anywhere() -> Scope {
    Scope::new(&[
        Rule::Connect(Prefix::new(0, 0).unwrap(), Ports::new(1, 65535).unwrap()),
        Rule::Listen(Ports::new(1, 65535).unwrap()),
    ])
    .unwrap()
}

/// Waits for the socket's status to stop being `Connecting`.
pub fn settle(w: &mut World, who: Owner, n: u32) -> (Status, u32) {
    let mut last = None;
    let done = w.until(5_000_000, |w| match w.stack().status(who, n, 64).unwrap() {
        Ready::Now(s) => {
            last = Some(s);
            true
        }
        Ready::Wait => false,
    });
    assert!(done, "socket {n} still connecting");
    last.unwrap()
}

// ---- driving ipd from a stream of bytes: the fuzz targets and the seeded sweeps ----

use core::num::NonZeroU64;

use redoubt_rt::abi::{Error, Handle, Handles};
use redoubt_rt::server::minted::Minter;
use redoubt_rt::server::ninep::Answer;
use redoubt_rt::wire::MSIZE;
use redoubt_rt::wire::ninep::{Body, Message, NOFID, Names};
use redoubt_rt::wire::proto::{ipd as ipd_proto, net_ctl, ninep_common};
use smoltcp::phy::ChecksumCapabilities;
use smoltcp::wire::{EthernetRepr, Ipv4Repr, TcpControl, TcpRepr, TcpSeqNumber, TcpTimestampRepr};

/// The kernel's part in minting, for host tests: handles numbered from 100, ids from a
/// generator.
pub struct FakeKernel {
    pub minted: Vec<u64>,
    pub rng: u64,
}

impl FakeKernel {
    pub fn new() -> FakeKernel { FakeKernel { minted: Vec::new(), rng: 77 } }
}

impl Default for FakeKernel {
    fn default() -> Self { FakeKernel::new() }
}

impl Minter for FakeKernel {
    fn mint(&mut self, badge: NonZeroU64) -> Result<Handle, Error> {
        self.minted.push(badge.get());
        Handle::new(99 + (self.minted.len() as u32 % 1000)).ok_or(Error::InvalidArgument)
    }

    fn random(&mut self) -> Result<u64, Error> {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        Ok(self.rng)
    }
}

/// A well-formed, checksummed TCP segment from `src` to `dst`, sent by the peer.
pub fn segment(src: u32, dst: u32, tcp: &TcpRepr) -> Vec<u8> {
    let caps = ChecksumCapabilities::default();
    let ip = Ipv4Repr {
        src_addr: Ipv4Address::from(src),
        dst_addr: Ipv4Address::from(dst),
        next_header: IpProtocol::Tcp,
        payload_len: tcp.buffer_len(),
        hop_limit: 64,
    };
    let eth = EthernetRepr {
        src_addr: EthernetAddress(PEER_MAC),
        dst_addr: EthernetAddress(IPD_MAC),
        ethertype: EthernetProtocol::Ipv4,
    };
    let mut frame = vec![0u8; eth.buffer_len() + ip.buffer_len() + tcp.buffer_len()];
    let mut e = EthernetFrame::new_unchecked(&mut frame[..]);
    eth.emit(&mut e);
    let mut p = Ipv4Packet::new_unchecked(e.payload_mut());
    ip.emit(&mut p, &caps);
    let mut t = TcpPacket::new_unchecked(p.payload_mut());
    tcp.emit(&mut t, &IpAddress::Ipv4(ip.src_addr), &IpAddress::Ipv4(ip.dst_addr), &caps);
    frame
}

/// A well-formed IPv4 packet of any `protocol` from `src` to `dst`, its header checksummed, with
/// `payload` as its body: UDP, ICMP or a protocol nobody speaks, as the wire could carry them.
pub fn datagram(src: u32, dst: u32, protocol: IpProtocol, payload: &[u8]) -> Vec<u8> {
    let caps = ChecksumCapabilities::default();
    let ip = Ipv4Repr {
        src_addr: Ipv4Address::from(src),
        dst_addr: Ipv4Address::from(dst),
        next_header: protocol,
        payload_len: payload.len(),
        hop_limit: 64,
    };
    let eth = EthernetRepr {
        src_addr: EthernetAddress(PEER_MAC),
        dst_addr: EthernetAddress(IPD_MAC),
        ethertype: EthernetProtocol::Ipv4,
    };
    let mut frame = vec![0u8; eth.buffer_len() + ip.buffer_len() + payload.len()];
    let mut e = EthernetFrame::new_unchecked(&mut frame[..]);
    eth.emit(&mut e);
    let mut p = Ipv4Packet::new_unchecked(e.payload_mut());
    ip.emit(&mut p, &caps);
    p.payload_mut().copy_from_slice(payload);
    frame
}

/// An ICMP echo request's bytes, checksummed: what `ping` sends.
pub fn echo_request(payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; 8 + payload.len()];
    bytes[0] = 8;
    bytes[4..6].copy_from_slice(&0x1234u16.to_be_bytes());
    bytes[6..8].copy_from_slice(&1u16.to_be_bytes());
    bytes[8..].copy_from_slice(payload);
    let mut sum = 0u32;
    for pair in bytes.chunks(2) {
        sum += u32::from(u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)]));
    }
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    bytes[2..4].copy_from_slice(&(!(sum as u16)).to_be_bytes());
    bytes
}

/// What a drive did, so a sweep can show it went deep.
#[derive(Clone, Copy, Debug, Default)]
pub struct Drove {
    pub steps: usize,
    /// Frames `ipd` sent.
    pub sent: usize,
    /// 9P requests answered without an error, and held.
    pub answered: usize,
    pub held: usize,
    /// Connections minted (grants and `new_connection`).
    pub minted: usize,
    /// The most sockets alive at once.
    pub sockets: usize,
}

/// Bytes, one at a time, zero once they run out.
pub struct Bytes<'a>(pub &'a [u8]);

impl Bytes<'_> {
    pub fn u8(&mut self) -> u8 {
        match self.0.split_first() {
            Some((b, rest)) => {
                self.0 = rest;
                *b
            }
            None => 0,
        }
    }

    pub fn u16(&mut self) -> u16 { u16::from_le_bytes([self.u8(), self.u8()]) }

    pub fn u32(&mut self) -> u32 { u32::from_le_bytes([self.u8(), self.u8(), self.u8(), self.u8()]) }

    pub fn pick<T: Copy>(&mut self, from: &[T]) -> T { from[usize::from(self.u8()) % from.len()] }

    pub fn take(&mut self, n: usize) -> Vec<u8> { (0..n).map(|_| self.u8()).collect() }

    pub fn is_empty(&self) -> bool { self.0.is_empty() }
}

const SOURCES: &[u32] =
    &[GATEWAY, LAN_HOST, FAR_HOST, ADDR, ip(127, 0, 0, 1), ip(0, 0, 0, 1), SELF_EXTRA, ip(10, 1, 255, 255)];
const PORTS: &[u16] = &[22, 7, 8000, 8001, 49152, 65535, 1, 80];

/// A segment built from the input: addresses and ports from small sets (so it reaches sockets),
/// every flag, number, option and payload byte from the input.
fn fuzzed_segment(b: &mut Bytes) -> Vec<u8> {
    let control =
        b.pick(&[TcpControl::None, TcpControl::Psh, TcpControl::Syn, TcpControl::Fin, TcpControl::Rst]);
    let flags = b.u8();
    let len = usize::from(b.u8() % 96);
    let payload = b.take(len);
    let repr = TcpRepr {
        src_port: b.pick(&[40000, 40001, 7, 8000, 22]),
        dst_port: b.pick(PORTS),
        control,
        seq_number: TcpSeqNumber(b.u32() as i32),
        ack_number: (flags & 1 != 0).then(|| TcpSeqNumber(b.u32() as i32)),
        window_len: b.u16(),
        window_scale: (flags & 2 != 0).then(|| b.u8() % 16),
        max_seg_size: (flags & 4 != 0).then(|| b.u16()),
        sack_permitted: flags & 8 != 0,
        sack_ranges: [(flags & 16 != 0).then(|| (b.u32(), b.u32())), None, None],
        timestamp: (flags & 32 != 0).then(|| TcpTimestampRepr { tsval: b.u32(), tsecr: b.u32() }),
        payload: &payload,
    };
    let src = b.pick(SOURCES);
    let dst = if flags & 64 != 0 { b.pick(SOURCES) } else { ADDR };
    segment(src, dst, &repr)
}

/// A well-formed IPv4 packet that is not TCP, from the input: UDP, ICMP (an echo request, or
/// anything) or another protocol, from any of the sources, to `ipd` or elsewhere. `ipd` must
/// answer none of them (QA D3-code-review-5): [`check_frame`] fails on anything but TCP or ARP.
fn fuzzed_datagram(b: &mut Bytes) -> Vec<u8> {
    let protocol = match b.u8() % 4 {
        0 => IpProtocol::Udp,
        1 => IpProtocol::Icmp,
        2 => IpProtocol::Unknown(99),
        _ => IpProtocol::from(b.u8()),
    };
    let len = usize::from(b.u8() % 64);
    let payload = if protocol == IpProtocol::Icmp && b.u8() % 2 == 0 {
        echo_request(&b.take(len))
    } else {
        b.take(len)
    };
    let src = b.pick(SOURCES);
    let dst = if b.u8() % 4 == 0 { b.pick(SOURCES) } else { ADDR };
    datagram(src, dst, protocol, &payload)
}

/// Drives `ipd`'s stack with frames from the input: well-formed segments with fuzzed fields,
/// well-formed IPv4 that is not TCP, raw bytes, the peer's own traffic, the clock, and the stack's
/// own operations, auditing the
/// table after each. A panic anywhere is the failure; every frame `ipd` sends is checked as it
/// goes ([`check_frame`]). Shared by the `frames` fuzz target and a seeded sweep.
pub fn drive_frames(input: &[u8]) -> Drove {
    let mut drove = Drove::default();
    let mut b = Bytes(input);
    let mut w = World::unmetered(64);
    let sshd = Owner { badge: LISTENER, key: AdmitKey::of(&caller(LISTENER, 0, &[])) };
    let client = Owner { badge: ANY, key: AdmitKey::of(&caller(ANY, 1, &[])) };
    let l = w.nine.fs.stack.allocate(sshd, LISTENER, 20).unwrap();
    w.nine.fs.stack.listen(sshd, l, &anywhere(), 8000, 2, 20).unwrap();
    let _echo = w.peer.listen(7);
    let c = w.nine.fs.stack.allocate(client, ANY, 8).unwrap();
    let now = w.now;
    w.nine.fs.stack.connect(client, c, &anywhere(), LAN_HOST, 7, now).unwrap();
    w.pump();
    for _ in 0..48 {
        if b.is_empty() {
            break;
        }
        let now = w.now;
        match b.u8() % 9 {
            0 | 1 => {
                let frame = fuzzed_segment(&mut b);
                w.nine.fs.stack.ingress(&frame, now);
            }
            8 => {
                let frame = fuzzed_datagram(&mut b);
                w.nine.fs.stack.ingress(&frame, now);
            }
            2 => {
                let len = usize::from(b.u8() % 96);
                let frame = b.take(len);
                w.nine.fs.stack.ingress(&frame, now);
            }
            3 => w.run_for(u64::from(b.u8()) * 20_000, 10_000),
            4 => {
                let len = usize::from(b.u8());
                let data = b.take(len);
                let _ = w.nine.fs.stack.send(client, c, &data);
                let mut out = [0u8; 512];
                let _ = w.nine.fs.stack.recv(client, c, &mut out);
            }
            5 => {
                let port = b.pick(&[41000, 41001, 41002]);
                let _ = w.peer.connect(GATEWAY, port, 8000);
            }
            6 => match w.nine.fs.stack.status(sshd, l, 20) {
                Ok(Ready::Now((Status::Listening, m))) if b.u8() % 2 == 0 => {
                    let _ = w.nine.fs.stack.release(sshd, m, b.u8() % 2 == 0, now);
                }
                _ => {}
            },
            _ => w.pump(),
        }
        w.nine.fs.stack.audit().unwrap();
        drove.steps += 1;
        drove.sockets = drove.sockets.max(w.nine.fs.stack.live());
    }
    w.run_for(70_000_000, 5_000_000);
    w.nine.fs.stack.audit().unwrap();
    drove.sent = w.wire.borrow().sent.len();
    drove
}

/// A 9P request from the input, as `who`, answered in place: whether it was answered without an
/// error, and whether it was held.
fn fuzzed_9p(w: &mut World, who: &Caller, b: &mut Bytes) -> (bool, bool) {
    const NAMES: &[&str] = &["tcp", "clone", "0", "1", "2", "ctl", "data", "remote", "..", "x", "00"];
    let fid = u32::from(b.u8() % 8);
    let newfid = u32::from(b.u8() % 8);
    let names: Vec<&str> = (0..b.u8() % 4).map(|_| b.pick(NAMES)).collect();
    let data = match b.u8() % 5 {
        0 => {
            let any = b.u32();
            let addr =
                b.pick(&[LAN_HOST, FAR_HOST, ADDR, SELF_EXTRA, ip(10, 1, 9, 110), ip(127, 0, 0, 1), any]);
            op_bytes(net_ctl::Message::Connect(net_ctl::Connect {
                addr: &addr.to_be_bytes(),
                port: b.pick(PORTS),
            }))
        }
        1 => {
            op_bytes(net_ctl::Message::Listen(net_ctl::Listen { port: b.pick(PORTS), backlog: b.u8() % 10 }))
        }
        2 => op_bytes(net_ctl::Message::Close(net_ctl::Close {})),
        3 => op_bytes(net_ctl::Message::Abort(net_ctl::Abort {})),
        _ => {
            let len = usize::from(b.u8() % 40);
            b.take(len)
        }
    };
    let body = match b.u8() % 9 {
        0 => Body::Tattach { fid, afid: NOFID, uname: "", aname: "" },
        1 | 2 => Body::Twalk { fid, newfid, wnames: Names::new(&names).unwrap() },
        3 => Body::Topen { fid, mode: b.pick(&[0, 1, 2, 3, 0x10]) },
        4 | 5 => Body::Tread { fid, offset: u64::from(b.u8() % 8), count: u32::from(b.u16()) },
        6 => Body::Twrite { fid, offset: 0, data: &data },
        7 => Body::Tclunk { fid },
        _ => Body::Tstat { fid },
    };
    let mut buf = vec![0u8; MSIZE];
    if (Message { tag: 1, body }).encode(&mut buf).is_err() {
        return (false, false);
    }
    let answer = w.answer(who, &mut buf);
    let _ = w.nine.fs.take_wait();
    let ok = answer == Answer::Replied
        && !matches!(Message::decode(&buf).expect("ipd's reply does not decode").body, Body::Rerror { .. });
    (ok, answer == Answer::Waiting)
}

/// One 9P request as `who`: `Ok(Some(data))` for a read's data, `Ok(None)` for any other
/// success, `Err(true)` if it was held, `Err(false)` for an `Rerror`.
fn rpc(w: &mut World, who: &Caller, body: Body<'_>) -> Result<Option<Vec<u8>>, bool> {
    let mut buf = vec![0u8; MSIZE];
    (Message { tag: 1, body }).encode(&mut buf).map_err(|_| false)?;
    let answer = w.answer(who, &mut buf);
    let _ = w.nine.fs.take_wait();
    if answer == Answer::Waiting {
        return Err(true);
    }
    match Message::decode(&buf).expect("ipd's reply does not decode").body {
        Body::Rerror { .. } => Err(false),
        Body::Rread { data } => Ok(Some(data.to_vec())),
        _ => Ok(None),
    }
}

/// Makes `who` a socket the way a client does (attaching at fid 0 and walking fid 1 to `/tcp`
/// first if need be); its number.
fn make_socket(w: &mut World, who: &Caller) -> Option<u32> {
    if rpc(w, who, Body::Twalk { fid: 0, newfid: 1, wnames: Names::new(&["tcp"]).unwrap() }).is_err() {
        let _ = rpc(w, who, Body::Tattach { fid: 0, afid: NOFID, uname: "", aname: "" });
        let _ = rpc(w, who, Body::Twalk { fid: 0, newfid: 1, wnames: Names::new(&["tcp"]).unwrap() });
    }
    let _ = rpc(w, who, Body::Tclunk { fid: 2 });
    rpc(w, who, Body::Twalk { fid: 1, newfid: 2, wnames: Names::new(&["clone"]).unwrap() }).ok()?;
    rpc(w, who, Body::Topen { fid: 2, mode: 0 }).ok()?;
    let data = rpc(w, who, Body::Tread { fid: 2, offset: 0, count: 64 }).ok()??;
    let _ = rpc(w, who, Body::Tclunk { fid: 2 });
    Some(u32::from_le_bytes(data.get(..4)?.try_into().ok()?))
}

/// Opens socket `n`'s `file` read-write at fid 3 (clunking what was there).
fn open_socket_file(w: &mut World, who: &Caller, n: u32, file: &str) -> bool {
    let _ = rpc(w, who, Body::Tclunk { fid: 3 });
    let name = alloc::format!("{n}");
    rpc(w, who, Body::Twalk { fid: 1, newfid: 3, wnames: Names::new(&[&name, file]).unwrap() }).is_ok()
        && rpc(w, who, Body::Topen { fid: 3, mode: 2 }).is_ok()
}

fn op_bytes(message: net_ctl::Message<'_>) -> Vec<u8> {
    let mut out = vec![0u8; 64];
    let n = message.encode_file(&mut out).unwrap_or(0);
    out.truncate(n);
    out
}

/// The sockets' ledger (QA D3-code-review-5, P2-2): no reservation outlives its request, and
/// every bucket holds at least one `State` unit for each of its live sockets.
pub fn check_ledger(w: &World) {
    assert_eq!(w.nine.fs.stack.meter(), crate::stack::Meter::Closed, "a reservation outlived its request");
    for key in w.nine.fs.stack.charged_keys() {
        let held = w.nine.admission().held(key, redoubt_rt::server::Resource::State) as usize;
        assert!(
            held >= w.nine.fs.stack.charged(key),
            "sockets not paid for: {held} units for {}",
            w.nine.fs.stack.charged(key)
        );
    }
}

/// Drives `/net` through the 9P skeleton with requests, `ctl` operations, grants,
/// `new_connection` and `disconnect` from several callers, interleaved with frames and the
/// clock. Checked after every step: the table's invariants ([`Stack::audit`]) and every frame
/// `ipd` sends. Shared by the `session` fuzz target and a seeded sweep.
pub fn drive_session(input: &[u8]) -> Drove {
    let mut drove = Drove::default();
    let mut b = Bytes(input);
    let mut w = World::new(64);
    let mut k = FakeKernel::new();
    let _echo = w.peer.listen(7);
    let mut callers: Vec<Caller> = vec![
        caller(ANY, 1, &[]),
        caller(NARROW, 2, &[]),
        caller(LISTENER, 0, &[]),
        caller(INGRESS, 0, &[]),
        caller(ANY, 0, &[]),
    ];
    let mut ids: Vec<(Caller, u64)> = Vec::new();
    for _ in 0..64 {
        if b.is_empty() {
            break;
        }
        // Half the time the first caller, whose scope reaches the peer.
        let pick = usize::from(b.u8());
        let who = if pick % 2 == 0 { callers[0] } else { callers[pick / 2 % callers.len()] };
        match b.u8() % 14 {
            0 | 1 => {
                let (ok, held) = fuzzed_9p(&mut w, &who, &mut b);
                drove.answered += usize::from(ok);
                drove.held += usize::from(held);
            }
            2 => {
                drove.answered += usize::from(make_socket(&mut w, &who).is_some());
            }
            10..=13 => {
                // An operation on one of the caller's sockets, through its files.
                let mut numbers =
                    w.nine.fs.stack.numbers(Owner { badge: who.badge, key: AdmitKey::of(&who) });
                if numbers.is_empty() {
                    numbers.extend(make_socket(&mut w, &who));
                }
                let Some(&n) = numbers.get(usize::from(b.u8()) % numbers.len().max(1)) else { continue };
                let file = b.pick(&["ctl", "data", "remote"]);
                if !open_socket_file(&mut w, &who, n, file) {
                    continue;
                }
                let body_data;
                let result = match (file, b.u8() % 2) {
                    ("ctl", 0) => {
                        let any = b.u32();
                        let addr = b.pick(&[LAN_HOST, FAR_HOST, ADDR, SELF_EXTRA, ip(10, 1, 9, 110), any]);
                        body_data = match b.u8() % 4 {
                            0 => op_bytes(net_ctl::Message::Connect(net_ctl::Connect {
                                addr: &addr.to_be_bytes(),
                                port: b.pick(PORTS),
                            })),
                            1 => op_bytes(net_ctl::Message::Listen(net_ctl::Listen {
                                port: b.pick(PORTS),
                                backlog: 1 + b.u8() % 3,
                            })),
                            2 => op_bytes(net_ctl::Message::Close(net_ctl::Close {})),
                            _ => op_bytes(net_ctl::Message::Abort(net_ctl::Abort {})),
                        };
                        rpc(&mut w, &who, Body::Twrite { fid: 3, offset: 0, data: &body_data })
                    }
                    ("data", 0) => {
                        let len = usize::from(b.u16() % 9000);
                        body_data = vec![b.u8(); len];
                        rpc(&mut w, &who, Body::Twrite { fid: 3, offset: 0, data: &body_data })
                    }
                    _ => rpc(&mut w, &who, Body::Tread { fid: 3, offset: 0, count: u32::from(b.u16()) }),
                };
                match result {
                    Ok(_) => drove.answered += 1,
                    Err(true) => drove.held += 1,
                    Err(false) => {}
                }
            }
            5 => {
                let scope = if b.u8() % 2 == 0 {
                    let len = b.pick(&[0u8, 8, 16, 24, 32]);
                    let addr = b.pick(&[LAN_HOST, 0, ip(10, 1, 9, 0), FAR_HOST]) & crate::scope::mask(len);
                    let lo = b.pick(PORTS);
                    let rules = [Rule::Connect(Prefix::new(addr, len).unwrap(), Ports::new(lo, lo).unwrap())];
                    Scope::new(&rules).unwrap().encode()
                } else {
                    let len = usize::from(b.u8() % 24);
                    b.take(len)
                };
                let mut lend = vec![0u8; 256];
                let words =
                    ipd_proto::Message::Grant(ipd_proto::Grant { scope: &scope }).encode(&mut lend).unwrap();
                let (outcome, minted) = crate::server::answer_grant(
                    &mut w.nine,
                    &who,
                    &words,
                    &Handles::new(),
                    &mut lend,
                    &mut k,
                );
                if let (Ok(Ok(ipd_proto::Reply::Grant(r))), Some(badge)) = (
                    ipd_proto::Reply::decode(16, &outcome.words, &lend, outcome.send.as_slice().len()),
                    minted,
                ) {
                    callers.push(caller(badge, who.account, &[]));
                    ids.push((who, r.id));
                    drove.minted += 1;
                }
            }
            6 => {
                let root = b.pick(&["", "tcp", "tcp/0", "..", "tcp/clone"]);
                let mut lend = vec![0u8; 256];
                let words =
                    ninep_common::Message::NewConnection(ninep_common::NewConnection { root, quota: 0 })
                        .encode(&mut lend)
                        .unwrap();
                let outcome = w.nine.answer_common(&who, &words, &Handles::new(), &mut lend, &mut k);
                if let Ok(Ok(ninep_common::Reply::NewConnection(r))) =
                    ninep_common::Reply::decode(2, &outcome.words, &lend, outcome.send.as_slice().len())
                {
                    callers.push(caller(*k.minted.last().unwrap(), who.account, &[]));
                    ids.push((who, r.id));
                    drove.minted += 1;
                }
            }
            7 if !ids.is_empty() => {
                let (holder, id) = ids[usize::from(b.u8()) % ids.len()];
                let asker = if b.u8() % 4 == 0 { who } else { holder };
                let words = ninep_common::Message::Disconnect(ninep_common::Disconnect { id })
                    .encode(&mut [])
                    .unwrap();
                let _ = w.nine.answer_common(&asker, &words, &Handles::new(), &mut [], &mut k);
            }
            8 => {
                let frame = fuzzed_segment(&mut b);
                let now = w.now;
                w.nine.fs.stack.ingress(&frame, now);
            }
            9 => {
                let port = b.pick(&[22u16, 8000]);
                let from = b.pick(&[41000u16, 41001, 41002, 41003]);
                let _ = w.peer.connect(GATEWAY, from, port);
                w.pump();
            }
            _ => w.run_for(u64::from(b.u8()) * 50_000, 25_000),
        }
        w.nine.fs.stack.audit().unwrap();
        check_ledger(&w);
        drove.steps += 1;
        drove.sockets = drove.sockets.max(w.nine.fs.stack.live());
    }
    // Everything ends: every connection disconnected, the clock past every linger.
    for (holder, id) in ids {
        let words =
            ninep_common::Message::Disconnect(ninep_common::Disconnect { id }).encode(&mut []).unwrap();
        let _ = w.nine.answer_common(&holder, &words, &Handles::new(), &mut [], &mut k);
    }
    w.run_for(75_000_000, 5_000_000);
    w.nine.fs.stack.audit().unwrap();
    assert_eq!(w.nine.fs.minted_connections(), 0, "a disconnected connection is still minted");
    // No connection is left, so each bucket's `State` units are exactly its live sockets (a root
    // badge's sockets live on; every released one is gone and its unit given back).
    for who in &callers {
        let key = AdmitKey::of(who);
        let held = w.nine.admission().held(key, redoubt_rt::server::Resource::State) as usize;
        assert_eq!(held, w.nine.fs.stack.charged(key), "units and sockets do not balance");
    }
    drove.sent = w.wire.borrow().sent.len();
    drove
}
