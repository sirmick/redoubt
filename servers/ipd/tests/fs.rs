//! `/net` over 9P (NAMESPACES.md, What `ipd` serves in milestone 1), through the real skeleton
//! (`NineServer::answer_in_place` through `World::answer`, which meters sockets as the program
//! does; no system calls) over the stack and the peer of tests/common:
//! the tree, per-connection sockets, `ctl` operations and their refusals, waiting reads and
//! writes, `new_connection`, `grant` and `disconnect`.

mod common;

use std::num::NonZeroU64;

use common::*;
use redoubt_ipd::scope::{Ports, Prefix, Rule, Scope, ip};
use redoubt_ipd::server::answer_grant;
use redoubt_rt::abi::{Error, Handle, Handles};
use redoubt_rt::ipc::Caller;
use redoubt_rt::server::minted::Minter;
use redoubt_rt::server::ninep::{Answer, mode};
use redoubt_rt::wire::MSIZE;
use redoubt_rt::wire::ninep::{Body, Message, NOFID, Names, stats};
use redoubt_rt::wire::proto::{ipd, net_ctl, ninep_common};

/// The kernel's part in minting, for host tests.
struct FakeKernel {
    minted: Vec<u64>,
    rng: u64,
}

impl Minter for FakeKernel {
    fn mint(&mut self, badge: NonZeroU64) -> Result<Handle, Error> {
        self.minted.push(badge.get());
        Ok(Handle::new(99 + self.minted.len() as u32).unwrap())
    }

    fn random(&mut self) -> Result<u64, Error> {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        Ok(self.rng)
    }
}

fn kernel() -> FakeKernel { FakeKernel { minted: Vec::new(), rng: 77 } }

/// What a 9P request came back as.
#[derive(Debug, PartialEq, Eq)]
enum R {
    Ok,
    Data(Vec<u8>),
    Count(u32),
    Names(Vec<String>),
    Err(String),
    /// The file server asked to hold it: the T-message is still in the lend.
    Waiting,
}

fn rpc(w: &mut World, who: &Caller, body: Body<'_>) -> R {
    let mut buf = vec![0u8; MSIZE];
    Message { tag: 9, body }.encode(&mut buf).unwrap();
    match w.answer(who, &mut buf) {
        Answer::Waiting => return R::Waiting,
        Answer::NoRoom => panic!("no room"),
        Answer::Replied => {}
    }
    let reply = Message::decode(&buf).unwrap();
    assert_eq!(reply.tag, 9);
    match reply.body {
        Body::Rerror { ename } => R::Err(ename.into()),
        Body::Rread { data } => R::Data(data.to_vec()),
        Body::Rwrite { count } => R::Count(count),
        _ => R::Ok,
    }
}

fn attach(w: &mut World, who: &Caller, fid: u32) -> R {
    rpc(w, who, Body::Tattach { fid, afid: NOFID, uname: "", aname: "" })
}

fn walk(w: &mut World, who: &Caller, fid: u32, newfid: u32, names: &[&str]) -> R {
    rpc(w, who, Body::Twalk { fid, newfid, wnames: Names::new(names).unwrap() })
}

fn open(w: &mut World, who: &Caller, fid: u32, m: u8) -> R { rpc(w, who, Body::Topen { fid, mode: m }) }

fn read(w: &mut World, who: &Caller, fid: u32, offset: u64) -> R {
    rpc(w, who, Body::Tread { fid, offset, count: 4096 })
}

fn write(w: &mut World, who: &Caller, fid: u32, data: &[u8]) -> R {
    rpc(w, who, Body::Twrite { fid, offset: 0, data })
}

fn clunk(w: &mut World, who: &Caller, fid: u32) -> R { rpc(w, who, Body::Tclunk { fid }) }

/// A directory's entries' names.
fn list(w: &mut World, who: &Caller, fid: u32) -> R {
    match read(w, who, fid, 0) {
        R::Data(bytes) => R::Names(stats(&bytes).map(|s| s.unwrap().name.to_string()).collect()),
        other => other,
    }
}

/// One `net_ctl` operation, as the bytes of a `ctl` write.
fn op(message: net_ctl::Message<'_>) -> Vec<u8> {
    let mut out = vec![0u8; 64];
    let n = message.encode_file(&mut out).unwrap();
    out.truncate(n);
    out
}

fn connect_op(addr: u32, port: u16) -> Vec<u8> {
    op(net_ctl::Message::Connect(net_ctl::Connect { addr: &addr.to_be_bytes(), port }))
}

fn listen_op(port: u16, backlog: u8) -> Vec<u8> {
    op(net_ctl::Message::Listen(net_ctl::Listen { port, backlog }))
}

/// Attaches `who` at fid 0 and makes a socket: fid 1 is `/tcp`, and the socket's number.
fn socket(w: &mut World, who: &Caller) -> u32 {
    match walk(w, who, 0, 1, &["tcp"]) {
        R::Ok => {}
        R::Err(e) if e == "fid already in use" => {}
        _ => {
            assert_eq!(attach(w, who, 0), R::Ok);
            assert_eq!(walk(w, who, 0, 1, &["tcp"]), R::Ok);
        }
    }
    let fid = 1000 + w.nine.fids(who) as u32;
    assert_eq!(walk(w, who, 1, fid, &["clone"]), R::Ok);
    assert_eq!(open(w, who, fid, mode::OREAD), R::Ok);
    let R::Data(bytes) = read(w, who, fid, 0) else { panic!("clone read") };
    assert_eq!(clunk(w, who, fid), R::Ok);
    u32::from_le_bytes(bytes.try_into().unwrap())
}

/// Opens socket `n`'s `file` at `fid`.
fn open_file(w: &mut World, who: &Caller, n: u32, file: &str, fid: u32, m: u8) {
    let name = n.to_string();
    assert_eq!(walk(w, who, 1, fid, &[&name, file]), R::Ok, "walk tcp/{n}/{file}");
    assert_eq!(open(w, who, fid, m), R::Ok);
}

fn ctl_status(w: &mut World, who: &Caller, fid: u32) -> R {
    match read(w, who, fid, 0) {
        R::Data(bytes) => {
            let state = u32::from_le_bytes(bytes[..4].try_into().unwrap());
            let n = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
            R::Data(vec![state as u8, n as u8])
        }
        other => other,
    }
}

#[test]
fn only_a_root_badge_with_a_scope_attaches() {
    let mut w = World::new(64);
    assert_eq!(attach(&mut w, &caller(ANY, 0, &[]), 0), R::Ok);
    assert_eq!(attach(&mut w, &caller(INGRESS, 0, &[]), 0), R::Err("permission denied".into()));
    assert_eq!(attach(&mut w, &caller(12345, 0, &[]), 0), R::Err("permission denied".into()));
    // A minted badge ipd never made is no connection at all.
    assert_eq!(attach(&mut w, &caller(1 << 63 | 7, 0, &[]), 0), R::Err("no such connection".into()));
}

#[test]
fn clone_makes_a_socket_and_tcp_lists_only_the_callers() {
    let mut w = World::new(64);
    let (a, b) = (caller(ANY, 1, &[]), caller(ANY, 2, &[]));
    assert_eq!(socket(&mut w, &a), 0);
    assert_eq!(socket(&mut w, &a), 1);
    assert_eq!(socket(&mut w, &b), 0);
    assert_eq!(walk(&mut w, &a, 1, 50, &[]), R::Ok);
    assert_eq!(open(&mut w, &a, 50, mode::OREAD), R::Ok);
    assert_eq!(list(&mut w, &a, 50), R::Names(vec!["clone".into(), "0".into(), "1".into()]));
    assert_eq!(walk(&mut w, &b, 1, 50, &[]), R::Ok);
    assert_eq!(open(&mut w, &b, 50, mode::OREAD), R::Ok);
    assert_eq!(list(&mut w, &b, 50), R::Names(vec!["clone".into(), "0".into()]));
    // Another connection's socket is not there at all.
    assert_eq!(walk(&mut w, &b, 1, 51, &["1"]), R::Err("file does not exist".into()));
    assert_eq!(walk(&mut w, &b, 1, 51, &["1", "ctl"]), R::Err("file does not exist".into()));
    assert_eq!(walk(&mut w, &b, 1, 51, &["01"]), R::Err("file does not exist".into()));
    // A socket's directory holds ctl, data, remote; remote and clone are read-only.
    assert_eq!(walk(&mut w, &a, 1, 52, &["0"]), R::Ok);
    assert_eq!(open(&mut w, &a, 52, mode::OREAD), R::Ok);
    assert_eq!(list(&mut w, &a, 52), R::Names(vec!["ctl".into(), "data".into(), "remote".into()]));
    assert_eq!(walk(&mut w, &a, 1, 53, &["0", "remote"]), R::Ok);
    assert_eq!(open(&mut w, &a, 53, mode::OWRITE), R::Err("permission denied".into()));
    // A clone read past offset 0 makes nothing.
    assert_eq!(walk(&mut w, &a, 1, 54, &["clone"]), R::Ok);
    assert_eq!(open(&mut w, &a, 54, mode::OREAD), R::Ok);
    assert_eq!(read(&mut w, &a, 54, 4), R::Data(vec![]));
    assert_eq!(w.nine.fs.stack.numbers(owner(&a)), vec![0, 1]);
}

#[test]
fn a_connect_waits_then_carries_data_through_the_files() {
    let mut w = World::new(64);
    let peer = w.peer.listen(7);
    let a = caller(ANY, 1, &[]);
    let n = socket(&mut w, &a);
    open_file(&mut w, &a, n, "ctl", 10, mode::ORDWR);
    open_file(&mut w, &a, n, "data", 11, mode::ORDWR);
    open_file(&mut w, &a, n, "remote", 12, mode::OREAD);
    assert_eq!(read(&mut w, &a, 12, 0), R::Data(vec![]), "no peer yet");
    let c = connect_op(LAN_HOST, 7);
    assert_eq!(write(&mut w, &a, 10, &c), R::Count(c.len() as u32));
    assert_eq!(ctl_status(&mut w, &a, 10), R::Waiting, "a ctl read waits while connecting");
    w.pump();
    assert_eq!(ctl_status(&mut w, &a, 10), R::Data(vec![2, n as u8]), "established");
    let mut remote = LAN_HOST.to_be_bytes().to_vec();
    remote.extend_from_slice(&7u16.to_le_bytes());
    assert_eq!(read(&mut w, &a, 12, 0), R::Data(remote));
    assert_eq!(read(&mut w, &a, 11, 0), R::Waiting, "nothing to read yet");
    assert_eq!(write(&mut w, &a, 11, b"ping"), R::Count(4));
    w.pump();
    let s = w.peer.socket(peer);
    let mut buf = [0u8; 16];
    let k = s.recv_slice(&mut buf).unwrap();
    s.send_slice(&buf[..k]).unwrap();
    w.pump();
    assert_eq!(read(&mut w, &a, 11, 0), R::Data(b"ping".to_vec()));
    // Close: the socket is gone from the caller's tree at once.
    assert_eq!(write(&mut w, &a, 10, &op(net_ctl::Message::Close(net_ctl::Close {}))), R::Count(4));
    assert_eq!(read(&mut w, &a, 11, 0), R::Err("file does not exist".into()));
}

#[test]
fn ctl_refusals_are_named_and_checked_before_the_stack() {
    let mut w = World::new(64);
    let narrow = caller(NARROW, 1, &[]);
    let n = socket(&mut w, &narrow);
    open_file(&mut w, &narrow, n, "ctl", 10, mode::ORDWR);
    let refused = [
        (connect_op(ip(10, 1, 9, 101), 7), "not_permitted"),
        (connect_op(ip(10, 1, 9, 110), 8), "not_permitted"),
        (connect_op(ADDR, 7), "not_permitted"),
        (connect_op(ip(127, 0, 0, 1), 7), "not_permitted"),
        (listen_op(22, 1), "not_permitted"),
        (op(net_ctl::Message::Connect(net_ctl::Connect { addr: &[10, 1, 9], port: 7 })), "malformed message"),
        (vec![1, 0, 0], "malformed message"),
        (vec![9, 0, 0, 0], "malformed message"),
    ];
    for (bytes, name) in refused {
        assert_eq!(write(&mut w, &narrow, 10, &bytes), R::Err(name.into()), "{bytes:?}");
    }
    // Even an anywhere scope never reaches the box's own addresses.
    let a = caller(ANY, 1, &[]);
    let m = socket(&mut w, &a);
    open_file(&mut w, &a, m, "ctl", 20, mode::ORDWR);
    for addr in [ADDR, SELF_EXTRA, ip(10, 1, 255, 255), ip(224, 0, 0, 1)] {
        assert_eq!(write(&mut w, &a, 20, &connect_op(addr, 22)), R::Err("not_permitted".into()));
    }
    // Backlog out of range, and a second connect on the same socket.
    assert_eq!(write(&mut w, &a, 20, &listen_op(9000, 0)), R::Err("state".into()));
    assert_eq!(write(&mut w, &a, 20, &listen_op(9000, 9)), R::Err("state".into()));
    let _l = w.peer.listen(7);
    let _ok = write(&mut w, &narrow, 10, &connect_op(ip(10, 1, 9, 110), 7));
    assert_eq!(write(&mut w, &narrow, 10, &connect_op(ip(10, 1, 9, 110), 7)), R::Err("state".into()));
    w.run_for(1_000_000, 100_000);
    assert_eq!(w.peer.syns, vec![(ip(10, 1, 9, 110), 7)], "only the permitted SYN went out");
}

#[test]
fn a_listener_accepts_through_ctl() {
    let mut w = World::new(64);
    let sshd = caller(LISTENER, 0, &[]);
    let n = socket(&mut w, &sshd);
    open_file(&mut w, &sshd, n, "ctl", 10, mode::ORDWR);
    assert_eq!(write(&mut w, &sshd, 10, &listen_op(22, 2)), R::Count(7));
    assert_eq!(ctl_status(&mut w, &sshd, 10), R::Waiting);
    let peer = w.peer.connect(GATEWAY, 40000, 22);
    w.run_for(100_000, 10_000);
    let R::Data(status) = ctl_status(&mut w, &sshd, 10) else { panic!("no accept") };
    assert_eq!(status[0], 5);
    let m = u32::from(status[1]);
    open_file(&mut w, &sshd, m, "data", 11, mode::ORDWR);
    w.peer.socket(peer).send_slice(b"SSH-2.0-test\r\n").unwrap();
    w.pump();
    assert_eq!(read(&mut w, &sshd, 11, 0), R::Data(b"SSH-2.0-test\r\n".to_vec()));
}

/// Sockets per bucket: the default 8, or a root badge's override, for account 0 only.
#[test]
fn clone_stops_at_the_buckets_socket_cap() {
    let mut w = World::new(64);
    // One badge alone in its account's bucket gets half of it (answer 90): 6 of its 12 `State`
    // units, sockets being paid in them (QA D3-code-review-5).
    let a = caller(ANY, 1, &[]);
    for _ in 0..6 {
        socket(&mut w, &a);
    }
    assert_eq!(walk(&mut w, &a, 1, 60, &["clone"]), R::Ok);
    assert_eq!(open(&mut w, &a, 60, mode::OREAD), R::Ok);
    assert_eq!(read(&mut w, &a, 60, 0), R::Err("too_many".into()));
    // The listener badge has 20, as account 0 (a bucket of its own, with no shares), and then
    // its socket cap stops it.
    let sshd = caller(LISTENER, 0, &[]);
    for _ in 0..20 {
        socket(&mut w, &sshd);
    }
    assert_eq!(walk(&mut w, &sshd, 1, 60, &["clone"]), R::Ok);
    assert_eq!(open(&mut w, &sshd, 60, mode::OREAD), R::Ok);
    assert_eq!(read(&mut w, &sshd, 60, 0), R::Err("too_many".into()));
    // With any other account it is the default, halved for a lone badge.
    let not_sshd = caller(LISTENER, 5, &[]);
    for _ in 0..6 {
        socket(&mut w, &not_sshd);
    }
    assert_eq!(walk(&mut w, &not_sshd, 1, 60, &["clone"]), R::Ok);
    assert_eq!(open(&mut w, &not_sshd, 60, mode::OREAD), R::Ok);
    assert_eq!(read(&mut w, &not_sshd, 60, 0), R::Err("too_many".into()));
}

fn new_connection(w: &mut World, who: &Caller, root: &str, k: &mut FakeKernel) -> Result<(u64, u64), u32> {
    let mut lend = vec![0u8; 256];
    let words = ninep_common::Message::NewConnection(ninep_common::NewConnection { root, quota: 0 })
        .encode(&mut lend)
        .unwrap();
    let outcome = w.nine.answer_common(who, &words, &Handles::new(), &mut lend, k);
    match ninep_common::Reply::decode(2, &outcome.words, &lend, outcome.send.as_slice().len()).unwrap() {
        Ok(ninep_common::Reply::NewConnection(r)) => Ok((r.id, *k.minted.last().unwrap())),
        Ok(_) => panic!(),
        Err(code) => Err(code.code()),
    }
}

fn disconnect(w: &mut World, who: &Caller, id: u64, k: &mut FakeKernel) -> bool {
    let words = ninep_common::Message::Disconnect(ninep_common::Disconnect { id }).encode(&mut []).unwrap();
    let outcome = w.nine.answer_common(who, &words, &Handles::new(), &mut [], k);
    matches!(ninep_common::Reply::decode(3, &outcome.words, &[], 0), Ok(Ok(_)))
}

fn grant(w: &mut World, who: &Caller, scope: &Scope, k: &mut FakeKernel) -> Result<(u64, u64), u32> {
    let bytes = scope.encode();
    let mut lend = vec![0u8; 512];
    let words = ipd::Message::Grant(ipd::Grant { scope: &bytes }).encode(&mut lend).unwrap();
    let (outcome, minted) = answer_grant(&mut w.nine, who, &words, &Handles::new(), &mut lend, k);
    match ipd::Reply::decode(16, &outcome.words, &lend, outcome.send.as_slice().len()).unwrap() {
        Ok(ipd::Reply::Grant(r)) => Ok((r.id, minted.expect("a minted badge"))),
        Ok(_) => panic!(),
        Err(code) => Err(code.code()),
    }
}

fn grant_raw(w: &mut World, who: &Caller, bytes: &[u8], k: &mut FakeKernel) -> Result<(u64, u64), u32> {
    let mut lend = vec![0u8; 512];
    let words = ipd::Message::Grant(ipd::Grant { scope: bytes }).encode(&mut lend).unwrap();
    let (outcome, minted) = answer_grant(&mut w.nine, who, &words, &Handles::new(), &mut lend, k);
    match ipd::Reply::decode(16, &outcome.words, &lend, outcome.send.as_slice().len()).unwrap() {
        Ok(ipd::Reply::Grant(r)) => Ok((r.id, minted.unwrap())),
        Ok(_) => panic!(),
        Err(code) => Err(code.code()),
    }
}

/// `new_connection` keeps the caller's scope, and is rooted at `""` or `"tcp"` only.
#[test]
fn new_connection_keeps_the_scope() {
    let mut w = World::new(64);
    let mut k = kernel();
    let narrow = caller(NARROW, 1, &[]);
    assert_eq!(attach(&mut w, &narrow, 0), R::Ok);
    let (_, badge) = new_connection(&mut w, &narrow, "", &mut k).unwrap();
    assert!(new_connection(&mut w, &narrow, "tcp", &mut k).is_ok());
    socket(&mut w, &narrow);
    assert!(new_connection(&mut w, &narrow, "tcp/0", &mut k).is_err(), "a socket's directory was delegated");
    // The child reaches exactly what its parent does.
    let child = caller(badge, 1, &[]);
    let n = socket(&mut w, &child);
    open_file(&mut w, &child, n, "ctl", 10, mode::ORDWR);
    assert_eq!(write(&mut w, &child, 10, &connect_op(ip(10, 1, 9, 101), 7)), R::Err("not_permitted".into()));
    assert_eq!(write(&mut w, &child, 10, &connect_op(ip(10, 1, 9, 110), 7)), R::Count(14));
}

/// `grant`: canonical and no wider, or `not_permitted`; the granted connection reaches exactly
/// its scope; `disconnect` frees it, its sockets and its scope.
#[test]
fn a_grant_narrows_and_disconnect_frees_everything() {
    let mut w = World::new(64);
    let mut k = kernel();
    let root = caller(ANY, 0, &[]);
    let kept = w.nine.fs.scopes_kept();
    let wide =
        Scope::new(&[Rule::Connect(Prefix::new(0, 0).unwrap(), Ports::new(1, 65535).unwrap())]).unwrap();
    let narrow = connect_scope(ip(10, 1, 9, 0), 24, 7, 7);
    // From the narrow root, a wider scope is refused; a non-canonical one too.
    let narrow_root = caller(NARROW, 0, &[]);
    assert_eq!(grant(&mut w, &narrow_root, &wide, &mut k), Err(2));
    assert_eq!(grant(&mut w, &narrow_root, &listen_scope(7, 7), &mut k), Err(2), "listen added");
    assert_eq!(
        grant_raw(&mut w, &root, &[1, 1, 10, 1, 9, 1, 24, 7, 0, 7, 0], &mut k),
        Err(2),
        "host bits set"
    );
    assert_eq!(grant_raw(&mut w, &root, &[1, 1, 10, 1, 9, 0, 24, 8, 0, 7, 0], &mut k), Err(2), "lo > hi");
    assert_eq!(w.nine.fs.scopes_kept(), kept, "refused grants keep nothing");
    // A narrower one is granted.
    let (id, badge) = grant(&mut w, &root, &narrow, &mut k).unwrap();
    assert_eq!(w.nine.fs.minted_connections(), 1);
    let granted = caller(badge, 0, &[]);
    let n = socket(&mut w, &granted);
    open_file(&mut w, &granted, n, "ctl", 10, mode::ORDWR);
    assert_eq!(write(&mut w, &granted, 10, &connect_op(ip(10, 1, 8, 1), 7)), R::Err("not_permitted".into()));
    assert_eq!(
        write(&mut w, &granted, 10, &connect_op(ip(10, 1, 9, 100), 8)),
        R::Err("not_permitted".into())
    );
    let _l = w.peer.listen(7);
    assert_eq!(write(&mut w, &granted, 10, &connect_op(LAN_HOST, 7)), R::Count(14));
    w.pump();
    // A grant from the grant narrows again; one as wide as the grant is fine.
    assert!(grant(&mut w, &granted, &narrow, &mut k).is_ok());
    assert_eq!(grant(&mut w, &granted, &wide, &mut k), Err(2));
    // Only the id's holder may name it; the holder's disconnect frees everything under it.
    assert!(!disconnect(&mut w, &granted, id, &mut k));
    assert!(disconnect(&mut w, &root, id, &mut k));
    assert_eq!(w.nine.fs.minted_connections(), 0);
    assert_eq!(w.nine.fs.scopes_kept(), kept, "the grants' scopes were kept");
    assert!(w.nine.fs.stack.numbers(owner(&granted)).is_empty());
    w.pump();
    assert_eq!(w.nine.fs.stack.charged(owner(&granted).key), 0);
    assert_eq!(attach(&mut w, &granted, 0), R::Err("no such connection".into()));
}

/// A listened port is its group's: another grant from the same root is `in_use`, a connection
/// the listener minted with `new_connection` is not.
#[test]
fn a_port_is_shared_with_new_connection_children_but_not_grants() {
    let mut w = World::new(64);
    let mut k = kernel();
    let sshd = caller(LISTENER, 0, &[]);
    let n = socket(&mut w, &sshd);
    open_file(&mut w, &sshd, n, "ctl", 10, mode::ORDWR);
    assert_eq!(write(&mut w, &sshd, 10, &listen_op(22, 1)), R::Count(7));
    let (_, granted) = grant(&mut w, &sshd, &listen_scope(22, 22), &mut k).unwrap();
    let g = caller(granted, 0, &[]);
    let m = socket(&mut w, &g);
    open_file(&mut w, &g, m, "ctl", 10, mode::ORDWR);
    assert_eq!(write(&mut w, &g, 10, &listen_op(22, 1)), R::Err("in_use".into()));
    let (_, child) = new_connection(&mut w, &sshd, "", &mut k).unwrap();
    let c = caller(child, 0, &[]);
    let j = socket(&mut w, &c);
    open_file(&mut w, &c, j, "ctl", 10, mode::ORDWR);
    assert_eq!(write(&mut w, &c, 10, &listen_op(22, 1)), R::Count(7));
}

/// How many sockets `who` can make before `clone` answers `too_many`.
fn sockets_until_refused(w: &mut World, who: &Caller) -> usize {
    if walk(w, who, 0, 1, &["tcp"]) != R::Ok && attach(w, who, 0) == R::Ok {
        assert_eq!(walk(w, who, 0, 1, &["tcp"]), R::Ok);
    }
    let mut made = 0;
    loop {
        assert_eq!(walk(w, who, 1, 900, &["clone"]), R::Ok);
        assert_eq!(open(w, who, 900, mode::OREAD), R::Ok);
        let got = read(w, who, 900, 0);
        assert_eq!(clunk(w, who, 900), R::Ok);
        match got {
            R::Data(_) => made += 1,
            R::Err(e) if e == "too_many" => return made,
            other => panic!("clone: {other:?}"),
        }
    }
}

/// Sockets are paid in the shared admission, so they have fair shares (answer 90; QA
/// D3-code-review-5, P2-2): an agent sharing its sponsor's account takes at most half of the
/// bucket's 12 units alone, and its sponsor, arriving second, still gets its own third.
#[test]
fn an_agent_cannot_take_all_its_sponsors_sockets() {
    use redoubt_rt::server::{AdmitKey, Resource};
    let mut w = World::new(64);
    let agent = caller(ANY, 7, &[]);
    let sponsor = caller(NARROW, 7, &[]);
    assert_eq!(sockets_until_refused(&mut w, &agent), 6, "a lone badge takes half the bucket");
    assert_eq!(sockets_until_refused(&mut w, &sponsor), 4, "the sponsor gets its share after the agent's");
    let key = AdmitKey::of(&agent);
    assert_eq!(w.nine.admission().held(key, Resource::State), 10);
    assert_eq!(w.nine.admission().held_by(key, ANY, Resource::State), 6);
    assert_eq!(w.nine.fs.stack.charged(key), 10);
}

/// A socket keeps its unit, and so its bucket, while it lingers after its owner let go of
/// everything else (QA D3-code-review-5, P2-2): the bucket cannot be given to a newcomer while
/// the socket still counts. The unit goes back once the stack removes the socket.
#[test]
fn a_lingering_socket_keeps_its_bucket() {
    use redoubt_rt::server::{AdmitKey, Resource};
    let mut w = World::new(64);
    let _peer = w.peer.listen(9);
    let who = caller(ANY, 8, &[]);
    let key = AdmitKey::of(&who);
    let n = socket(&mut w, &who);
    open_file(&mut w, &who, n, "ctl", 10, mode::ORDWR);
    assert_eq!(write(&mut w, &who, 10, &connect_op(LAN_HOST, 9)), R::Count(14));
    w.run_for(200_000, 50_000);
    assert!(matches!(read(&mut w, &who, 10, 0), R::Data(d) if d[0] == 2), "established");
    // The peer goes quiet, and then the owner closes: the FIN is never answered, and the socket
    // lingers, sending it again.
    w.wire.borrow_mut().blackhole.push(9);
    let close = op(net_ctl::Message::Close(net_ctl::Close {}));
    assert_eq!(write(&mut w, &who, 10, &close), R::Count(close.len() as u32));
    for fid in [10, 1, 0] {
        assert_eq!(clunk(&mut w, &who, fid), R::Ok);
    }
    w.run_for(1_000_000, 100_000);
    assert_eq!(w.nine.fids(&who), 0, "every fid is gone");
    assert_eq!(w.nine.fs.stack.charged(key), 1, "the socket lingers");
    assert_eq!(w.nine.admission().held(key, Resource::State), 1, "and so does its unit, and its bucket");
    assert_eq!(w.nine.admission().keys(), 1);
    // Past the linger deadline ipd resets it, removes it, and gives the unit back.
    w.run_for(70_000_000, 1_000_000);
    assert_eq!(w.nine.fs.stack.charged(key), 0);
    assert_eq!(w.nine.admission().held(key, Resource::State), 0);
    assert_eq!(w.nine.admission().keys(), 0, "the bucket is free again");
}
