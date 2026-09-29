//! `typed`, against the real `keyd` and against hostile servers, on the fake kernel: the reply
//! decodes through the generated codec, the server's refusal is its own error code, and no reply
//! leaves a handle behind that nobody owns.

mod common;

use common::{AUDIT_BADGE, HOST_BADGE, Keyd};
use redoubt_client::typed::{self, Received};
use redoubt_client::{Error, Lend};
use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{FOREVER, Handles};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::server::typed::{Outcome, finish};
use redoubt_rt::wire::proto::keyd::{self, ErrorCode, Grant, Message, PublicKey, Release, Reply, SignRecord};

/// Buffer-shaped and inline calls both round-trip: a public key and a signature through the lend,
/// a grant's capability through the reply's handles, taken by its new owner, and its release.
#[test]
fn typed_calls_reach_keyd() {
    let keyd = Keyd::start();
    let (client, audit) = keyd.client(AUDIT_BADGE);
    let before = fake().held(client).0;
    fake().as_process(client, || {
        let to = Endpoint::from_handle(audit);
        let mut lend = Lend::new(1).unwrap();
        let public = typed::call::<keyd::Protocol, _>(
            &to,
            &mut lend,
            &Message::PublicKey(PublicKey {}),
            &[],
            |r, _| match r {
                Reply::PublicKey(r) => r.key.to_vec(),
                _ => panic!("another reply"),
            },
        );
        assert_eq!(public.unwrap().len(), 32);
        let signed = typed::call::<keyd::Protocol, _>(
            &to,
            &mut lend,
            &Message::SignRecord(SignRecord { record: b"r" }),
            &[],
            |r, _| matches!(r, Reply::SignRecord(s) if s.signature.len() == 64),
        );
        assert_eq!(signed, Ok(true));
        let (id, capability) = typed::call::<keyd::Protocol, _>(
            &to,
            &mut lend,
            &Message::Grant(Grant {}),
            &[],
            |r, got: &mut Received| {
                let Reply::Grant(g) = r else { panic!("another reply") };
                (g.id, got.take(0).expect("the capability"))
            },
        )
        .unwrap();
        let child = Endpoint::from_handle(capability);
        let ok = |r: Reply<'_>, _: &mut Received| matches!(r, Reply::SignRecord(_));
        assert_eq!(
            typed::call::<keyd::Protocol, _>(
                &child,
                &mut lend,
                &Message::SignRecord(SignRecord { record: b"x" }),
                &[],
                ok
            ),
            Ok(true)
        );
        typed::call::<keyd::Protocol, _>(&to, &mut lend, &Message::Release(Release { id }), &[], |_, _| ())
            .unwrap();
        // The attack: the released capability does nothing, and says so with keyd's own code.
        let refused = typed::call::<keyd::Protocol, _>(
            &child,
            &mut lend,
            &Message::SignRecord(SignRecord { record: b"x" }),
            &[],
            ok,
        );
        assert_eq!(refused, Err(Error::Server(ErrorCode::NotPermitted.code())));
        child.close().unwrap();
    });
    assert_eq!(fake().held(client).0, before, "every handle has its owner, and was closed by it");
    keyd.stop();
}

/// The attack: a call keyd refuses fails with keyd's code, whatever the library would think of it.
#[test]
fn a_refusal_is_the_servers_code() {
    let keyd = Keyd::start();
    let (client, host) = keyd.client(HOST_BADGE);
    fake().as_process(client, || {
        let mut lend = Lend::new(1).unwrap();
        let sign = Message::SignRecord(SignRecord { record: b"not the host key's job" });
        let refused =
            typed::call::<keyd::Protocol, _>(&Endpoint::from_handle(host), &mut lend, &sign, &[], |_, _| ());
        assert_eq!(refused, Err(Error::Server(ErrorCode::NotPermitted.code())));
    });
    keyd.stop();
}

/// A hostile server's replies: a success carrying more handles than its table says, an error
/// carrying handles, words that do not decode. Each is refused, and none leaves a handle in the
/// client.
#[test]
fn a_hostile_reply_leaves_no_handle() {
    let f = fake();
    let hostile = f.process(0, &[]);
    let receive = f.endpoint(hostile);
    let client = f.process(1001, &[]);
    let conn = f.grant(hostile, receive, client, 1);
    let replies: [[u64; 4]; 3] = [[0, 7, 0, 0], [2, 0, 0, 0], [0, 1 << 40, 0, 0]];
    let server = f.run(hostile, move || {
        let endpoint = Endpoint::from_handle(receive);
        for words in replies {
            let Ok(Event::Call(request)) = endpoint.receive(FOREVER, 0) else { return 1 };
            let junk = [Endpoint::create().unwrap().handle(), Endpoint::create().unwrap().handle()];
            let send = Handles::from_slice(&junk).unwrap();
            finish(request, &Outcome { words, send, close: send }).unwrap();
        }
        0
    });
    let before = f.held(client).0;
    f.as_process(client, || {
        let mut lend = Lend::new(1).unwrap();
        let to = Endpoint::from_handle(conn);
        for _ in replies {
            let grant =
                typed::call::<keyd::Protocol, _>(&to, &mut lend, &Message::Grant(Grant {}), &[], |_, _| ());
            assert!(grant.is_err(), "{grant:?}");
        }
    });
    assert_eq!(server.join().unwrap(), 0);
    assert_eq!(f.held(client).0, before, "no handle a hostile reply brought is left");
}

/// A server that dies holding the call: the caller sees `Disconnected`, holds no handle it did not
/// have, and its lend is whole for the next call.
#[test]
fn a_server_dying_mid_call_is_disconnected() {
    let f = fake();
    let dying = f.process(0, &[]);
    let receive = f.endpoint(dying);
    let client = f.process(1001, &[]);
    let conn = f.grant(dying, receive, client, 1);
    let server = f.run(dying, move || {
        let endpoint = Endpoint::from_handle(receive);
        let Ok(Event::Call(request)) = endpoint.receive(FOREVER, 0) else { return 1 };
        // It dies holding the call: no reply ever comes (a drop would send one), and its
        // endpoint goes with it.
        std::mem::forget(request);
        fake().destroy(dying, receive);
        0
    });
    let before = f.held(client).0;
    f.as_process(client, || {
        let mut lend = Lend::new(1).unwrap();
        let sign = Message::SignRecord(SignRecord { record: b"r" });
        let to = Endpoint::from_handle(conn);
        assert_eq!(
            typed::call::<keyd::Protocol, _>(&to, &mut lend, &sign, &[], |_, _| ()),
            Err(Error::Disconnected)
        );
        assert_eq!(
            typed::call::<keyd::Protocol, _>(&to, &mut lend, &sign, &[], |_, _| ()),
            Err(Error::Disconnected)
        );
    });
    assert_eq!(server.join().unwrap(), 0);
    assert_eq!(f.held(client).0, before);
}
