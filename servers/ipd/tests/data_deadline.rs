//! A `data` read with nothing coming waits at most [`DATA_WAIT_US`], then is answered `timeout`,
//! and bytes the peer sends after that wait in the socket's buffer for the next read, which is
//! answered at once with them: nothing is lost to the stream by the deadline. Driven through
//! `Ipd::on_call`, `Ipd::expire` and `Ipd::poll` on the rt fake kernel, the peer a second smoltcp
//! stack, with the clock the test chooses.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc;

use redoubt_fake_kernel::fake;
use redoubt_ipd::fake::{
    ADDR, ANY, GATEWAY, INGRESS, IPD_MAC, LAN_HOST, LEN, Peer, Pipe, Seeds, Wire, anywhere, instant, selfset,
};
use redoubt_ipd::fs::{NetFs, SocketCaps};
use redoubt_ipd::link::Link;
use redoubt_ipd::server::{DATA_WAIT_US, Ipd};
use redoubt_ipd::stack::{Net, Stack};
use redoubt_rt::abi::{FOREVER, Handle};
use redoubt_rt::client::{ClientError, Connection, Lend};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::server::Limits;
use redoubt_rt::server::ninep::{NineServer, mode};
use redoubt_rt::wire::proto::net_ctl;

/// The clock when the test begins (µs): the link comes up and the connection is made then.
const START: u64 = 1_000_000;
/// What the peer sends once the read has timed out.
const LATE: &[u8] = b"late";

fn ipd(wire: &Rc<RefCell<Wire>>) -> Ipd<Pipe, Seeds> {
    let entropy = Seeds {
        state: Rc::new(Cell::new(0x0bad_5eed_1234_5678)),
        seeds: Rc::new(RefCell::new(Vec::new())),
        fail: Rc::new(Cell::new(false)),
    };
    let net = Net { addr: ADDR, len: LEN, gateway: Some(GATEWAY), selfset: selfset() };
    let mut stack = Stack::new(net, Link::new(Pipe(wire.clone())), entropy, 64);
    assert!(stack.link_up(IPD_MAC, START));
    let fs = NetFs::new(stack, &[(ANY, anywhere())], SocketCaps { default: 12, overrides: vec![] });
    let limits = Limits { buckets: 8, in_flight: 5, files: 24, state: 12, requests: 0, pages: 0 };
    Ipd::new(NineServer::new(fs, limits, 7).unwrap(), INGRESS)
}

/// What the program does after a `receive` that returned no call: polls the stack, which serves
/// again every parked call that would no longer wait; then moves frames both ways with the peer,
/// until nothing moves.
fn pump(ipd: &mut Ipd<Pipe, Seeds>, wire: &Rc<RefCell<Wire>>, peer: &mut Peer, now: u64) {
    for _ in 0..10_000 {
        ipd.received_no_call();
        ipd.poll(now);
        let mut moved = false;
        loop {
            let next = wire.borrow_mut().to_peer.pop_front();
            let Some(frame) = next else { break };
            peer.dev.rx.push_back(frame);
            moved = true;
        }
        peer.iface.poll(instant(now), &mut peer.dev, &mut peer.sockets);
        while let Some(frame) = peer.dev.tx.pop_front() {
            ipd.fs().stack.ingress(&frame, now);
            moved = true;
        }
        if !moved {
            return;
        }
    }
    panic!("the network never went quiet");
}

/// Connects to the peer's port 7 and reads its `data` with nothing coming, waiting as long as
/// `ipd` lets it; then, told the peer has sent, reads again. 0 if the first read ended with `ipd`'s
/// `timeout` and the second was answered with what the peer sent; 1 if the first read ended
/// otherwise; 2 if the second did.
fn read_twice(conn: Handle, again: mpsc::Receiver<()>) -> u32 {
    let mut c = Connection::new(Endpoint::from_handle(conn));
    let mut lend = Lend::new(2).unwrap();
    c.attach(&mut lend, 0, "").unwrap();
    c.walk(&mut lend, 0, 1, "tcp/clone").unwrap();
    c.open(&mut lend, 1, mode::OREAD).unwrap();
    let mut n = [0u8; 4];
    c.read(&mut lend, 1, 0, &mut n).unwrap();
    let n = u32::from_le_bytes(n);
    c.walk(&mut lend, 0, 2, &format!("tcp/{n}/ctl")).unwrap();
    c.open(&mut lend, 2, mode::ORDWR).unwrap();
    let mut connect = [0u8; 16];
    let len = net_ctl::Message::Connect(net_ctl::Connect { addr: &LAN_HOST.to_be_bytes(), port: 7 })
        .encode_file(&mut connect)
        .unwrap();
    c.write(&mut lend, 2, 0, &connect[..len]).unwrap();
    c.timeout = FOREVER;
    // The `ctl` read waits while the handshake runs, and completes with state 2 (established).
    let mut status = [0u8; 8];
    c.read(&mut lend, 2, 0, &mut status).unwrap();
    assert_eq!(u32::from_le_bytes(status[..4].try_into().unwrap()), 2, "not established");
    c.walk(&mut lend, 0, 3, &format!("tcp/{n}/data")).unwrap();
    c.open(&mut lend, 3, mode::OREAD).unwrap();
    let mut got = [0u8; 16];
    if !matches!(c.read(&mut lend, 3, 0, &mut got), Err(ClientError::Rerror(_))) {
        return 1;
    }
    again.recv().unwrap();
    match c.read(&mut lend, 3, 0, &mut got) {
        Ok(k) if &got[..k] == LATE => 0,
        _ => 2,
    }
}

#[test]
fn a_data_read_with_nothing_coming_ends_with_the_data_deadline_and_late_bytes_wait_for_the_next() {
    let f = fake();
    let pid = f.process(0, &[]);
    let ep = f.endpoint(pid);
    let client = f.process(1, &[]);
    let conn = f.grant(pid, ep, client, ANY);
    let wire = Rc::new(RefCell::new(Wire { selfset: Some(selfset()), ..Default::default() }));
    let mut ipd = ipd(&wire);
    let mut peer = Peer::new(START);
    let listening = peer.listen(7);
    let (tell, again) = mpsc::channel();
    let reader = f.run(client, move || read_twice(conn, again));
    let mut now = START;
    // Serves every request at `now` until one parks.
    let serve_until_parked = |ipd: &mut Ipd<Pipe, Seeds>, now: u64| {
        while ipd.parked() == 0 {
            let request = f.as_process(pid, || match Endpoint::from_handle(ep).receive(FOREVER, 1) {
                Ok(Event::Call(request)) => request,
                other => panic!("not a call: {other:?}"),
            });
            f.as_process(pid, || ipd.on_call(request, now));
        }
    };
    // The `ctl` read parks while connecting; the handshake runs and it is answered.
    serve_until_parked(&mut ipd, now);
    f.as_process(pid, || pump(&mut ipd, &wire, &mut peer, now));
    assert_eq!(ipd.parked(), 0, "the ctl read was not answered once established");
    // The `data` read parks with nothing coming. Just before the deadline it still waits; at the
    // deadline it is answered `timeout`.
    serve_until_parked(&mut ipd, now);
    f.as_process(pid, || ipd.expire(now + DATA_WAIT_US - 1));
    assert_eq!(ipd.parked(), 1, "a data read was answered before its deadline");
    now += DATA_WAIT_US;
    f.as_process(pid, || ipd.expire(now));
    assert_eq!(ipd.parked(), 0, "a data read outlived its deadline");
    // The peer sends after the deadline: the bytes reach the socket's buffer with no read parked,
    // and the read the client makes next is answered at once with them.
    peer.socket(listening).send_slice(LATE).unwrap();
    now += 1;
    f.as_process(pid, || pump(&mut ipd, &wire, &mut peer, now));
    assert_eq!(ipd.parked(), 0);
    tell.send(()).unwrap();
    let request = f.as_process(pid, || match Endpoint::from_handle(ep).receive(FOREVER, 1) {
        Ok(Event::Call(request)) => request,
        other => panic!("not a call: {other:?}"),
    });
    f.as_process(pid, || ipd.on_call(request, now));
    assert_eq!(ipd.parked(), 0, "the read after the late bytes parked instead of taking them");
    assert_eq!(
        reader.join().unwrap(),
        0,
        "the first read did not end with `timeout`, or the second lost the bytes"
    );
}
