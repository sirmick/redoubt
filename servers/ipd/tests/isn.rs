//! Initial sequence numbers (servers/ipd.md R62): every open, active or passive, draws one
//! seed from the CSPRNG for a fresh interface, and its ISN is that seed's; the main interface's
//! PRNG never makes one; and with no seed there is no open, never a fallback.

mod common;

use std::collections::HashSet;

use common::*;
use redoubt_ipd::stack::{CtlError, Ready, Status};
use smoltcp::wire::{EthernetFrame, Ipv4Packet, TcpPacket};

/// smoltcp 0.14.0's interface PRNG (src/rand.rs), reproduced to predict what a seed gives.
struct Rand(u64);

impl Rand {
    fn u32(&mut self) -> u32 {
        const M: u64 = 0xbb2efcec3c39611d;
        const A: u64 = 0x7590ef39;
        let s = self.0.wrapping_mul(M).wrapping_add(A);
        self.0 = s;
        let shift = 29 - (s >> 61);
        (s >> shift) as u32
    }

    fn u16(&mut self) -> u16 {
        let n = self.u32();
        (n ^ (n >> 16)) as u16
    }
}

/// The ISN an interface seeded with `seed` gives its first connection: `Interface::new` draws a
/// non-zero IPv4 ident first (never sent: without fragmentation the ident is 0), then the ISN.
fn isn_of(seed: u64) -> u32 {
    let mut r = Rand(seed);
    while r.u16() == 0 {}
    r.u32()
}

/// Each SYN (without ACK) or SYN-ACK `ipd` sent, with its sequence number, in order.
fn syns(w: &World, with_ack: bool) -> Vec<u32> {
    w.wire
        .borrow()
        .sent
        .iter()
        .filter_map(|f| {
            let eth = EthernetFrame::new_checked(&f[..]).ok()?;
            let ipp = Ipv4Packet::new_checked(eth.payload()).ok()?;
            let t = TcpPacket::new_checked(ipp.payload()).ok()?;
            (t.syn() && t.ack() == with_ack).then_some(t.seq_number().0 as u32)
        })
        .collect()
}

#[test]
fn each_active_open_takes_one_seed_and_its_isn_is_that_seeds() {
    let mut w = World::unmetered(2000);
    let who = owner(&caller(5, 1, &[]));
    let main_seed = w.seeds.borrow()[0];
    assert_eq!(w.seeds.borrow().len(), 1, "one seed for the main interface");
    let mut isns = Vec::new();
    for k in 0..1000u32 {
        let before = w.seeds.borrow().len();
        let n = w.nine.fs.stack.allocate(who, 5, 2000).unwrap();
        w.nine.fs.stack.connect(who, n, &anywhere(), LAN_HOST, 1 + (k % 1000) as u16, w.now).unwrap();
        let drawn = w.seeds.borrow().len() - before;
        assert_eq!(drawn, 1, "open {k} took {drawn} seeds");
        let seed = *w.seeds.borrow().last().unwrap();
        w.nine.fs.stack.poll(w.now);
        w.pump();
        let sent = syns(&w, false);
        assert_eq!(sent.len(), k as usize + 1, "open {k} sent no SYN");
        assert_eq!(*sent.last().unwrap(), isn_of(seed), "open {k}'s ISN is not its seed's");
        isns.push(*sent.last().unwrap());
        w.nine.fs.stack.release(who, n, false, w.now).unwrap();
    }
    // The main interface's PRNG never made one: none of the 1000 is among its first 10,000.
    let mut main = Rand(main_seed);
    let outputs: HashSet<u32> = (0..10_000).map(|_| main.u32()).collect();
    assert!(isns.iter().all(|isn| !outputs.contains(isn)), "an ISN came from the main interface's PRNG");
}

#[test]
fn each_passive_open_takes_one_seed_and_its_isn_is_that_seeds() {
    let mut w = World::unmetered(64);
    let who = owner(&caller(22, 0, &[]));
    let n = w.nine.fs.stack.allocate(who, 22, 16).unwrap();
    w.nine.fs.stack.listen(who, n, &anywhere(), 22, 1, 16).unwrap();
    for k in 0..20u16 {
        let before = w.seeds.borrow().len();
        let _c = w.peer.connect(GATEWAY, 45000 + k, 22);
        w.run_for(100_000, 10_000);
        let drawn = w.seeds.borrow().len() - before;
        assert_eq!(drawn, 1, "SYN {k} took {drawn} seeds");
        let seed = *w.seeds.borrow().last().unwrap();
        assert_eq!(*syns(&w, true).last().unwrap(), isn_of(seed), "SYN-ACK {k}'s ISN is not its seed's");
        let Ok(Ready::Now((Status::Listening, m))) = w.nine.fs.stack.status(who, n, 16) else {
            panic!("{k} not accepted")
        };
        w.nine.fs.stack.release(who, m, false, w.now).unwrap();
    }
}

/// No seed, no open: the connect is `unreachable` and nothing goes out; a SYN is dropped
/// unanswered. Nothing falls back to the main interface's PRNG.
#[test]
fn without_a_seed_nothing_opens() {
    let mut w = World::unmetered(64);
    let who = owner(&caller(5, 1, &[]));
    let listener = owner(&caller(22, 0, &[]));
    let l = w.nine.fs.stack.allocate(listener, 22, 16).unwrap();
    w.nine.fs.stack.listen(listener, l, &anywhere(), 22, 1, 16).unwrap();
    w.fail_random.set(true);
    let n = w.nine.fs.stack.allocate(who, 5, 16).unwrap();
    assert_eq!(w.nine.fs.stack.connect(who, n, &anywhere(), LAN_HOST, 7, w.now), Err(CtlError::Unreachable));
    let _c = w.peer.connect(GATEWAY, 46000, 22);
    w.run_for(500_000, 10_000);
    assert!(syns(&w, false).is_empty() && syns(&w, true).is_empty(), "a connection opened with no seed");
    assert_eq!(w.nine.fs.stack.status(listener, l, 16), Ok(Ready::Wait));
    // With the CSPRNG back, both work.
    w.fail_random.set(false);
    let _l = w.peer.listen(7);
    w.nine.fs.stack.connect(who, n, &anywhere(), LAN_HOST, 7, w.now).unwrap();
    assert_eq!(settle(&mut w, who, n).0, Status::Established);
}
