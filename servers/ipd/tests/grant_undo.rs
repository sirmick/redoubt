//! `grant`'s undo (answer 168; QA D3-code-review-5, P2-3 d): a connection `grant` minted is
//! forgotten again, with its scope, unless the reply carrying its handle was delivered. Driven
//! through `Ipd::on_call` on the rt fake kernel, with a caller that gives up before the reply.

#[path = "../../../libs/rt/tests/common/mod.rs"]
mod kernel;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use kernel::fake;
use redoubt_ipd::fake::{
    ADDR, ANY, GATEWAY, INGRESS, LEN, Pipe, Seeds, Wire, anywhere, connect_scope, selfset,
};
use redoubt_ipd::fs::{NetFs, SocketCaps};
use redoubt_ipd::link::Link;
use redoubt_ipd::scope::ip;
use redoubt_ipd::server::Ipd;
use redoubt_ipd::stack::{Net, Stack};
use redoubt_rt::abi::{Error, FOREVER, Handle};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Buffer, Event};
use redoubt_rt::server::Limits;
use redoubt_rt::server::ninep::NineServer;
use redoubt_rt::wire::proto::ipd;

fn ipd() -> Ipd<Pipe, Seeds> {
    let wire = Rc::new(RefCell::new(Wire { selfset: Some(selfset()), ..Default::default() }));
    let entropy = Seeds {
        state: Rc::new(Cell::new(0x1234_5678_9abc_def1)),
        seeds: Rc::new(RefCell::new(Vec::new())),
        fail: Rc::new(Cell::new(false)),
    };
    let net = Net { addr: ADDR, len: LEN, gateway: Some(GATEWAY), selfset: selfset() };
    let stack = Stack::new(net, Link::new(Pipe(wire)), entropy, 64);
    let fs = NetFs::new(stack, &[(ANY, anywhere())], SocketCaps { default: 8, overrides: vec![] });
    let limits = Limits { buckets: 8, in_flight: 5, files: 24, state: 4 };
    Ipd::new(NineServer::new(fs, limits, 7).unwrap(), INGRESS)
}

/// A `grant` of 10.1.9.110/32 port 7 on `conn`, waiting `timeout` for the answer: whether a
/// connection came back.
fn grant(conn: Handle, timeout: u64) -> u32 {
    let scope = connect_scope(ip(10, 1, 9, 110), 32, 7, 7).encode();
    let mut page = Buffer::new(1).unwrap();
    let words = ipd::Message::Grant(ipd::Grant { scope: &scope }).encode(&mut page).unwrap();
    match Endpoint::from_handle(conn).call(&words, &[], Some(page), timeout).into_result() {
        Ok((reply, _)) => u32::from(reply.handles.as_slice().iter().flatten().count() == 1),
        Err(Error::Timeout) => 100,
        Err(_) => 200,
    }
}

/// The next call on `ep`, past the notice of the call the steward abandoned.
fn next_call(ep: Handle) -> redoubt_rt::ipc::Request {
    loop {
        match Endpoint::from_handle(ep).receive(FOREVER, 1) {
            Ok(Event::Call(request)) => return request,
            Ok(Event::Abandoned(_)) => continue,
            other => panic!("not a call: {other:?}"),
        }
    }
}

#[test]
fn a_grant_nobody_received_is_undone() {
    let f = fake();
    let pid = f.process(0, &[]);
    let ep = f.endpoint(pid);
    let steward = f.process(0, &[]);
    let conn = f.grant(pid, ep, steward, ANY);
    let mut ipd = ipd();
    let scopes = ipd.nine.fs.scopes_kept();

    // The steward gives up before ipd answers: ipd has taken the call, the reply reaches nobody.
    let caller = f.run(steward, move || grant(conn, 50_000));
    let request = f.as_process(pid, || next_call(ep));
    assert_eq!(caller.join().unwrap(), 100, "the steward's call timed out");
    f.as_process(pid, || ipd.on_call(request, 0));
    assert_eq!(ipd.nine.connections(), 0, "the connection outlived its undelivered grant");
    assert_eq!(ipd.nine.fs.scopes_kept(), scopes, "the scope outlived its undelivered grant");

    // The control: the same grant, received, is kept.
    let caller = f.run(steward, move || grant(conn, FOREVER));
    let request = f.as_process(pid, || next_call(ep));
    f.as_process(pid, || ipd.on_call(request, 0));
    assert_eq!(caller.join().unwrap(), 1, "the steward got its connection");
    assert_eq!(ipd.nine.connections(), 1);
    assert_eq!(ipd.nine.fs.scopes_kept(), scopes + 1);
}
