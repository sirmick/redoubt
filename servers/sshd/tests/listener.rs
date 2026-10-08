//! The listener's accept outlives `ipd`'s wait: a `ctl` read `ipd` answers `timeout` at its
//! deadline is asked again, and only another answer ends it. Driven against the real `ipd`, through
//! `Ipd::on_call` and `Ipd::expire` on the rt fake kernel, with the clock the test chooses.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use redoubt_fake_kernel::fake;
use redoubt_ipd::fake::{ADDR, ANY, GATEWAY, INGRESS, IPD_MAC, LEN, Pipe, Seeds, Wire, anywhere, selfset};
use redoubt_ipd::fs::{NetFs, SocketCaps};
use redoubt_ipd::link::Link;
use redoubt_ipd::server::{CTL_WAIT_US, Ipd};
use redoubt_ipd::stack::{Net, Stack};
use redoubt_rt::abi::{FOREVER, Handle};
use redoubt_rt::client::{Connection, Lend};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::server::Limits;
use redoubt_rt::server::ninep::{NineServer, mode};
use redoubt_rt::wire::proto::net_ctl;
use redoubt_sshd::listener::status;

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
    let limits = Limits { buckets: 8, in_flight: 5, files: 24, state: 12, requests: 0, pages: 0 };
    Ipd::new(NineServer::new(fs, limits, 7).unwrap(), INGRESS)
}

/// Listens on port 22 as `sshd` does, then waits in [`status`] for an accept: 1 if it gave one,
/// 0 if it ended without.
fn listen_and_accept(conn: Handle) -> u32 {
    let mut c = Connection::new(Endpoint::from_handle(conn));
    let mut lend = Lend::new(2).unwrap();
    c.attach(&mut lend, 0, "").unwrap();
    c.walk(&mut lend, 0, 1, "tcp/clone").unwrap();
    c.open(&mut lend, 1, mode::OREAD).unwrap();
    let mut n = [0u8; 4];
    c.read(&mut lend, 1, 0, &mut n).unwrap();
    c.walk(&mut lend, 0, 2, &format!("tcp/{}/ctl", u32::from_le_bytes(n))).unwrap();
    c.open(&mut lend, 2, mode::ORDWR).unwrap();
    let mut listen = [0u8; 16];
    let len =
        net_ctl::Message::Listen(net_ctl::Listen { port: 22, backlog: 1 }).encode_file(&mut listen).unwrap();
    c.write(&mut lend, 2, 0, &listen[..len]).unwrap();
    c.timeout = FOREVER;
    u32::from(status(&c, &mut lend, 2).is_some())
}

#[test]
fn an_accept_ipd_s_wait_ran_out_on_is_asked_again() {
    let f = fake();
    let pid = f.process(0, &[]);
    let ep = f.endpoint(pid);
    let client = f.process(1, &[]);
    let conn = f.grant(pid, ep, client, ANY);
    let mut ipd = ipd();
    let listener = f.run(client, move || listen_and_accept(conn));
    let serve_until_parked = |ipd: &mut Ipd<Pipe, Seeds>, now: u64| {
        while ipd.parked() == 0 {
            let request = f.as_process(pid, || match Endpoint::from_handle(ep).receive(FOREVER, 1) {
                Ok(Event::Call(request)) => request,
                other => panic!("not a call: {other:?}"),
            });
            f.as_process(pid, || ipd.on_call(request, now));
        }
    };
    serve_until_parked(&mut ipd, 0);
    // At the deadline ipd answers the accept `timeout`; the listener asks again, and waits.
    f.as_process(pid, || ipd.expire(CTL_WAIT_US));
    assert_eq!(ipd.parked(), 0, "the accept outlived ipd's deadline");
    serve_until_parked(&mut ipd, CTL_WAIT_US);
    assert_eq!(ipd.parked(), 1, "the listener did not ask again after `timeout`");
    // Any other answer ends it: ipd going away answers the parked accept `malformed`.
    f.as_process(pid, || drop(ipd));
    assert_eq!(listener.join().unwrap(), 0, "the listener gave an accept nobody made");
}

/// The listener's accept and a connection's read or write are asked again at once after ipd's
/// `timeout`, after a pause after `too many` (every in-flight call of sshd's share taken) until
/// it has been refused that often in a row, and never after anything else.
#[test]
fn a_data_call_refused_too_many_is_asked_again_a_bounded_number_of_times() {
    use redoubt_rt::client::ClientError;
    use redoubt_rt::wire::ninep::ErrorName;
    use redoubt_sshd::listener::{Again, RETRY_US, TOO_MANY_TRIES, again};
    let (timeout, too_many) =
        (ClientError::Rerror(ErrorName::Timeout), ClientError::Rerror(ErrorName::TooMany));
    assert_eq!(again(&timeout, TOO_MANY_TRIES), Again::Now);
    assert_eq!(again(&too_many, 0), Again::After(RETRY_US));
    assert_eq!(again(&too_many, TOO_MANY_TRIES - 1), Again::After(RETRY_US));
    assert_eq!(again(&too_many, TOO_MANY_TRIES), Again::No);
    assert_eq!(again(&ClientError::Rerror(ErrorName::NotFound), 0), Again::No);
    assert_eq!(again(&ClientError::Remote, 0), Again::No);
}
