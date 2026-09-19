//! Regressions from the WP-R1 red team that need the fake kernel: handle leaks between client
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
            let reply = ep.call(&words, &[a.handle(), b.handle()], None, FOREVER).unwrap();
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
        let reply = Endpoint::from_handle(conn).call(&[0; 4], &[], None, FOREVER).unwrap();
        assert_eq!(reply.words, [1, 0, 0, 0]);
        0
    });
    assert_eq!(code.join().unwrap(), 0);
    assert_eq!(server_thread.join().unwrap(), 0);
}
