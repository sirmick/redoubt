//! A listener's `ctl` read (an accept) with nobody connecting waits at most [`CTL_WAIT_US`], then
//! is answered `timeout`: driven through `Ipd::on_call` and `Ipd::expire` on the rt fake kernel,
//! with the clock the test chooses.

#[path = "../../../libs/rt/tests/common/mod.rs"]
mod kernel;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use kernel::fake;
use redoubt_ipd::fake::{ADDR, ANY, GATEWAY, INGRESS, IPD_MAC, LEN, Pipe, Seeds, Wire, anywhere, selfset};
use redoubt_ipd::fs::{NetFs, SocketCaps};
use redoubt_ipd::link::Link;
use redoubt_ipd::server::{CTL_WAIT_US, Ipd};
use redoubt_ipd::stack::{Net, Stack};
use redoubt_rt::abi::{FOREVER, Handle};
use redoubt_rt::client::{Client, ClientError};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::server::Limits;
use redoubt_rt::server::ninep::{NineServer, mode};
use redoubt_rt::wire::proto::net_ctl;

fn ipd() -> Ipd<Pipe, Seeds> {
    let wire = Rc::new(RefCell::new(Wire { selfset: Some(selfset()), ..Default::default() }));
    let entropy = Seeds {
        state: Rc::new(Cell::new(0x0bad_5eed_1234_5678)),
        seeds: Rc::new(RefCell::new(Vec::new())),
        fail: Rc::new(Cell::new(false)),
    };
    let net = Net { addr: ADDR, len: LEN, gateway: Some(GATEWAY), selfset: selfset() };
    let mut stack = Stack::new(net, Link::new(Pipe(wire)), entropy, 64);
    assert!(stack.link_up(IPD_MAC, 0));
    let fs = NetFs::new(stack, &[(ANY, anywhere())], SocketCaps { default: 12, overrides: vec![] });
    let limits = Limits { buckets: 8, in_flight: 5, files: 24, state: 12 };
    Ipd::new(NineServer::new(fs, limits, 7).unwrap(), INGRESS)
}

/// Listens on 8000 and reads its `ctl`, waiting as long as ipd lets it: what the read ended with.
fn accept_nobody(conn: Handle) -> Result<usize, ClientError> {
    let mut c = Client::new(Endpoint::from_handle(conn), 2).unwrap();
    c.attach(0, "").unwrap();
    c.walk(0, 1, "tcp/clone").unwrap();
    c.open(1, mode::OREAD).unwrap();
    let mut n = [0u8; 4];
    c.read(1, 0, &mut n).unwrap();
    c.walk(0, 2, &format!("tcp/{}/ctl", u32::from_le_bytes(n))).unwrap();
    c.open(2, mode::ORDWR).unwrap();
    let mut listen = [0u8; 16];
    let len = net_ctl::Message::Listen(net_ctl::Listen { port: 8000, backlog: 1 })
        .encode_file(&mut listen)
        .unwrap();
    c.write(2, 0, &listen[..len]).unwrap();
    c.timeout = FOREVER;
    let mut status = [0u8; 8];
    c.read(2, 0, &mut status)
}

#[test]
fn an_accept_nobody_answers_ends_with_the_ctl_deadline() {
    let f = fake();
    let pid = f.process(0, &[]);
    let ep = f.endpoint(pid);
    let client = f.process(1, &[]);
    let conn = f.grant(pid, ep, client, ANY);
    let mut ipd = ipd();
    let listener = f.run(client, move || match accept_nobody(conn) {
        Err(ClientError::Remote) => 0,
        Ok(_) => 1,
        Err(_) => 2,
    });
    // Serve every request at time 0 until the accept parks.
    while ipd.parked() == 0 {
        let request = f.as_process(pid, || match Endpoint::from_handle(ep).receive(FOREVER, 1) {
            Ok(Event::Call(request)) => request,
            other => panic!("not a call: {other:?}"),
        });
        f.as_process(pid, || ipd.on_call(request, 0));
    }
    // Just before the deadline it still waits; at the deadline it is answered `timeout`.
    f.as_process(pid, || ipd.expire(CTL_WAIT_US - 1));
    assert_eq!(ipd.parked(), 1, "an accept was answered before its deadline");
    f.as_process(pid, || ipd.expire(CTL_WAIT_US));
    assert_eq!(ipd.parked(), 0, "an accept outlived its deadline");
    assert_eq!(listener.join().unwrap(), 0, "the accept did not end with ipd's `timeout`");
}
