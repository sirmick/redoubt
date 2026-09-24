//! `/net` as a 9P file server (NAMESPACES.md, What `ipd` serves in milestone 1): the tree, whose
//! sockets each connection sees, and what each file does. Everything a file holds is typed
//! (WIRE.md's encoding), never text.
//!
//! ```text
//! /              the connection's root; its node carries the connection's scope
//! /tcp/          lists only this connection's sockets
//! /tcp/clone     read at offset 0: a new socket N, as `n: u32`
//! /tcp/N/ctl     write: one `net_ctl` operation; read: `state: u32`, `n: u32` (waits)
//! /tcp/N/data    read: bytes (waits; 0 is the peer's end); write: bytes (waits)
//! /tcp/N/remote  read: `addr: bytes[4]`, `port: u16`
//! ```
//!
//! **Scopes.** A node carries the scope of the connection it was reached through: a root badge's
//! from `ipd`'s arguments, a granted connection's from its `grant`, and a connection minted by
//! `new_connection` its parent's (rooted at `""` or `"tcp"` only). Scopes are kept by an id that
//! is never reused, so a node that outlives its connection names no scope, which permits nothing.
//!
//! **Waiting.** A read or write that must wait says so ([`Read::Wait`], [`Write::Wait`]) and
//! records what it waits for ([`NetFs::take_wait`]), for the server to park the call on.

use alloc::string::String;
use alloc::vec::Vec;

use redoubt_rt::ipc::Caller;
use redoubt_rt::server::AdmitKey;
use redoubt_rt::server::ninep::{DMDIR, FileServer, FileStat, NineError, QTDIR, Qid, Read, Write, mode};
use redoubt_rt::wire::proto::net_ctl;

use crate::link::Netif;
use crate::scope::Scope;
use crate::stack::{CtlError, Entropy, Owner, Ready, Stack, WaitFor};

/// A scope's id: never reused.
pub type ScopeId = u32;

/// Where a node is in the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum At {
    Root,
    Tcp,
    Clone,
    Dir(u32),
    Ctl(u32),
    Data(u32),
    Remote(u32),
}

/// A node: where it is, and the scope of the connection it was reached through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub scope: ScopeId,
    pub at: At,
}

impl Node {
    pub fn qid(&self) -> Qid {
        let (kind, path) = match self.at {
            At::Root => (QTDIR, 1),
            At::Tcp => (QTDIR, 2),
            At::Clone => (0, 3),
            At::Dir(n) => (QTDIR, (u64::from(n) + 1) << 4),
            At::Ctl(n) => (0, (u64::from(n) + 1) << 4 | 1),
            At::Data(n) => (0, (u64::from(n) + 1) << 4 | 2),
            At::Remote(n) => (0, (u64::from(n) + 1) << 4 | 3),
        };
        Qid { kind, version: 0, path }
    }

    fn name(&self) -> String {
        let name = match self.at {
            At::Root => "/",
            At::Tcp => "tcp",
            At::Clone => "clone",
            At::Ctl(_) => "ctl",
            At::Data(_) => "data",
            At::Remote(_) => "remote",
            At::Dir(n) => return decimal(n),
        };
        String::from(name)
    }

    fn socket(&self) -> Option<u32> {
        match self.at {
            At::Dir(n) | At::Ctl(n) | At::Data(n) | At::Remote(n) => Some(n),
            _ => None,
        }
    }
}

fn decimal(n: u32) -> String {
    use core::fmt::Write as _;
    let mut s = String::new();
    let _ = write!(s, "{n}");
    s
}

/// A decimal socket number as a walk names it: no sign, no leading zero.
fn parse_number(name: &str) -> Option<u32> {
    if name.is_empty() || name.len() > 10 || !name.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if name.len() > 1 && name.starts_with('0') {
        return None;
    }
    name.parse().ok()
}

/// One connection `ipd` minted (`grant` or `new_connection`): its scope and its port-ownership
/// group.
#[derive(Clone, Copy, Debug)]
struct Minted {
    badge: u64,
    scope: ScopeId,
    group: u64,
}

/// The sockets one bucket may hold: the default, or a root badge's override (account 0 only).
#[derive(Clone, Debug, Default)]
pub struct SocketCaps {
    pub default: usize,
    pub overrides: Vec<(u64, usize)>,
}

impl SocketCaps {
    pub fn of(&self, caller: &Caller) -> usize {
        if caller.account == 0 {
            if let Some((_, cap)) = self.overrides.iter().find(|(badge, _)| *badge == caller.badge) {
                return *cap;
            }
        }
        self.default
    }
}

/// `/net`.
pub struct NetFs<N: Netif, E: Entropy> {
    pub stack: Stack<N, E>,
    scopes: Vec<(ScopeId, Scope)>,
    next_scope: ScopeId,
    /// The root badges `ipd`'s arguments give a scope, and the scope each has.
    roots: Vec<(u64, ScopeId)>,
    minted: Vec<Minted>,
    caps: SocketCaps,
    /// Set while the server mints a `grant`, so [`FileServer::minted`] starts a new port group.
    granting: bool,
    /// What the last request that waited waits for.
    wait: Option<(Owner, u32, WaitFor)>,
    /// The time, set by the server before each request (µs since boot).
    pub now: u64,
}

/// A `net_ctl` failure as the `Rerror` it is: the error's name.
fn ctl(e: CtlError) -> NineError { NineError(e.name()) }

fn owner(caller: &Caller) -> Owner { Owner { badge: caller.badge, key: AdmitKey::of(caller) } }

impl<N: Netif, E: Entropy> NetFs<N, E> {
    /// `/net` over `stack`, with each root badge's scope from the arguments.
    pub fn new(stack: Stack<N, E>, roots: &[(u64, Scope)], caps: SocketCaps) -> NetFs<N, E> {
        let mut fs = NetFs {
            stack,
            scopes: Vec::new(),
            next_scope: 1,
            roots: Vec::new(),
            minted: Vec::new(),
            caps,
            granting: false,
            wait: None,
            now: 0,
        };
        for (badge, scope) in roots {
            let id = fs.add_scope(scope.clone());
            fs.roots.push((*badge, id));
        }
        fs
    }

    /// Keeps `scope` under a new id.
    pub fn add_scope(&mut self, scope: Scope) -> ScopeId {
        let id = self.next_scope;
        self.next_scope += 1;
        self.scopes.push((id, scope));
        id
    }

    pub fn scope(&self, id: ScopeId) -> Option<&Scope> {
        self.scopes.iter().find(|(i, _)| *i == id).map(|(_, s)| s)
    }

    /// The scope `caller`'s connection holds: its root badge's, or its minted connection's.
    pub fn scope_of(&self, caller: &Caller) -> Option<(ScopeId, &Scope)> {
        let id = match self.roots.iter().find(|(b, _)| *b == caller.badge) {
            Some((_, id)) => *id,
            None => self.minted.iter().find(|m| m.badge == caller.badge)?.scope,
        };
        self.scope(id).map(|s| (id, s))
    }

    /// Drops scopes nothing refers to any more (a grant whose connection is gone, or whose
    /// minting failed). Root scopes are kept for ever.
    pub fn collect_scopes(&mut self) {
        let (roots, minted) = (&self.roots, &self.minted);
        self.scopes
            .retain(|(id, _)| roots.iter().any(|(_, r)| r == id) || minted.iter().any(|m| m.scope == *id));
    }

    /// The server is about to mint a `grant`: the next [`FileServer::minted`] starts a new port
    /// group (a grant never shares a listened port with the connection it came from).
    pub fn set_granting(&mut self, granting: bool) { self.granting = granting; }

    /// What the request just served waits for, if it asked to wait.
    pub fn take_wait(&mut self) -> Option<(Owner, u32, WaitFor)> { self.wait.take() }

    /// Connections minted and not yet gone.
    pub fn minted_connections(&self) -> usize { self.minted.len() }

    pub fn scopes_kept(&self) -> usize { self.scopes.len() }

    /// The port-ownership group of `caller`'s connection: a root badge's own, a grant's own, and
    /// for a connection minted by `new_connection` its parent's.
    fn group(&self, caller: &Caller) -> u64 {
        self.minted.iter().find(|m| m.badge == caller.badge).map_or(caller.badge, |m| m.group)
    }

    fn scope_of_node(&self, node: &Node) -> Scope { self.scope(node.scope).cloned().unwrap_or_default() }

    /// The socket a node names must be the caller's.
    fn check_socket(&self, caller: &Caller, node: &Node) -> Result<(), NineError> {
        match node.socket() {
            Some(n) if !self.stack.exists(owner(caller), n) => Err(NineError::NOT_FOUND),
            _ => Ok(()),
        }
    }

    fn ctl_write(&mut self, caller: &Caller, node: &Node, n: u32, data: &[u8]) -> Result<usize, NineError> {
        let op = net_ctl::Message::decode_file(data).map_err(|_| NineError::BAD_MESSAGE)?;
        let who = owner(caller);
        let now = self.now;
        match op {
            net_ctl::Message::Connect(c) => {
                let addr: [u8; 4] = c.addr.try_into().map_err(|_| NineError::BAD_MESSAGE)?;
                let scope = self.scope_of_node(node);
                self.stack.connect(who, n, &scope, u32::from_be_bytes(addr), c.port, now).map_err(ctl)?;
            }
            net_ctl::Message::Listen(l) => {
                let scope = self.scope_of_node(node);
                let cap = self.caps.of(caller);
                self.stack.listen(who, n, &scope, l.port, l.backlog, cap).map_err(ctl)?;
            }
            net_ctl::Message::Close(_) => self.stack.release(who, n, true, now).map_err(ctl)?,
            net_ctl::Message::Abort(_) => self.stack.release(who, n, false, now).map_err(ctl)?,
        }
        Ok(data.len())
    }
}

impl<N: Netif, E: Entropy> FileServer for NetFs<N, E> {
    type Node = Node;

    fn attach(&mut self, caller: &Caller, aname: &str) -> Result<(Node, Qid), NineError> {
        // Only a root badge with a scope attaches; the ingress badge has none, so no `/net`.
        let (_, scope) = self.roots.iter().find(|(b, _)| *b == caller.badge).ok_or(NineError::PERMISSION)?;
        if !aname.is_empty() {
            return Err(NineError::NOT_FOUND);
        }
        let node = Node { scope: *scope, at: At::Root };
        Ok((node, node.qid()))
    }

    fn minted(&mut self, caller: &Caller, badge: u64, root: &Node, _quota: u64) -> Result<(), NineError> {
        // `new_connection` may root a connection at `""` or `"tcp"`, never at a socket's
        // directory (a socket is not delegated in milestone 1).
        if !matches!(root.at, At::Root | At::Tcp) {
            return Err(NineError::PERMISSION);
        }
        let group = if self.granting { badge } else { self.group(caller) };
        self.minted.try_reserve(1).map_err(|_| NineError::NO_MEMORY)?;
        self.minted.push(Minted { badge, scope: root.scope, group });
        Ok(())
    }

    fn disconnected(&mut self, badge: u64) {
        self.minted.retain(|m| m.badge != badge);
        self.stack.disconnect(badge, self.now);
        self.collect_scopes();
    }

    fn labels(&self, _node: &Node) -> &[u64] { &[] }

    fn walk(&mut self, caller: &Caller, dir: &Node, name: &str) -> Result<(Node, Qid), NineError> {
        let at = match (dir.at, name) {
            (At::Root, "tcp") => At::Tcp,
            (At::Tcp, "clone") => At::Clone,
            (At::Tcp, n) => {
                let n = parse_number(n).ok_or(NineError::NOT_FOUND)?;
                At::Dir(n)
            }
            (At::Dir(n), "ctl") => At::Ctl(n),
            (At::Dir(n), "data") => At::Data(n),
            (At::Dir(n), "remote") => At::Remote(n),
            _ => return Err(NineError::NOT_FOUND),
        };
        let node = Node { scope: dir.scope, at };
        // Another connection's socket is not there at all.
        self.check_socket(caller, &node)?;
        Ok((node, node.qid()))
    }

    fn open(&mut self, caller: &Caller, node: &Node, m: u8) -> Result<Qid, NineError> {
        self.check_socket(caller, node)?;
        let writes = matches!(m & 3, mode::OWRITE | mode::ORDWR) || m & mode::OTRUNC != 0;
        if writes && !matches!(node.at, At::Ctl(_) | At::Data(_)) {
            return Err(NineError::PERMISSION);
        }
        Ok(node.qid())
    }

    fn read(&mut self, caller: &Caller, node: &Node, offset: u64, out: &mut [u8]) -> Result<Read, NineError> {
        self.check_socket(caller, node)?;
        let who = owner(caller);
        match node.at {
            At::Clone => {
                if offset != 0 {
                    return Ok(Read::Done(0));
                }
                let out = out.get_mut(..4).ok_or(NineError::TOO_SMALL)?;
                let (group, cap) = (self.group(caller), self.caps.of(caller));
                let n = self.stack.allocate(who, group, cap).map_err(ctl)?;
                out.copy_from_slice(&n.to_le_bytes());
                Ok(Read::Done(4))
            }
            // The offset is not looked at: each read is the socket's state now (or the next
            // accepted connection), never a position in a file.
            At::Ctl(n) => {
                let out = out.get_mut(..8).ok_or(NineError::TOO_SMALL)?;
                match self.stack.status(who, n, self.caps.of(caller)).map_err(ctl)? {
                    Ready::Now((status, m)) => {
                        out[..4].copy_from_slice(&(status as u32).to_le_bytes());
                        out[4..].copy_from_slice(&m.to_le_bytes());
                        Ok(Read::Done(8))
                    }
                    Ready::Wait => {
                        self.wait = Some((who, n, WaitFor::Ctl));
                        Ok(Read::Wait)
                    }
                }
            }
            At::Data(n) => match self.stack.recv(who, n, out).map_err(ctl)? {
                Ready::Now(k) => Ok(Read::Done(k)),
                Ready::Wait => {
                    self.wait = Some((who, n, WaitFor::Recv));
                    Ok(Read::Wait)
                }
            },
            At::Remote(n) => {
                if offset != 0 {
                    return Ok(Read::Done(0));
                }
                let Some((addr, port)) = self.stack.remote(who, n).map_err(ctl)? else {
                    return Ok(Read::Done(0));
                };
                let out = out.get_mut(..6).ok_or(NineError::TOO_SMALL)?;
                out[..4].copy_from_slice(&addr.to_be_bytes());
                out[4..].copy_from_slice(&port.to_le_bytes());
                Ok(Read::Done(6))
            }
            At::Root | At::Tcp | At::Dir(_) => Err(NineError::NOT_SUPPORTED),
        }
    }

    fn write(&mut self, caller: &Caller, node: &Node, offset: u64, data: &[u8]) -> Result<usize, NineError> {
        match self.write_or_wait(caller, node, offset, data)? {
            Write::Done(n) => Ok(n),
            // Only `serve_parking` may hold a write; anything else is told to come back.
            Write::Wait => {
                self.wait = None;
                Err(ctl(CtlError::Timeout))
            }
        }
    }

    fn write_or_wait(
        &mut self,
        caller: &Caller,
        node: &Node,
        _offset: u64,
        data: &[u8],
    ) -> Result<Write, NineError> {
        self.check_socket(caller, node)?;
        match node.at {
            At::Ctl(n) => self.ctl_write(caller, node, n, data).map(Write::Done),
            At::Data(n) => {
                let who = owner(caller);
                match self.stack.send(who, n, data).map_err(ctl)? {
                    Ready::Now(k) => Ok(Write::Done(k)),
                    Ready::Wait => {
                        self.wait = Some((who, n, WaitFor::Send));
                        Ok(Write::Wait)
                    }
                }
            }
            _ => Err(NineError::PERMISSION),
        }
    }

    fn stat(&mut self, caller: &Caller, node: &Node) -> Result<FileStat, NineError> {
        self.check_socket(caller, node)?;
        let dir = node.qid().kind & QTDIR != 0;
        let mode = if dir { DMDIR | 0o555 } else { 0o666 };
        Ok(FileStat { qid: node.qid(), mode, mtime: 0, length: 0, name: node.name() })
    }

    fn dir_entry(
        &mut self,
        caller: &Caller,
        dir: &Node,
        index: u64,
    ) -> Result<Option<(Node, FileStat)>, NineError> {
        let at = match dir.at {
            At::Root if index == 0 => At::Tcp,
            At::Tcp if index == 0 => At::Clone,
            At::Tcp => {
                let numbers = self.stack.numbers(owner(caller));
                match usize::try_from(index - 1).ok().and_then(|i| numbers.get(i)) {
                    Some(n) => At::Dir(*n),
                    None => return Ok(None),
                }
            }
            At::Dir(n) => match index {
                0 => At::Ctl(n),
                1 => At::Data(n),
                2 => At::Remote(n),
                _ => return Ok(None),
            },
            _ => return Ok(None),
        };
        let node = Node { scope: dir.scope, at };
        let stat = self.stat(caller, &node)?;
        Ok(Some((node, stat)))
    }
}
