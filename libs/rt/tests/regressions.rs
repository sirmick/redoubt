//! Red-team regressions that need the fake kernel: handle leaks between client
//! and server, and a reply that cannot be encoded.

mod common;

use common::fake;
use redoubt_rt::abi::{Error, FOREVER, Handle, Handles};
use redoubt_rt::client::{Client, ClientError};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Caller, Event, Words};
use redoubt_rt::server::ninep::WORDS_9P;
use redoubt_rt::server::typed::{Answer, Protocol, TypedServer, serve_call};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::example::{ErrorCode, Grant, GrantReply, Message, Reply};

#[test]
fn the_9p_client_closes_handles_a_hostile_server_sends() {
    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(1001, &[]));
    let receive = f.endpoint(server);
    let conn = f.grant(server, receive, client, 1);
    let server_thread = f.run(server, move || {
        let ep = Endpoint::from_handle(receive);
        for _ in 0..20 {
            let Ok(Event::Call(request)) = ep.receive(FOREVER, 0) else { return 1 };
            // Four handles the client never asked for, with 9P's words.
            let junk: Vec<Endpoint> = (0..4).map(|_| Endpoint::create().unwrap()).collect();
            let handles: Vec<Handle> = junk.iter().map(Endpoint::handle).collect();
            request.reply(&WORDS_9P, &handles).unwrap();
            junk.into_iter().for_each(|e| e.close().unwrap());
        }
        0
    });
    let before = f.held(client).0;
    let code = f.run(client, move || {
        let mut c = Client::new(Endpoint::from_handle(conn), 1).unwrap();
        for _ in 0..20 {
            assert_eq!(c.attach(0, "").err(), Some(ClientError::Unexpected));
        }
        0
    });
    assert_eq!(code.join().unwrap(), 0);
    assert_eq!(server_thread.join().unwrap(), 0);
    assert_eq!(f.held(client).0, before, "no handle stayed in the client's table");
}

struct Example;

impl Protocol for Example {
    type Error = ErrorCode;
    type Reply<'a> = Reply<'a>;
    type Request<'a> = Message<'a>;

    fn decode<'a>(words: &Words, buf: &'a [u8], handles: usize) -> Result<Message<'a>, WireError> {
        Message::decode(words, buf, handles)
    }

    fn encode_reply(reply: &Reply<'_>, buf: &mut [u8]) -> Result<Words, WireError> { reply.encode(buf) }

    fn error_words(error: ErrorCode) -> Words { error.encode() }
}

/// Answers `grant` with a connection minted for the caller: a copy goes to the caller, and ours
/// must not stay behind.
struct Minter;

impl TypedServer<Example> for Minter {
    fn handle<'s>(
        &'s mut self,
        _: &Caller,
        request: Message<'_>,
        handles: &[Handle],
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        let Message::Grant(Grant { .. }) = request else { return Err(ErrorCode::NotFound) };
        // The request's two handles are ours now; close them, and hand back a fresh endpoint.
        handles.iter().for_each(|h| redoubt_rt::handle::close(*h).unwrap());
        let fresh = Endpoint::create().map_err(|_| ErrorCode::Denied)?;
        let handles = Handles::from_slice(&[fresh.handle()]).unwrap();
        Ok(Answer { reply: Reply::Grant(GrantReply {}), handles, close_after_reply: true })
    }
}

#[test]
fn typed_replies_close_the_handles_made_for_the_caller() {
    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(1001, &[]));
    let receive = f.endpoint(server);
    let conn = f.grant(server, receive, client, 1);
    let before = f.held(server).0;
    let server_thread = f.run(server, move || {
        let ep = Endpoint::from_handle(receive);
        for _ in 0..10 {
            let Ok(Event::Call(request)) = ep.receive(FOREVER, 0) else { return 1 };
            serve_call::<Example, _>(&mut Minter, request).unwrap();
        }
        0
    });
    let code = f.run(client, move || {
        let ep = Endpoint::from_handle(conn);
        for _ in 0..10 {
            let (a, b) = (Endpoint::create().unwrap(), Endpoint::create().unwrap());
            let words = Message::Grant(Grant { pages: 1 }).encode(&mut []).unwrap();
            let reply = ep.call(&words, &[a.handle(), b.handle()], None, FOREVER).into_result().unwrap().0;
            assert_eq!(reply.words, [0; 4]);
            reply.handles.as_slice().iter().flatten().for_each(|h| redoubt_rt::handle::close(*h).unwrap());
            a.close().unwrap();
            b.close().unwrap();
        }
        0
    });
    assert_eq!(code.join().unwrap(), 0);
    assert_eq!(server_thread.join().unwrap(), 0);
    assert_eq!(f.held(server).0, before, "one handle per reply used to stay in the server");
}

#[test]
fn a_reply_that_cannot_be_encoded_gives_the_request_back() {
    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(1001, &[]));
    let receive = f.endpoint(server);
    let conn = f.grant(server, receive, client, 1);
    let server_thread = f.run(server, move || {
        let ep = Endpoint::from_handle(receive);
        let Ok(Event::Call(request)) = ep.receive(FOREVER, 0) else { return 1 };
        // Five handles do not fit a message: refused before anything is sent...
        let five = [receive; 5];
        let (error, request) = request.reply(&[0; 4], &five).unwrap_err();
        assert_eq!(error, Error::TooLarge);
        // ... and the server can still answer.
        request.reply(&[1, 0, 0, 0], &[]).unwrap();
        0
    });
    let code = f.run(client, move || {
        let reply = Endpoint::from_handle(conn).call(&[0; 4], &[], None, FOREVER).into_result().unwrap().0;
        assert_eq!(reply.words, [1, 0, 0, 0]);
        0
    });
    assert_eq!(code.join().unwrap(), 0);
    assert_eq!(server_thread.join().unwrap(), 0);
}

/// A 9P call the skeleton hands back to be parked has already had whatever handles it
/// brought closed, and its own list emptied with them, so serving it a second time closes
/// nothing. Without that, the second serving closes the same table indices — which by then name
/// whatever the server has opened since.
#[test]
fn a_held_9p_call_closes_what_it_brought_exactly_once() {
    use redoubt_rt::server::Limits;
    use redoubt_rt::server::ninep::{FileServer, FileStat, NineError, NineServer, Qid, Read, WORDS_9P, mode};
    use redoubt_rt::wire::ninep::{Body, Message as NineP, NOFID};

    /// One file, which waits for a read until `ready`.
    struct WaitOnce {
        ready: bool,
    }

    impl FileServer for WaitOnce {
        type Node = ();

        fn attach(&mut self, _: &Caller, _: &str) -> Result<((), Qid), NineError> {
            Ok(((), Qid { kind: 0, version: 0, path: 0 }))
        }

        fn labels(&self, _: &()) -> &[u64] { &[] }

        fn walk(&mut self, _: &Caller, _: &(), _: &str) -> Result<((), Qid), NineError> {
            Err(NineError::NOT_DIR)
        }

        fn open(&mut self, _: &Caller, _: &(), _: u8) -> Result<Qid, NineError> {
            Ok(Qid { kind: 0, version: 0, path: 0 })
        }

        fn read(&mut self, _: &Caller, _: &(), _: u64, out: &mut [u8]) -> Result<Read, NineError> {
            if !self.ready {
                return Ok(Read::Wait);
            }
            out.first_mut().map(|b| *b = b'!');
            Ok(Read::Done(1))
        }

        fn write(&mut self, _: &Caller, _: &(), _: u64, d: &[u8]) -> Result<usize, NineError> { Ok(d.len()) }

        fn stat(&mut self, _: &Caller, _: &()) -> Result<FileStat, NineError> { Ok(FileStat::default()) }

        fn dir_entry(&mut self, _: &Caller, _: &(), _: u64) -> Result<Option<((), FileStat)>, NineError> {
            Ok(None)
        }
    }

    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(1001, &[]));
    let receive = f.endpoint(server);
    let conn = f.grant(server, receive, client, 1);
    // A handle the client sends with its 9P read. 9P carries none, so the skeleton closes it.
    let spare = f.endpoint(client);

    let server_thread = f.run(server, move || {
        let ep = Endpoint::from_handle(receive);
        let limits = Limits { buckets: 2, in_flight: 2, files: 4, state: 0 };
        let mut nine = NineServer::new(WaitOnce { ready: false }, limits, 9).unwrap();
        let own = |_: &mut NineServer<WaitOnce>, r: redoubt_rt::ipc::Request| {
            r.reply(&[1, 0, 0, 0], &[]).map(|_| ()).map_err(|(e, _)| e)
        };
        let mut verdict = 0;
        while let Ok(event) = ep.receive(FOREVER, 0) {
            let Event::Call(request) = event else { continue };
            let Ok(Some(held)) = nine.serve_parking(request, own) else { continue };
            // The client's handle is already closed, so the next handle this server opens takes
            // its index; if the second serving closed that index too, the count would drop.
            let mine = Endpoint::create().unwrap();
            let before = f.held(server).0;
            nine.fs.ready = true;
            let _ = nine.serve_parking(held, own);
            verdict = u32::from(f.held(server).0 == before && mine.close().is_ok());
        }
        verdict
    });

    let read = f.run(client, move || {
        let mut buf = Some(redoubt_rt::ipc::Buffer::new(1).unwrap());
        let ep = Endpoint::from_handle(conn);
        let mut rpc = |body: Body<'_>, handles: &[Handle]| {
            NineP { tag: 3, body }.encode(buf.as_mut().unwrap()).unwrap();
            let (reply, returned) =
                ep.call(&WORDS_9P, handles, buf.take(), FOREVER).into_result().expect("the call");
            buf = returned;
            assert_eq!(reply.words, WORDS_9P);
            NineP::decode(buf.as_ref().unwrap()).unwrap().body.kind()
        };
        assert_eq!(rpc(Body::Tattach { fid: 0, afid: NOFID, uname: "", aname: "" }, &[]), 105);
        assert_eq!(rpc(Body::Topen { fid: 0, mode: mode::OREAD }, &[]), 113);
        // The read waits, is handed back, and is served again: `Rread`.
        u32::from(rpc(Body::Tread { fid: 0, offset: 0, count: 8 }, &[spare]))
    });
    assert_eq!(read.join().unwrap(), 117, "the held read was answered with an Rread");
    f.destroy(server, receive);
    assert_eq!(server_thread.join().unwrap(), 1, "the second serving closed a handle it did not own");
}

/// A file server that asks to wait (`Read::Wait`) and is served through a plain `serve` — not
/// `serve_parking` — is **refused**, not left hanging (servers/serving.md, "The 9P server
/// skeleton"). The skeleton has nowhere to put a held call, so the caller gets a status-1 refusal
/// instead of a reply that never comes; only a server that serves through `serve_parking` may
/// wait.
#[test]
fn a_wait_without_serve_parking_is_refused_not_stranded() {
    use redoubt_rt::server::Limits;
    use redoubt_rt::server::ninep::{FileServer, FileStat, NineError, NineServer, Qid, Read, WORDS_9P, mode};
    use redoubt_rt::wire::ninep::{Body, Message as NineP, NOFID};

    /// Every read waits, and nothing ever makes it ready: the server never parks, so a `serve`
    /// must refuse rather than hold the caller.
    struct AlwaysWaits;

    impl FileServer for AlwaysWaits {
        type Node = ();

        fn attach(&mut self, _: &Caller, _: &str) -> Result<((), Qid), NineError> {
            Ok(((), Qid { kind: 0, version: 0, path: 0 }))
        }

        fn labels(&self, _: &()) -> &[u64] { &[] }

        fn walk(&mut self, _: &Caller, _: &(), _: &str) -> Result<((), Qid), NineError> {
            Err(NineError::NOT_DIR)
        }

        fn open(&mut self, _: &Caller, _: &(), _: u8) -> Result<Qid, NineError> {
            Ok(Qid { kind: 0, version: 0, path: 0 })
        }

        fn read(&mut self, _: &Caller, _: &(), _: u64, _: &mut [u8]) -> Result<Read, NineError> {
            Ok(Read::Wait)
        }

        fn write(&mut self, _: &Caller, _: &(), _: u64, d: &[u8]) -> Result<usize, NineError> { Ok(d.len()) }

        fn stat(&mut self, _: &Caller, _: &()) -> Result<FileStat, NineError> { Ok(FileStat::default()) }

        fn dir_entry(&mut self, _: &Caller, _: &(), _: u64) -> Result<Option<((), FileStat)>, NineError> {
            Ok(None)
        }
    }

    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(1001, &[]));
    let receive = f.endpoint(server);
    let conn = f.grant(server, receive, client, 1);

    let server_thread = f.run(server, move || {
        let ep = Endpoint::from_handle(receive);
        let limits = Limits { buckets: 2, in_flight: 2, files: 4, state: 0 };
        let mut nine = NineServer::new(AlwaysWaits, limits, 9).unwrap();
        let own = |_: &mut NineServer<AlwaysWaits>, r: redoubt_rt::ipc::Request| {
            r.reply(&[1, 0, 0, 0], &[]).map(|_| ()).map_err(|(e, _)| e)
        };
        let mut answered = 0;
        while let Ok(event) = ep.receive(FOREVER, 0) {
            let Event::Call(request) = event else { continue };
            // A plain `serve`: it must answer every call, so the waiting read is refused here.
            if nine.serve_with(request, own).is_ok() {
                answered += 1;
            }
        }
        answered
    });

    let verdict = f.run(client, move || {
        let mut buf = Some(redoubt_rt::ipc::Buffer::new(1).unwrap());
        let ep = Endpoint::from_handle(conn);
        let mut rpc = |body: Body<'_>| -> redoubt_rt::ipc::Words {
            NineP { tag: 3, body }.encode(buf.as_mut().unwrap()).unwrap();
            let (reply, returned) =
                ep.call(&WORDS_9P, &[], buf.take(), FOREVER).into_result().expect("the call");
            buf = returned;
            reply.words
        };
        assert_eq!(rpc(Body::Tattach { fid: 0, afid: NOFID, uname: "", aname: "" }), [0u64; 4]);
        assert_eq!(rpc(Body::Topen { fid: 0, mode: mode::OREAD }), [0u64; 4]);
        // The waiting read is refused: status 1, not a hang and not an Rread.
        let refused = rpc(Body::Tread { fid: 0, offset: 0, count: 8 });
        u32::from(refused == [1u64, 0, 0, 0])
    });

    assert_eq!(
        verdict.join().unwrap(),
        1,
        "a `Wait` served through a plain `serve` is refused with status 1, not stranded"
    );
    f.destroy(server, receive);
    assert_eq!(server_thread.join().unwrap(), 3, "every call was answered");
}
