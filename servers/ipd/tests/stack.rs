//! `ipd`'s stack end to end on the host: its sockets against a second smoltcp stack over a
//! recording pipe (tests/common). Every frame `ipd` sends is checked as it goes: no SYN without
//! ACK to one of the box's own addresses, and no ARP for one but the gateway.

mod common;

use common::*;
use redoubt_ipd::link::LinkFault;
use redoubt_ipd::scope::ip;
use redoubt_ipd::stack::{CtlError, EPHEMERAL, LINGER_US, Ready, Status, WaitFor};

const CAP: usize = 16;

fn echo_once(w: &mut World, peer: smoltcp::iface::SocketHandle) {
    let s = w.peer.socket(peer);
    let mut buf = [0u8; 4096];
    if s.can_recv() {
        let n = s.recv_slice(&mut buf).unwrap();
        s.send_slice(&buf[..n]).unwrap();
    }
}

/// Connect, the ctl wait, bytes both ways, the peer's end, close.
#[test]
fn a_connection_carries_bytes_both_ways_and_ends() {
    let mut w = World::new(64);
    let listener = w.peer.listen(7);
    let me = caller(5, 1, &[]);
    let who = owner(&me);
    let n = w.nine.fs.stack.allocate(who, 5, CAP).unwrap();
    assert_eq!(n, 0);
    assert_eq!(w.nine.fs.stack.status(who, n, CAP), Ok(Ready::Now((Status::Closed, 0))), "a fresh socket");
    w.nine.fs.stack.connect(who, n, &anywhere(), LAN_HOST, 7, w.now).unwrap();
    assert_eq!(w.nine.fs.stack.status(who, n, CAP), Ok(Ready::Wait), "a ctl read waits while connecting");
    assert!(w.nine.fs.stack.would_wait(who, n, WaitFor::Ctl));
    assert_eq!(settle(&mut w, who, n), (Status::Established, 0));
    assert_eq!(w.nine.fs.stack.remote(who, n), Ok(Some((LAN_HOST, 7))));
    assert_eq!(w.peer.syns_to(LAN_HOST, 7), 1);

    // Nothing to read yet: a read waits.
    let mut buf = [0u8; 64];
    assert_eq!(w.nine.fs.stack.recv(who, n, &mut buf), Ok(Ready::Wait));
    assert_eq!(w.nine.fs.stack.send(who, n, b"hello, peer"), Ok(Ready::Now(11)));
    w.pump();
    echo_once(&mut w, listener);
    w.pump();
    assert!(!w.nine.fs.stack.would_wait(who, n, WaitFor::Recv));
    assert_eq!(w.nine.fs.stack.recv(who, n, &mut buf), Ok(Ready::Now(11)));
    assert_eq!(&buf[..11], b"hello, peer");

    // The peer ends its side: a read is the end (0), not a wait.
    w.peer.socket(listener).close();
    w.pump();
    assert_eq!(w.nine.fs.stack.recv(who, n, &mut buf), Ok(Ready::Now(0)));
    w.nine.fs.stack.release(who, n, true, w.now).unwrap();
    assert!(!w.nine.fs.stack.exists(who, n), "a closed socket is no longer the owner's");
    // It goes once smoltcp has ended it; then nothing is charged.
    w.run_for(15_000_000, 500_000);
    assert_eq!(w.nine.fs.stack.charged(who.key), 0);
    assert_eq!(w.nine.fs.stack.live(), 0);
}

/// Behind the gateway: ipd asks ARP for the gateway, and only for it.
#[test]
fn a_far_host_is_reached_through_the_gateway() {
    let mut w = World::new(64);
    let listener = w.peer.listen(443);
    let who = owner(&caller(5, 1, &[]));
    let n = w.nine.fs.stack.allocate(who, 5, CAP).unwrap();
    w.nine
        .fs
        .stack
        .connect(who, n, &connect_scope(ip(203, 0, 113, 0), 24, 443, 443), FAR_HOST, 443, w.now)
        .unwrap();
    assert_eq!(settle(&mut w, who, n).0, Status::Established);
    assert!(w.peer.socket(listener).is_active());
}

/// A write waits while the send buffer is full, and goes on once the peer has read.
#[test]
fn a_write_waits_while_the_send_buffer_is_full() {
    let mut w = World::new(64);
    let listener = w.peer.listen(9);
    let who = owner(&caller(5, 1, &[]));
    let n = w.nine.fs.stack.allocate(who, 5, CAP).unwrap();
    w.nine.fs.stack.connect(who, n, &anywhere(), LAN_HOST, 9, w.now).unwrap();
    settle(&mut w, who, n);
    // The peer does not read: its window (16 KiB) and our send buffer (8 KiB) fill.
    let chunk = [0x5au8; 1024];
    let mut sent = 0;
    let mut waited = false;
    for _ in 0..200 {
        match w.nine.fs.stack.send(who, n, &chunk).unwrap() {
            Ready::Now(k) => sent += k,
            Ready::Wait => {
                waited = true;
                break;
            }
        }
        w.pump();
    }
    assert!(waited, "the send buffer never filled");
    assert!(w.nine.fs.stack.would_wait(who, n, WaitFor::Send));
    // The peer reads everything: there is room again.
    let mut got = 0;
    let mut buf = vec![0u8; 65536];
    for _ in 0..100 {
        let s = w.peer.socket(listener);
        while s.can_recv() {
            got += s.recv_slice(&mut buf).unwrap();
        }
        w.pump();
        if !w.nine.fs.stack.would_wait(who, n, WaitFor::Send) {
            break;
        }
    }
    assert!(!w.nine.fs.stack.would_wait(who, n, WaitFor::Send));
    assert!(matches!(w.nine.fs.stack.send(who, n, &chunk), Ok(Ready::Now(k)) if k > 0));
    assert!(got <= sent + chunk.len());
}

/// Listen with a backlog of 2: two connections in, each accepted by a ctl read as a new number,
/// each charged to the listener's owner, and the backlog refilled.
#[test]
fn a_listener_accepts_its_backlog_and_listens_again() {
    let mut w = World::new(64);
    let who = owner(&caller(22, 0, &[]));
    let n = w.nine.fs.stack.allocate(who, 22, CAP).unwrap();
    w.nine.fs.stack.listen(who, n, &listen_scope(22, 22), 22, 2, CAP).unwrap();
    assert_eq!(w.nine.fs.stack.charged(who.key), 2, "the backlog is charged to the holder");
    assert_eq!(w.nine.fs.stack.status(who, n, CAP), Ok(Ready::Wait), "nothing accepted yet");

    let a = w.peer.connect(GATEWAY, 40001, 22);
    let b = w.peer.connect(GATEWAY, 40002, 22);
    w.run_for(200_000, 10_000);
    let Ok(Ready::Now((Status::Listening, m1))) = w.nine.fs.stack.status(who, n, CAP) else {
        panic!("no first accept")
    };
    let Ok(Ready::Now((Status::Listening, m2))) = w.nine.fs.stack.status(who, n, CAP) else {
        panic!("no second accept")
    };
    assert_ne!(m1, m2);
    assert!(m1 != n && m2 != n);
    assert_eq!(w.nine.fs.stack.status(who, n, CAP), Ok(Ready::Wait));
    // Two accepted and a refilled backlog of two, all the holder's.
    assert_eq!(w.nine.fs.stack.charged(who.key), 4);

    // Echo on each.
    for (m, h, msg) in [(m1, a, &b"one"[..]), (m2, b, &b"two"[..])] {
        w.peer.socket(h).send_slice(msg).unwrap();
        w.pump();
        let mut buf = [0u8; 16];
        let Ok(Ready::Now(k)) = w.nine.fs.stack.recv(who, m, &mut buf) else {
            panic!("nothing arrived on {m}")
        };
        assert_eq!(&buf[..k], msg);
        assert_eq!(w.nine.fs.stack.remote(who, m).unwrap().map(|r| r.0), Some(GATEWAY));
    }
    // A third comes in after the refill.
    let _c = w.peer.connect(GATEWAY, 40003, 22);
    w.run_for(200_000, 10_000);
    assert!(matches!(w.nine.fs.stack.status(who, n, CAP), Ok(Ready::Now((Status::Listening, _)))));
}

/// A SYN that is never answered holds its backlog slot for 3 s, and no longer: then the slot
/// listens again.
#[test]
fn a_half_open_connection_gives_its_slot_back() {
    let mut w = World::new(64);
    let who = owner(&caller(22, 0, &[]));
    let n = w.nine.fs.stack.allocate(who, 22, CAP).unwrap();
    w.nine.fs.stack.listen(who, n, &listen_scope(8000, 8000), 8000, 1, CAP).unwrap();
    // A SYN whose sender has gone: nothing ipd sends it arrives, so its SYN-ACK is never
    // answered (nor reset).
    w.wire.borrow_mut().blackhole.push(41000);
    let spoof = w.peer.connect(GATEWAY, 41000, 8000);
    w.run_for(100_000, 10_000);
    assert_eq!(w.peer.syns.len(), 0, "the peer's own SYNs are not counted");
    w.peer.sockets.remove(spoof);
    w.run_for(900_000, 100_000);
    // One second in, the backlog of one is still held: another connection is not accepted.
    let _early = w.peer.connect(GATEWAY, 41001, 8000);
    w.run_for(300_000, 10_000);
    assert_eq!(w.nine.fs.stack.status(who, n, CAP), Ok(Ready::Wait), "accepted while the slot was half-open");
    // Past 3 s the slot listens again, and the next connection is accepted.
    w.run_for(2_000_000, 100_000);
    let _real = w.peer.connect(GATEWAY, 41002, 8000);
    w.run_for(300_000, 10_000);
    assert!(matches!(w.nine.fs.stack.status(who, n, CAP), Ok(Ready::Now((Status::Listening, _)))));
}

/// Outside the prefix, and to the box's own addresses whatever the scope: refused before smoltcp
/// sees anything, so the peer sees no SYN at all.
#[test]
fn connects_outside_the_scope_or_to_the_box_are_refused_and_send_nothing() {
    let mut w = World::new(64);
    let who = owner(&caller(5, 1, &[]));
    let narrow = connect_scope(ip(10, 1, 9, 110), 32, 7, 7);
    let n = w.nine.fs.stack.allocate(who, 5, CAP).unwrap();
    assert_eq!(
        w.nine.fs.stack.connect(who, n, &narrow, ip(10, 1, 9, 101), 7, w.now),
        Err(CtlError::NotPermitted)
    );
    assert_eq!(
        w.nine.fs.stack.connect(who, n, &narrow, ip(10, 1, 9, 110), 8, w.now),
        Err(CtlError::NotPermitted)
    );
    let own = [
        ADDR,
        ip(10, 1, 0, 0),
        ip(10, 1, 255, 255),
        ip(255, 255, 255, 255),
        ip(127, 0, 0, 1),
        ip(127, 1, 2, 3),
        ip(0, 0, 0, 0),
        ip(0, 1, 2, 3),
        ip(224, 0, 0, 1),
        ip(239, 255, 255, 250),
        ip(240, 0, 0, 1),
        SELF_EXTRA,
    ];
    for addr in own {
        assert_eq!(
            w.nine.fs.stack.connect(who, n, &anywhere(), addr, 7, w.now),
            Err(CtlError::NotPermitted),
            "{addr:#x}"
        );
    }
    w.run_for(1_000_000, 100_000);
    assert!(w.peer.syns.is_empty(), "a refused connect sent {:?}", w.peer.syns);
    assert!(w.tcp_sent().is_empty());
    // The socket is still fresh: an allowed connect works.
    let _l = w.peer.listen(7);
    w.nine.fs.stack.connect(who, n, &narrow, ip(10, 1, 9, 110), 7, w.now).unwrap();
    assert_eq!(settle(&mut w, who, n).0, Status::Established);
    assert_eq!(w.peer.syns, vec![(ip(10, 1, 9, 110), 7)]);
}

/// Every socket's number is its owner's own; another owner's is not there at all.
#[test]
fn numbers_are_per_connection_and_invisible_to_others() {
    let mut w = World::new(64);
    let (a, b) = (owner(&caller(5, 1, &[])), owner(&caller(6, 2, &[])));
    assert_eq!(w.nine.fs.stack.allocate(a, 5, CAP), Ok(0));
    assert_eq!(w.nine.fs.stack.allocate(a, 5, CAP), Ok(1));
    assert_eq!(w.nine.fs.stack.allocate(b, 6, CAP), Ok(0));
    assert_eq!(w.nine.fs.stack.numbers(a), vec![0, 1]);
    assert_eq!(w.nine.fs.stack.numbers(b), vec![0]);
    assert!(!w.nine.fs.stack.exists(b, 1));
    w.nine.fs.stack.release(a, 0, false, w.now).unwrap();
    assert_eq!(w.nine.fs.stack.numbers(a), vec![1]);
    assert_eq!(w.nine.fs.stack.allocate(a, 5, CAP), Ok(0), "the lowest free number again");
    // The same badge through another account is another owner (a copied handle).
    let a_elsewhere = owner(&caller(5, 9, &[]));
    assert!(w.nine.fs.stack.numbers(a_elsewhere).is_empty());
}

/// A bucket's sockets stop at its cap, and a backlog must fit it too.
#[test]
fn sockets_stop_at_the_buckets_cap() {
    let mut w = World::new(64);
    let who = owner(&caller(5, 1, &[]));
    for _ in 0..3 {
        w.nine.fs.stack.allocate(who, 5, 3).unwrap();
    }
    assert_eq!(w.nine.fs.stack.allocate(who, 5, 3), Err(CtlError::TooMany));
    let other = owner(&caller(6, 2, &[]));
    let n = w.nine.fs.stack.allocate(other, 6, 3).unwrap();
    assert_eq!(w.nine.fs.stack.listen(other, n, &anywhere(), 5000, 4, 3), Err(CtlError::TooMany));
    assert_eq!(w.nine.fs.stack.listen(other, n, &anywhere(), 5000, 3, 3), Ok(()));
    // And the box-wide limit: every bucket at its cap.
    let mut small = World::new(2);
    let x = owner(&caller(7, 3, &[]));
    small.nine.fs.stack.allocate(x, 7, 8).unwrap();
    small.nine.fs.stack.allocate(x, 7, 8).unwrap();
    assert_eq!(small.nine.fs.stack.allocate(x, 7, 8), Err(CtlError::TooMany));
}

/// A port belongs to the group that listened on it first; another group is `in_use`, the same
/// group joins, and a port in use by a connection is `in_use` to anyone.
#[test]
fn a_listened_port_is_its_groups() {
    let mut w = World::new(64);
    let (a, a2, b) =
        (owner(&caller(22, 0, &[])), owner(&caller(1 << 63 | 5, 0, &[])), owner(&caller(23, 0, &[])));
    let n = w.nine.fs.stack.allocate(a, 22, CAP).unwrap();
    w.nine.fs.stack.listen(a, n, &anywhere(), 22, 1, CAP).unwrap();
    let m = w.nine.fs.stack.allocate(b, 23, CAP).unwrap();
    assert_eq!(w.nine.fs.stack.listen(b, m, &anywhere(), 22, 1, CAP), Err(CtlError::InUse));
    // A connection minted from a's with new_connection is in a's group (22).
    let k = w.nine.fs.stack.allocate(a2, 22, CAP).unwrap();
    assert_eq!(w.nine.fs.stack.listen(a2, k, &anywhere(), 22, 1, CAP), Ok(()));
    // Once a's group has closed every listener on it, the port is free.
    w.nine.fs.stack.release(a, n, true, w.now).unwrap();
    w.nine.fs.stack.release(a2, k, true, w.now).unwrap();
    w.pump();
    assert_eq!(w.nine.fs.stack.listen(b, m, &anywhere(), 22, 1, CAP), Ok(()));
    // An ephemeral port a connection holds cannot be listened on.
    let _l = w.peer.listen(7);
    let c = w.nine.fs.stack.allocate(a, 22, CAP).unwrap();
    w.nine.fs.stack.connect(a, c, &anywhere(), LAN_HOST, 7, w.now).unwrap();
    settle(&mut w, a, c);
    let port = local_port(&w, LAN_HOST, 7);
    let d = w.nine.fs.stack.allocate(a, 22, CAP).unwrap();
    assert_eq!(w.nine.fs.stack.listen(a, d, &anywhere(), port, 1, CAP), Err(CtlError::InUse));
}

/// The source port of the first SYN `ipd` sent to (addr, port).
fn local_port(w: &World, addr: u32, port: u16) -> u16 {
    let wire = w.wire.borrow();
    for f in &wire.sent {
        let eth = smoltcp::wire::EthernetFrame::new_checked(&f[..]).unwrap();
        if let Ok(ipp) = smoltcp::wire::Ipv4Packet::new_checked(eth.payload()) {
            if let Ok(t) = smoltcp::wire::TcpPacket::new_checked(ipp.payload()) {
                if t.syn() && u32::from(ipp.dst_addr()) == addr && t.dst_port() == port {
                    return t.src_port();
                }
            }
        }
    }
    panic!("no SYN to {addr:#x}:{port}");
}

/// Ephemeral ports are drawn from 49152-65535 and never repeat among live sockets.
#[test]
fn ephemeral_ports_are_unique() {
    let mut w = World::new(200);
    let _l = w.peer.listen(7);
    let who = owner(&caller(5, 1, &[]));
    for _ in 0..40 {
        let n = w.nine.fs.stack.allocate(who, 5, 100).unwrap();
        w.nine.fs.stack.connect(who, n, &anywhere(), LAN_HOST, 7, w.now).unwrap();
    }
    w.run_for(500_000, 50_000);
    let mut ports: Vec<u16> = Vec::new();
    for f in &w.wire.borrow().sent {
        let eth = smoltcp::wire::EthernetFrame::new_checked(&f[..]).unwrap();
        let Ok(ipp) = smoltcp::wire::Ipv4Packet::new_checked(eth.payload()) else { continue };
        let t = smoltcp::wire::TcpPacket::new_checked(ipp.payload()).unwrap();
        if t.syn() && !t.ack() {
            ports.push(t.src_port());
        }
    }
    assert_eq!(ports.len(), 40);
    assert!(ports.iter().all(|p| EPHEMERAL.contains(p)));
    let mut unique = ports.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 40, "a port was used twice");
}

/// A port draw that lands on a port in use is drawn again (QA D3-code-review-5, P2-3 c): with the
/// generator set back before each connect, every connect's first draw is the same port, and each
/// still gets a port of its own, until `PORT_TRIES` draws all land on ports in use.
#[test]
fn a_port_in_use_is_drawn_again() {
    use redoubt_ipd::stack::PORT_TRIES;
    let mut w = World::new(200);
    let _l = w.peer.listen(7);
    let who = owner(&caller(5, 1, &[]));
    let start = w.rng.get();
    for _ in 0..PORT_TRIES {
        w.rng.set(start);
        let n = w.nine.fs.stack.allocate(who, 5, 100).unwrap();
        w.nine.fs.stack.connect(who, n, &anywhere(), LAN_HOST, 7, w.now).unwrap();
    }
    // The 17th connect's 16 draws all land on ports already taken.
    w.rng.set(start);
    let n = w.nine.fs.stack.allocate(who, 5, 100).unwrap();
    assert_eq!(w.nine.fs.stack.connect(who, n, &anywhere(), LAN_HOST, 7, w.now), Err(CtlError::TooMany));
    w.run_for(500_000, 50_000);
    let mut ports: Vec<u16> = Vec::new();
    for f in &w.wire.borrow().sent {
        let eth = smoltcp::wire::EthernetFrame::new_checked(&f[..]).unwrap();
        let Ok(ipp) = smoltcp::wire::Ipv4Packet::new_checked(eth.payload()) else { continue };
        let t = smoltcp::wire::TcpPacket::new_checked(ipp.payload()).unwrap();
        if t.syn() && !t.ack() {
            ports.push(t.src_port());
        }
    }
    let mut unique = ports.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!((ports.len(), unique.len()), (PORT_TRIES, PORT_TRIES), "a port was used twice: {ports:?}");
}

/// `disconnect` aborts every socket of the connection and gives its charges back.
#[test]
fn a_disconnect_aborts_and_returns_the_charges() {
    let mut w = World::new(64);
    let listener = w.peer.listen(7);
    let who = owner(&caller(1 << 63 | 9, 1, &[]));
    let n = w.nine.fs.stack.allocate(who, 9, CAP).unwrap();
    w.nine.fs.stack.connect(who, n, &anywhere(), LAN_HOST, 7, w.now).unwrap();
    settle(&mut w, who, n);
    let m = w.nine.fs.stack.allocate(who, 9, CAP).unwrap();
    w.nine.fs.stack.listen(who, m, &anywhere(), 7000, 2, CAP).unwrap();
    assert_eq!(w.nine.fs.stack.charged(who.key), 3);
    w.nine.fs.stack.disconnect(who.badge, w.now);
    assert!(w.nine.fs.stack.numbers(who).is_empty());
    w.pump();
    assert!(!w.peer.socket(listener).is_active(), "the peer saw the reset");
    assert_eq!(w.nine.fs.stack.charged(who.key), 0);
}

/// A closed socket whose peer never answers is reset when its linger ends: its charge is gone
/// within 60 s plus smoltcp's own timers.
#[test]
fn a_lingering_socket_is_bounded() {
    let mut w = World::new(64);
    let _listener = w.peer.listen(7);
    let who = owner(&caller(5, 1, &[]));
    let n = w.nine.fs.stack.allocate(who, 5, CAP).unwrap();
    w.nine.fs.stack.connect(who, n, &anywhere(), LAN_HOST, 7, w.now).unwrap();
    settle(&mut w, who, n);
    assert_eq!(w.nine.fs.stack.send(who, n, &[1; 100]), Ok(Ready::Now(100)));
    // The peer vanishes: nothing ipd sends is answered.
    w.wire.borrow_mut().fault = Some(LinkFault::Dropped);
    w.nine.fs.stack.release(who, n, true, w.now).unwrap();
    w.run_for(LINGER_US + 1_000_000, 1_000_000);
    assert_eq!(w.nine.fs.stack.charged(who.key), 0, "still charged after the linger");
}

/// No link: `netd` says its device is broken, connects and listens answer `unreachable`, and the
/// stack works again once `info` does.
#[test]
fn no_link_is_unreachable_until_it_comes_back() {
    let mut w = World::new(64);
    // Two: the connect made while the link was down goes out too once it is back.
    let (_l1, _l2) = (w.peer.listen(7), w.peer.listen(7));
    let who = owner(&caller(5, 1, &[]));
    w.wire.borrow_mut().fault = Some(LinkFault::Down);
    // Something must try to send for the link to learn it is down.
    let n = w.nine.fs.stack.allocate(who, 5, CAP).unwrap();
    w.nine.fs.stack.connect(who, n, &anywhere(), LAN_HOST, 7, w.now).unwrap();
    w.pump();
    assert!(!w.nine.fs.stack.is_up());
    let m = w.nine.fs.stack.allocate(who, 5, CAP).unwrap();
    assert_eq!(w.nine.fs.stack.connect(who, m, &anywhere(), LAN_HOST, 7, w.now), Err(CtlError::Unreachable));
    assert_eq!(w.nine.fs.stack.listen(who, m, &anywhere(), 80, 1, CAP), Err(CtlError::Unreachable));
    // `netd` answers again.
    w.wire.borrow_mut().fault = None;
    assert!(w.nine.fs.stack.link_up(IPD_MAC, w.now));
    w.nine.fs.stack.connect(who, m, &anywhere(), LAN_HOST, 7, w.now).unwrap();
    assert_eq!(settle(&mut w, who, m).0, Status::Established);
    // A MAC that is not unicast does not bring it up.
    assert!(!w.nine.fs.stack.link_up([0xff; 6], w.now));
    assert!(!w.nine.fs.stack.link_up([0x01, 0, 0x5e, 0, 0, 1], w.now));
    assert!(!w.nine.fs.stack.link_up([0; 6], w.now));
}

/// Inbound TCP claiming to come from the box itself or from nowhere is dropped before any
/// socket sees it.
#[test]
fn martian_sources_are_dropped() {
    let mut w = World::new(64);
    let who = owner(&caller(22, 0, &[]));
    let n = w.nine.fs.stack.allocate(who, 22, CAP).unwrap();
    w.nine.fs.stack.listen(who, n, &anywhere(), 8000, 2, CAP).unwrap();
    for src in [ADDR, ip(127, 0, 0, 1), ip(0, 0, 0, 5)] {
        let _s = w.peer.connect(src, 42000, 8000);
    }
    w.run_for(500_000, 10_000);
    assert_eq!(w.nine.fs.stack.status(who, n, CAP), Ok(Ready::Wait), "a martian SYN was taken");
    assert!(w.tcp_sent().is_empty(), "ipd answered a martian: {:?}", w.tcp_sent());
    // The gateway's address (where QEMU's forwarded connections come from) is not martian.
    let _ok = w.peer.connect(GATEWAY, 42001, 8000);
    w.run_for(300_000, 10_000);
    assert!(matches!(w.nine.fs.stack.status(who, n, CAP), Ok(Ready::Now((Status::Listening, _)))));
}

/// IPv4 that is not TCP gets no answer from anyone, and IPv4 of any protocol from the box's own
/// addresses is dropped before its protocol is looked at (QA D3-code-review-5, P2-1): without that,
/// smoltcp answers UDP or an unknown protocol with an ICMP "protocol unreachable", so a spoofed
/// source gets a reflection, and ipd's own address makes it ask ARP for itself. Every frame ipd
/// sends is checked as it goes (TCP or ARP only, no ARP for its own), and none may be sent here.
#[test]
fn non_tcp_and_martian_datagrams_get_no_answer() {
    use redoubt_ipd::fake::{datagram, echo_request};
    use redoubt_ipd::stack::{Class, classify};
    use smoltcp::wire::IpProtocol;
    let mut w = World::new(64);
    w.pump();
    let before = w.wire.borrow().sent.len();
    let net = redoubt_ipd::stack::Net { addr: ADDR, len: LEN, gateway: Some(GATEWAY), selfset: selfset() };
    let protocols = [IpProtocol::Udp, IpProtocol::Icmp, IpProtocol::Unknown(99)];
    let sources = [
        ADDR,
        ip(127, 0, 0, 1),
        ip(0, 0, 0, 5),
        SELF_EXTRA,
        ip(10, 1, 255, 255),
        LAN_HOST,
        FAR_HOST,
        GATEWAY,
    ];
    for protocol in protocols {
        for src in sources {
            let payload =
                if protocol == IpProtocol::Icmp { echo_request(b"ping") } else { b"udp or not".to_vec() };
            let frame = datagram(src, ADDR, protocol, &payload);
            let class = classify(&frame, &net);
            let martian = src != GATEWAY && (selfset().contains(src) || src == ADDR);
            let expected = if martian { Class::Martian } else { Class::NotTcp };
            assert_eq!(class, expected, "{protocol} from {src:#x}");
            let now = w.now;
            w.nine.fs.stack.ingress(&frame, now);
            w.pump();
        }
    }
    // TCP from the box's own addresses is martian too, and from the gateway it is not.
    let syn = segment(ADDR, ADDR, &syn_to(8000));
    assert_eq!(classify(&syn, &net), Class::Martian);
    assert_eq!(classify(&segment(GATEWAY, ADDR, &syn_to(8000)), &net), Class::Syn { port: 8000 });
    w.run_for(1_000_000, 100_000);
    let sent = w.wire.borrow().sent.len();
    assert_eq!(sent, before, "ipd answered a datagram that is not TCP, or one from its own addresses");
}

fn syn_to(port: u16) -> smoltcp::wire::TcpRepr<'static> {
    use smoltcp::wire::{TcpControl, TcpRepr, TcpSeqNumber};
    TcpRepr {
        src_port: 42000,
        dst_port: port,
        control: TcpControl::Syn,
        seq_number: TcpSeqNumber(1),
        ack_number: None,
        window_len: 1024,
        window_scale: None,
        max_seg_size: None,
        sack_permitted: false,
        sack_ranges: [None, None, None],
        timestamp: None,
        payload: &[],
    }
}
