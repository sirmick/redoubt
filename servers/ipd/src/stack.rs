//! The TCP/IP stack: smoltcp's interface and sockets, and `ipd`'s table of whose each socket is
//! (NAMESPACES.md, What `ipd` serves; answer 174).
//!
//! # Whose a socket is
//! Every socket belongs to one [`Owner`]: the badge of the connection that made it and the
//! caller's admission key. Only that connection sees it, by a number of its own (the lowest
//! free), and it is charged to the owner's bucket until smoltcp reports it closed and `ipd`
//! removes it. A socket its owner has closed, aborted or disconnected is no longer visible and
//! lingers at most [`LINGER_US`] more before `ipd` resets it.
//!
//! # Initial sequence numbers (answer 174, decision 11)
//! smoltcp draws an ISN from its interface's PRNG, which is seeded only when an interface is
//! made. So the main interface never makes one: each active open is connected through the
//! context of a **fresh interface** seeded from the kernel's CSPRNG ([`Entropy`]), and each SYN
//! to a listening port is taken in through a fresh one's ingress. The fresh interface has the
//! same address and route but an empty neighbour cache, so a reply it would send at once is
//! lost rather than sent; the SYN-ACK goes out later from the main interface, carrying the ISN
//! the fresh one drew. With no seed there is no open: the connect answers `unreachable`, the SYN
//! is dropped, and nothing falls back to the main PRNG.
//!
//! # What is checked before smoltcp sees anything
//! A `connect` is refused (`not_permitted`) to the box's own addresses ([`SelfSet`]) before the
//! scope is looked at, then by the scope, then by the socket's state. Inbound TCP from a martian
//! source (any of the box's own addresses but the gateway: [`classify`]) is dropped.

use alloc::vec;
use alloc::vec::Vec;

use redoubt_rt::server::AdmitKey;
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::socket::tcp::{self, State};
use smoltcp::time::{Duration, Instant};
use smoltcp::wire::{
    EthernetAddress, EthernetFrame, EthernetProtocol, HardwareAddress, IpAddress, IpCidr, IpListenEndpoint,
    IpProtocol, Ipv4Address, Ipv4Packet, TcpPacket,
};

use crate::link::{Link, Netif};
use crate::scope::{Scope, SelfSet, martian_source};

/// Bytes of each socket's receive and send buffers.
pub const BUFFER: usize = 8 * 1024;
/// How long a closed, aborted or disconnected socket may linger before `ipd` resets it (µs).
pub const LINGER_US: u64 = 60_000_000;
/// How long a half-open connection (SYN received, never answered) holds a backlog slot (µs).
pub const HALF_OPEN_US: u64 = 3_000_000;
/// An established socket with data unacknowledged, or silent past keep-alive, this long ends.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
pub const KEEP_ALIVE: Duration = Duration::from_secs(30);
/// Ephemeral ports: drawn at random from here, unique among every live socket.
pub const EPHEMERAL: core::ops::RangeInclusive<u16> = 49152..=65535;
/// Draws of an ephemeral port before `too_many`.
pub const PORT_TRIES: usize = 16;
/// The most sockets one listener keeps listening.
pub const MAX_BACKLOG: u8 = 8;

/// What a draw from the kernel's CSPRNG is for, so tests can tell seeds from ports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    /// A fresh interface's seed: one per active open, one per SYN taken in, one for the main
    /// interface.
    Seed,
    /// An ephemeral port.
    Port,
}

/// The kernel's CSPRNG (`random`); in tests, an injected source.
pub trait Entropy {
    fn draw(&mut self, why: Use) -> Option<u64>;
}

/// Whose a socket is: the connection's badge, and the caller's admission key (so holders of a
/// copied handle in another account never see its sockets).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Owner {
    pub badge: u64,
    pub key: AdmitKey,
}

/// Why a `ctl` operation failed: `net_ctl`'s error names (NAMESPACES.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtlError {
    NotPermitted,
    InUse,
    TooMany,
    State,
    Unreachable,
    Refused,
    Timeout,
}

impl CtlError {
    /// The `Rerror` text: the error's name in the `net_ctl` table.
    pub fn name(self) -> &'static str {
        match self {
            CtlError::NotPermitted => "not_permitted",
            CtlError::InUse => "in_use",
            CtlError::TooMany => "too_many",
            CtlError::State => "state",
            CtlError::Unreachable => "unreachable",
            CtlError::Refused => "refused",
            CtlError::Timeout => "timeout",
        }
    }
}

/// A socket's state as `ctl` reads it (NAMESPACES.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Status {
    Connecting = 1,
    Established = 2,
    Closing = 3,
    Closed = 4,
    Listening = 5,
}

/// What a read or a write of a socket would do now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ready<T> {
    Now(T),
    /// Nothing yet: the call waits.
    Wait,
}

/// What a parked call waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitFor {
    /// `ctl` read: a connect to finish, or a listener to accept.
    Ctl,
    /// `data` read: bytes, or the peer's end.
    Recv,
    /// `data` write: room in the send buffer.
    Send,
}

/// The network's configuration (`ipd`'s arguments).
#[derive(Clone, Debug)]
pub struct Net {
    pub addr: u32,
    pub len: u8,
    pub gateway: Option<u32>,
    pub selfset: SelfSet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Made by reading `clone`; neither connected nor listening yet.
    Fresh(SocketHandle),
    /// Connecting, connected, or ended.
    Conn(SocketHandle),
    /// `listen` was done on it: it holds no socket, its backlog does.
    Listener { port: u16, backlog: u8 },
    /// One of a listener's listening sockets; invisible until a `ctl` read accepts it.
    Backlog { listener: u64, handle: SocketHandle, half_open_since: Option<u64> },
}

impl Kind {
    fn handle(&self) -> Option<SocketHandle> {
        match *self {
            Kind::Fresh(h) | Kind::Conn(h) | Kind::Backlog { handle: h, .. } => Some(h),
            Kind::Listener { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    /// Unique for the life of the stack; a backlog names its listener by it.
    id: u64,
    owner: Owner,
    /// The port-ownership group of the connection that made it (a listener's port belongs to
    /// it: NAMESPACES.md, `listen`).
    group: u64,
    /// The owner's number for it; `None` once released, and for a backlog socket.
    n: Option<u32>,
    kind: Kind,
    /// When it was closed, aborted or disconnected: it is reset at this deadline if smoltcp
    /// has not ended it by then.
    linger_until: Option<u64>,
}

impl Entry {
    /// A listener on `port` its owner still holds.
    fn listens_on(&self, port: u16) -> bool {
        self.linger_until.is_none() && matches!(self.kind, Kind::Listener { port: p, .. } if p == port)
    }
}

/// The stack and its socket table.
pub struct Stack<N: Netif, E: Entropy> {
    pub link: Link<N>,
    entropy: E,
    net: Net,
    mac: Option<EthernetAddress>,
    iface: Option<Interface>,
    sockets: SocketSet<'static>,
    table: Vec<Entry>,
    next_id: u64,
    /// The most sockets that may exist at once, every bucket at its cap.
    max_sockets: usize,
}

fn instant(us: u64) -> Instant { Instant::from_micros(i64::try_from(us).unwrap_or(i64::MAX)) }

fn v4(addr: u32) -> Ipv4Address { Ipv4Address::from(addr) }

fn u32_of(addr: Ipv4Address) -> u32 { u32::from(addr) }

impl<N: Netif, E: Entropy> Stack<N, E> {
    /// A stack with no link yet: until [`Stack::link_up`] gives it a MAC it answers `unreachable`.
    pub fn new(net: Net, link: Link<N>, entropy: E, max_sockets: usize) -> Stack<N, E> {
        Stack {
            link,
            entropy,
            net,
            mac: None,
            iface: None,
            sockets: SocketSet::new(vec![]),
            table: Vec::new(),
            next_id: 1,
            max_sockets,
        }
    }

    pub fn net(&self) -> &Net { &self.net }

    /// `netd` answered `info` with `mac`: the link is up. The first time, the main interface is
    /// made (its seed from the CSPRNG; with none it stays down, and the next `info` tries again).
    /// A MAC that is not unicast leaves the link down.
    pub fn link_up(&mut self, mac: [u8; 6], now: u64) -> bool {
        let mac = EthernetAddress(mac);
        if !mac.is_unicast() || mac.0 == [0; 6] {
            return false;
        }
        match self.iface.as_mut() {
            Some(iface) => iface.set_hardware_addr(HardwareAddress::Ethernet(mac)),
            None => {
                let Some(seed) = self.entropy.draw(Use::Seed) else { return false };
                self.mac = Some(mac);
                self.iface = Some(self.interface(mac, seed, now));
            }
        }
        self.mac = Some(mac);
        self.link.up();
        true
    }

    /// Whether the stack can reach the network: it has an interface and `netd` has not said
    /// its device is broken.
    pub fn is_up(&self) -> bool { self.iface.is_some() && !self.link.is_down() }

    /// An interface with the box's address and route, seeded with `seed`: the main one, or a
    /// fresh one for one ISN. Both are made here, the same way.
    fn interface(&mut self, mac: EthernetAddress, seed: u64, now: u64) -> Interface {
        let mut config = Config::new(HardwareAddress::Ethernet(mac));
        config.random_seed = seed;
        let mut iface = Interface::new(config, &mut self.link, instant(now));
        let (addr, len) = (self.net.addr, self.net.len);
        iface.update_ip_addrs(|addrs| {
            let _ = addrs.push(IpCidr::new(IpAddress::Ipv4(v4(addr)), len));
        });
        if let Some(gateway) = self.net.gateway {
            let _ = iface.routes_mut().add_default_ipv4_route(v4(gateway));
        }
        iface
    }

    /// A fresh interface for one ISN, if the CSPRNG gives a seed.
    fn fresh(&mut self, now: u64) -> Option<Interface> {
        let mac = self.mac?;
        let seed = self.entropy.draw(Use::Seed)?;
        Some(self.interface(mac, seed, now))
    }

    // ---- the table ----

    fn entry(&self, owner: Owner, n: u32) -> Option<usize> {
        self.table.iter().position(|e| e.owner == owner && e.n == Some(n))
    }

    /// The sockets charged to `key`'s bucket: every one holding a smoltcp socket.
    pub fn charged(&self, key: AdmitKey) -> usize {
        self.table.iter().filter(|e| e.owner.key == key && e.kind.handle().is_some()).count()
    }

    /// Every socket holding a smoltcp socket, whoever's.
    pub fn live(&self) -> usize { self.table.iter().filter(|e| e.kind.handle().is_some()).count() }

    /// The owner's visible numbers, lowest first.
    pub fn numbers(&self, owner: Owner) -> Vec<u32> {
        let mut ns: Vec<u32> = self.table.iter().filter(|e| e.owner == owner).filter_map(|e| e.n).collect();
        ns.sort_unstable();
        ns
    }

    pub fn exists(&self, owner: Owner, n: u32) -> bool { self.entry(owner, n).is_some() }

    /// Every visible socket with each thing a call on it may wait for.
    pub fn waits(&self) -> impl Iterator<Item = (Owner, u32, WaitFor)> + '_ {
        self.table.iter().filter_map(|e| e.n.map(|n| (e.owner, n))).flat_map(|(owner, n)| {
            [WaitFor::Ctl, WaitFor::Recv, WaitFor::Send].into_iter().map(move |what| (owner, n, what))
        })
    }

    fn lowest_free(&self, owner: Owner) -> u32 {
        let taken = self.numbers(owner);
        let mut n = 0;
        for t in taken {
            if t != n {
                break;
            }
            n += 1;
        }
        n
    }

    /// Whether one more socket may be charged to `key`, whose bucket's cap is `cap`.
    fn room(&self, key: AdmitKey, cap: usize, more: usize) -> bool {
        self.charged(key) + more <= cap && self.live() + more <= self.max_sockets
    }

    fn new_socket(&mut self) -> SocketHandle {
        let socket = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0; BUFFER]),
            tcp::SocketBuffer::new(vec![0; BUFFER]),
        );
        self.sockets.add(socket)
    }

    fn push(&mut self, owner: Owner, group: u64, n: Option<u32>, kind: Kind) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.table.push(Entry { id, owner, group, n, kind, linger_until: None });
        id
    }

    /// Reading `clone`: a new socket, charged to `owner`'s bucket (cap `cap`), numbered for it.
    pub fn allocate(&mut self, owner: Owner, group: u64, cap: usize) -> Result<u32, CtlError> {
        if !self.room(owner.key, cap, 1) {
            return Err(CtlError::TooMany);
        }
        let n = self.lowest_free(owner);
        let handle = self.new_socket();
        self.push(owner, group, Some(n), Kind::Fresh(handle));
        Ok(n)
    }

    /// Every local port in use by a live socket (listening, connecting, connected, or ending,
    /// TIME-WAIT included), and every listened port.
    fn port_in_use(&self, port: u16) -> bool {
        self.table.iter().any(|e| match e.kind {
            Kind::Listener { port: p, .. } => p == port,
            _ => e.kind.handle().is_some_and(|h| {
                let s = self.sockets.get::<tcp::Socket>(h);
                s.local_endpoint().is_some_and(|l| l.port == port) || s.listen_endpoint().port == port
            }),
        })
    }

    /// Whether `group` may not listen on `port`: another group listens on it, or, with no
    /// listener on it at all, some live socket uses it. A port a group listens on is that group's
    /// (the connection that listened first and those it minted with `new_connection`), and more of
    /// its listeners join the same port.
    fn port_taken(&self, port: u16, group: u64) -> bool {
        let mut listeners = self.table.iter().filter(|e| e.listens_on(port));
        match listeners.next() {
            Some(first) => first.group != group || listeners.any(|e| e.group != group),
            None => self.port_in_use(port),
        }
    }

    fn ephemeral_port(&mut self) -> Result<u16, CtlError> {
        let span = u64::from(EPHEMERAL.end() - EPHEMERAL.start()) + 1;
        for _ in 0..PORT_TRIES {
            let draw = self.entropy.draw(Use::Port).ok_or(CtlError::Unreachable)?;
            let port = EPHEMERAL.start() + (draw % span) as u16;
            if !self.port_in_use(port) {
                return Ok(port);
            }
        }
        Err(CtlError::TooMany)
    }

    /// `connect`: the box's own addresses first, whatever the scope says, then the scope, then
    /// the socket's state and the link. Success means the SYN will go out on the next poll.
    pub fn connect(
        &mut self,
        owner: Owner,
        n: u32,
        scope: &Scope,
        addr: u32,
        port: u16,
        now: u64,
    ) -> Result<(), CtlError> {
        let i = self.entry(owner, n).ok_or(CtlError::State)?;
        if self.net.selfset.contains(addr) || !scope.permits_connect(addr, port) {
            return Err(CtlError::NotPermitted);
        }
        let Kind::Fresh(handle) = self.table[i].kind else { return Err(CtlError::State) };
        if !self.is_up() {
            return Err(CtlError::Unreachable);
        }
        let local = self.ephemeral_port()?;
        let mut fresh = self.fresh(now).ok_or(CtlError::Unreachable)?;
        let socket = self.sockets.get_mut::<tcp::Socket>(handle);
        socket.set_timeout(Some(IDLE_TIMEOUT));
        socket.set_keep_alive(Some(KEEP_ALIVE));
        let remote = (IpAddress::Ipv4(v4(addr)), port);
        let local = IpListenEndpoint { addr: Some(IpAddress::Ipv4(v4(self.net.addr))), port: local };
        socket.connect(fresh.context(), remote, local).map_err(|_| CtlError::State)?;
        self.table[i].kind = Kind::Conn(handle);
        Ok(())
    }

    /// `listen`: the scope first, then the state, the link, the port's owner, and room for the
    /// backlog. The socket becomes a listener holding `backlog` listening sockets, all charged to
    /// `owner`.
    #[allow(clippy::too_many_arguments)]
    pub fn listen(
        &mut self,
        owner: Owner,
        n: u32,
        scope: &Scope,
        port: u16,
        backlog: u8,
        cap: usize,
    ) -> Result<(), CtlError> {
        let i = self.entry(owner, n).ok_or(CtlError::State)?;
        if !scope.permits_listen(port) {
            return Err(CtlError::NotPermitted);
        }
        let Kind::Fresh(first) = self.table[i].kind else { return Err(CtlError::State) };
        if !(1..=MAX_BACKLOG).contains(&backlog) || port == 0 {
            return Err(CtlError::State);
        }
        if !self.is_up() {
            return Err(CtlError::Unreachable);
        }
        let group = self.table[i].group;
        if self.port_taken(port, group) {
            return Err(CtlError::InUse);
        }
        // The socket `clone` made is the first of the backlog; the rest need room.
        if !self.room(owner.key, cap, usize::from(backlog) - 1) {
            return Err(CtlError::TooMany);
        }
        let listener = self.table[i].id;
        self.table[i].kind = Kind::Listener { port, backlog };
        let mut handles = vec![first];
        for _ in 1..backlog {
            handles.push(self.new_socket());
        }
        for handle in handles {
            self.push(owner, group, None, Kind::Backlog { listener, handle, half_open_since: None });
            self.relisten(handle, port);
        }
        Ok(())
    }

    fn relisten(&mut self, handle: SocketHandle, port: u16) {
        let addr = IpAddress::Ipv4(v4(self.net.addr));
        let socket = self.sockets.get_mut::<tcp::Socket>(handle);
        socket.abort();
        socket.set_timeout(Some(Duration::from_micros(HALF_OPEN_US)));
        socket.set_keep_alive(None);
        let _ = socket.listen(IpListenEndpoint { addr: Some(addr), port });
    }

    /// `close` (graceful) or `abort` of the owner's socket `n`: it is released, no longer
    /// visible, and lingers at most [`LINGER_US`]. A listener's backlog goes with it.
    pub fn release(&mut self, owner: Owner, n: u32, graceful: bool, now: u64) -> Result<(), CtlError> {
        let i = self.entry(owner, n).ok_or(CtlError::State)?;
        self.release_at(i, graceful, now);
        Ok(())
    }

    fn release_at(&mut self, i: usize, graceful: bool, now: u64) {
        let entry = self.table[i];
        self.table[i].n = None;
        self.table[i].linger_until = Some(now.saturating_add(LINGER_US));
        match entry.kind {
            Kind::Listener { .. } => {
                for j in 0..self.table.len() {
                    if let Kind::Backlog { listener, handle, .. } = self.table[j].kind {
                        if listener == entry.id {
                            self.sockets.get_mut::<tcp::Socket>(handle).abort();
                            self.table[j].linger_until = Some(now);
                            // No longer anyone's backlog: it goes once smoltcp has ended it.
                            self.table[j].kind = Kind::Conn(handle);
                        }
                    }
                }
            }
            kind => {
                if let Some(handle) = kind.handle() {
                    let socket = self.sockets.get_mut::<tcp::Socket>(handle);
                    if graceful { socket.close() } else { socket.abort() }
                }
            }
        }
    }

    /// The connection with `badge` is gone (`disconnect`): every socket it owns is aborted and
    /// released.
    pub fn disconnect(&mut self, badge: u64, now: u64) {
        for i in 0..self.table.len() {
            if self.table[i].owner.badge == badge && self.table[i].linger_until.is_none() {
                if matches!(self.table[i].kind, Kind::Backlog { .. }) {
                    continue; // its listener releases it
                }
                self.release_at(i, false, now);
            }
        }
    }

    // ---- reading and writing ----

    /// `ctl` read: the socket's state and number, or [`Ready::Wait`] while it connects, or while a
    /// listener has nothing to accept. A listener's read accepts one connection: a new number,
    /// charged to the owner, whose slot in the backlog is refilled if there is room (`cap`).
    pub fn status(&mut self, owner: Owner, n: u32, cap: usize) -> Result<Ready<(Status, u32)>, CtlError> {
        let i = self.entry(owner, n).ok_or(CtlError::State)?;
        match self.table[i].kind {
            Kind::Fresh(_) => Ok(Ready::Now((Status::Closed, n))),
            Kind::Conn(h) | Kind::Backlog { handle: h, .. } => {
                let status = status_of(self.sockets.get::<tcp::Socket>(h).state());
                Ok(if status == Status::Connecting { Ready::Wait } else { Ready::Now((status, n)) })
            }
            Kind::Listener { port, .. } => {
                let id = self.table[i].id;
                let Some(j) = self.accepted(id) else { return Ok(Ready::Wait) };
                let Kind::Backlog { handle, .. } = self.table[j].kind else { return Ok(Ready::Wait) };
                let m = self.lowest_free(owner);
                self.table[j].kind = Kind::Conn(handle);
                self.table[j].n = Some(m);
                let socket = self.sockets.get_mut::<tcp::Socket>(handle);
                socket.set_timeout(Some(IDLE_TIMEOUT));
                socket.set_keep_alive(Some(KEEP_ALIVE));
                self.refill(owner, id, port, cap);
                Ok(Ready::Now((Status::Listening, m)))
            }
        }
    }

    /// A backlog socket of listener `id` that has a connection to hand over.
    fn accepted(&self, id: u64) -> Option<usize> {
        self.table.iter().position(|e| match e.kind {
            Kind::Backlog { listener, handle, .. } => {
                listener == id
                    && matches!(
                        self.sockets.get::<tcp::Socket>(handle).state(),
                        State::Established | State::CloseWait
                    )
            }
            _ => false,
        })
    }

    /// Tops listener `id`'s backlog back up to its size, as far as the owner's cap allows.
    fn refill(&mut self, owner: Owner, id: u64, port: u16, cap: usize) {
        let Some(l) = self.table.iter().position(|e| e.id == id) else { return };
        let (Kind::Listener { backlog, .. }, group) = (self.table[l].kind, self.table[l].group) else {
            return;
        };
        let have = self
            .table
            .iter()
            .filter(|e| matches!(e.kind, Kind::Backlog { listener, .. } if listener == id))
            .count();
        for _ in have..usize::from(backlog) {
            if !self.room(owner.key, cap, 1) {
                break;
            }
            let handle = self.new_socket();
            self.push(owner, group, None, Kind::Backlog { listener: id, handle, half_open_since: None });
            self.relisten(handle, port);
        }
    }

    /// `data` read: bytes into `out`, 0 at the peer's end, or [`Ready::Wait`] while there is
    /// nothing to read and more may come.
    pub fn recv(&mut self, owner: Owner, n: u32, out: &mut [u8]) -> Result<Ready<usize>, CtlError> {
        let i = self.entry(owner, n).ok_or(CtlError::State)?;
        let Kind::Conn(h) = self.table[i].kind else { return Err(CtlError::State) };
        let socket = self.sockets.get_mut::<tcp::Socket>(h);
        if socket.can_recv() {
            return Ok(Ready::Now(socket.recv_slice(out).unwrap_or(0)));
        }
        if socket.may_recv() || socket.state() == State::SynSent || socket.state() == State::SynReceived {
            return Ok(Ready::Wait);
        }
        Ok(Ready::Now(0))
    }

    /// `data` write: as much of `data` as the send buffer takes, or [`Ready::Wait`] while it is
    /// full. `state` once the socket can no longer send.
    pub fn send(&mut self, owner: Owner, n: u32, data: &[u8]) -> Result<Ready<usize>, CtlError> {
        let i = self.entry(owner, n).ok_or(CtlError::State)?;
        let Kind::Conn(h) = self.table[i].kind else { return Err(CtlError::State) };
        let socket = self.sockets.get_mut::<tcp::Socket>(h);
        if matches!(socket.state(), State::SynSent | State::SynReceived) {
            return Ok(Ready::Wait);
        }
        if !socket.may_send() {
            return Err(CtlError::State);
        }
        if data.is_empty() {
            return Ok(Ready::Now(0));
        }
        match socket.send_slice(data) {
            Ok(0) => Ok(Ready::Wait),
            Ok(k) => Ok(Ready::Now(k)),
            Err(_) => Err(CtlError::State),
        }
    }

    /// `remote`: the peer's address and port, once there is a peer.
    pub fn remote(&self, owner: Owner, n: u32) -> Result<Option<(u32, u16)>, CtlError> {
        let i = self.entry(owner, n).ok_or(CtlError::State)?;
        Ok(self.table[i].kind.handle().and_then(|h| {
            let remote = self.sockets.get::<tcp::Socket>(h).remote_endpoint()?;
            let IpAddress::Ipv4(a) = remote.addr;
            Some((u32_of(a), remote.port))
        }))
    }

    /// Whether a call parked on socket `n` for `what` would still wait: a parked call is served
    /// again only once it would not. A socket that is gone does not wait (the serving answers).
    pub fn would_wait(&self, owner: Owner, n: u32, what: WaitFor) -> bool {
        let Some(i) = self.entry(owner, n) else { return false };
        match (self.table[i].kind, what) {
            (Kind::Listener { .. }, WaitFor::Ctl) => self.accepted(self.table[i].id).is_none(),
            (Kind::Conn(h), what) => {
                let s = self.sockets.get::<tcp::Socket>(h);
                let connecting = matches!(s.state(), State::SynSent | State::SynReceived);
                match what {
                    WaitFor::Ctl => connecting,
                    WaitFor::Recv => !s.can_recv() && (s.may_recv() || connecting),
                    WaitFor::Send => connecting || (s.may_send() && !s.can_send()),
                }
            }
            _ => false,
        }
    }

    /// The table's invariants, for the tests and the fuzz targets: every smoltcp socket belongs
    /// to exactly one entry, every backlog to a listener that exists, no owner has one number
    /// twice, and no more sockets exist than every bucket at its cap.
    pub fn audit(&self) -> Result<(), &'static str> {
        let handles: Vec<SocketHandle> = self.table.iter().filter_map(|e| e.kind.handle()).collect();
        if handles.len() != self.sockets.iter().count() {
            return Err("a smoltcp socket with no entry, or an entry with no socket");
        }
        for (i, h) in handles.iter().enumerate() {
            if handles[..i].contains(h) {
                return Err("two entries share a smoltcp socket");
            }
        }
        for e in &self.table {
            if let Kind::Backlog { listener, .. } = e.kind {
                if !self.table.iter().any(|l| l.id == listener && matches!(l.kind, Kind::Listener { .. })) {
                    return Err("a backlog socket whose listener is gone");
                }
                if e.n.is_some() {
                    return Err("a backlog socket with a number");
                }
            }
            if let Some(n) = e.n {
                if self.table.iter().filter(|o| o.owner == e.owner && o.n == Some(n)).count() != 1 {
                    return Err("one owner has a number twice");
                }
                if e.linger_until.is_some() {
                    return Err("a released socket is still visible");
                }
            }
        }
        if self.live() > self.max_sockets {
            return Err("more sockets than every bucket at its cap");
        }
        Ok(())
    }

    // ---- the network ----

    /// A frame from `netd`: taken in through a fresh interface if it is a SYN to a port something
    /// listens on, through the main one otherwise, and dropped if its source is martian or the
    /// link has no interface yet. Returns whether any socket may have changed.
    pub fn ingress(&mut self, frame: &[u8], now: u64) -> bool {
        if self.iface.is_none() || !self.link.arrive(frame) {
            return false;
        }
        let changed = match classify(frame, &self.net) {
            Class::Martian | Class::NotTcp => {
                self.link.discard();
                false
            }
            Class::Syn { port } if self.listening_on(port) => match self.fresh(now) {
                Some(mut fresh) => {
                    fresh.poll_ingress_single(instant(now), &mut self.link, &mut self.sockets)
                        != smoltcp::iface::PollIngressSingleResult::None
                }
                None => {
                    self.link.discard();
                    false
                }
            },
            _ => match self.iface.as_mut() {
                Some(iface) => {
                    iface.poll_ingress_single(instant(now), &mut self.link, &mut self.sockets)
                        != smoltcp::iface::PollIngressSingleResult::None
                }
                None => false,
            },
        };
        // Anything the interface did not take (a frame it refused before reading) goes.
        self.link.discard();
        changed
    }

    fn listening_on(&self, port: u16) -> bool { self.table.iter().any(|e| e.listens_on(port)) }

    /// Egress and maintenance: sends what the sockets have to send, ends lingering and half-open
    /// sockets past their deadlines, listens again on backlog slots whose connection failed,
    /// and removes released sockets smoltcp has closed. Returns whether any socket may have
    /// changed.
    pub fn poll(&mut self, now: u64) -> bool {
        let mut changed = self.maintain(now);
        if let Some(iface) = self.iface.as_mut() {
            changed |= iface.poll(instant(now), &mut self.link, &mut self.sockets)
                == smoltcp::iface::PollResult::SocketStateChanged;
        }
        changed |= self.sweep();
        changed
    }

    fn maintain(&mut self, now: u64) -> bool {
        let mut changed = false;
        for i in 0..self.table.len() {
            let entry = self.table[i];
            if let Some(h) = entry.kind.handle() {
                if entry.linger_until.is_some_and(|d| d <= now) {
                    let socket = self.sockets.get_mut::<tcp::Socket>(h);
                    if socket.state() != State::Closed {
                        socket.abort();
                        changed = true;
                    }
                }
            }
            if let Kind::Backlog { listener, handle, half_open_since } = entry.kind {
                let state = self.sockets.get::<tcp::Socket>(handle).state();
                match (state, half_open_since) {
                    (State::SynReceived, None) => {
                        self.table[i].kind = Kind::Backlog { listener, handle, half_open_since: Some(now) };
                    }
                    (State::SynReceived, Some(since)) if now.saturating_sub(since) >= HALF_OPEN_US => {
                        let port = self.listener_port(listener);
                        self.table[i].kind = Kind::Backlog { listener, handle, half_open_since: None };
                        if let Some(port) = port {
                            self.relisten(handle, port);
                        }
                        changed = true;
                    }
                    (State::Closed | State::TimeWait, _) => {
                        // Its connection failed before it was accepted: listen again.
                        self.table[i].kind = Kind::Backlog { listener, handle, half_open_since: None };
                        if let Some(port) = self.listener_port(listener) {
                            self.relisten(handle, port);
                        }
                    }
                    (State::SynReceived, Some(_)) => {}
                    (_, Some(_)) => {
                        self.table[i].kind = Kind::Backlog { listener, handle, half_open_since: None };
                    }
                    _ => {}
                }
            }
        }
        changed
    }

    fn listener_port(&self, id: u64) -> Option<u16> {
        self.table.iter().find(|e| e.id == id).and_then(|e| match e.kind {
            Kind::Listener { port, .. } => Some(port),
            _ => None,
        })
    }

    /// Removes released sockets smoltcp has closed, and released listeners with no backlog left.
    fn sweep(&mut self) -> bool {
        let before = self.table.len();
        let mut i = 0;
        while i < self.table.len() {
            let e = self.table[i];
            let gone = e.linger_until.is_some()
                && match e.kind.handle() {
                    Some(h) => self.sockets.get::<tcp::Socket>(h).state() == State::Closed,
                    None => !self
                        .table
                        .iter()
                        .any(|b| matches!(b.kind, Kind::Backlog { listener, .. } if listener == e.id)),
                };
            if gone {
                if let Some(h) = e.kind.handle() {
                    self.sockets.remove(h);
                }
                self.table.swap_remove(i);
            } else {
                i += 1;
            }
        }
        self.table.len() != before
    }

    /// How long until the stack must be polled again (µs): smoltcp's next timer, or the nearest
    /// linger or half-open deadline. `None`: nothing is due.
    pub fn poll_delay(&mut self, now: u64) -> Option<u64> {
        let smoltcp = self.iface.as_mut().and_then(|iface| iface.poll_delay(instant(now), &self.sockets));
        let smoltcp = smoltcp.map(|d| d.total_micros());
        let deadlines = self.table.iter().filter_map(|e| {
            let linger = e.linger_until.filter(|_| e.kind.handle().is_some());
            let half = match e.kind {
                Kind::Backlog { half_open_since: Some(since), .. } => {
                    Some(since.saturating_add(HALF_OPEN_US))
                }
                _ => None,
            };
            linger.into_iter().chain(half).min()
        });
        let ours = deadlines.min().map(|d| d.saturating_sub(now));
        match (smoltcp, ours) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

fn status_of(state: State) -> Status {
    match state {
        State::SynSent | State::SynReceived | State::Listen => Status::Connecting,
        State::Established => Status::Established,
        State::FinWait1
        | State::FinWait2
        | State::Closing
        | State::TimeWait
        | State::CloseWait
        | State::LastAck => Status::Closing,
        State::Closed => Status::Closed,
    }
}

/// What an inbound frame is, as far as the choice of interface goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// IPv4 whose source is one of the box's own addresses (other than the gateway), or nowhere,
    /// whatever its protocol: dropped. It is spoofed, or the box talking to itself; and answering
    /// it would mean asking ARP for one of the box's own addresses.
    Martian,
    /// IPv4 that is not TCP, from anywhere: dropped. `ipd` serves only TCP, and the main
    /// interface would answer it with an ICMP "protocol unreachable" to its claimed source: a
    /// reflection for anyone who spoofs one, and a packet to the box's own addresses if the
    /// source were one (QA D3-code-review-5).
    NotTcp,
    /// A SYN without ACK to `ipd`'s own address.
    Syn { port: u16 },
    /// Anything else: the main interface's.
    Other,
}

/// Reads just enough of an inbound frame to choose: Ethernet, IPv4 without options checked by
/// smoltcp later, TCP's ports and flags. A frame that does not parse is `Other` (the main
/// interface refuses it properly).
///
/// **Martian sources.** IPv4 of any protocol from `ipd`'s own address, `127/8` or `0/8`
/// ([`martian_source`]), or from any other of the box's own addresses ([`SelfSet`]) except the
/// gateway, is dropped, before its protocol is looked at. The gateway is kept: on QEMU it is where
/// forwarded connections arrive from (answer 174). **Then every IPv4 packet that is not TCP is
/// dropped** ([`Class::NotTcp`]), so nothing but TCP ever reaches smoltcp from the wire.
pub fn classify(frame: &[u8], net: &Net) -> Class {
    let own = net.addr;
    let Ok(eth) = EthernetFrame::new_checked(frame) else { return Class::Other };
    if eth.ethertype() != EthernetProtocol::Ipv4 {
        return Class::Other;
    }
    let Ok(ip) = Ipv4Packet::new_checked(eth.payload()) else { return Class::Other };
    let src = u32_of(ip.src_addr());
    if martian_source(src, own) || (net.selfset.contains(src) && Some(src) != net.gateway) {
        return Class::Martian;
    }
    if ip.next_header() != IpProtocol::Tcp {
        return Class::NotTcp;
    }
    if u32_of(ip.dst_addr()) != own || ip.more_frags() || ip.frag_offset() != 0 {
        return Class::Other;
    }
    let Ok(tcp) = TcpPacket::new_checked(ip.payload()) else { return Class::Other };
    if tcp.syn() && !tcp.ack() && !tcp.rst() {
        return Class::Syn { port: tcp.dst_port() };
    }
    Class::Other
}
