//! Parking a typed call on the fake kernel (servers/serving.md, "Parking a typed call"): a
//! request the server says waits comes back unanswered with its lend intact, is parked, and is
//! answered later, by `reply` or by serving it again; one abandoned is freed and its admission
//! released, so the caller parks again.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use redoubt_fake_kernel::{answer, fake};
use redoubt_rt::abi::{Error, FOREVER, Handle};
use redoubt_rt::client::Lend;
use redoubt_rt::handle::{self, Endpoint};
use redoubt_rt::ipc::{Caller, Event, Words};
use redoubt_rt::server::parked::Parked;
use redoubt_rt::server::typed::{self, Answer, Protocol, TypedServer};
use redoubt_rt::server::{Admission, AdmitKey, Limits};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::example::{ErrorCode, Message, Named, NamedReply, Reply, Small, SmallReply};

/// Waits for `cond`, polling; fails naming `step` if it does not come.
fn until(step: &str, mut cond: impl FnMut() -> bool) {
    const TRIES: u32 = 20_000;
    for _ in 0..TRIES {
        if cond() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("{step}: not reached in {TRIES} polls");
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

/// `small` with `a` 0 waits, and `named` waits while `holding`; anything else is answered.
struct Waiter {
    holding: bool,
}

impl TypedServer<Example> for Waiter {
    fn handle<'s>(
        &'s mut self,
        caller: &Caller,
        request: Message<'_>,
        _: &[Handle],
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        match request {
            Message::Small(Small { a, b }) => {
                Ok(Answer::new(Reply::Small(SmallReply { c: u32::from(a) + u32::from(b) })))
            }
            Message::Named(Named { id, .. }) => {
                Ok(Answer::new(Reply::Named(NamedReply { id: id ^ caller.account as u32 })))
            }
            _ => Err(ErrorCode::NotFound),
        }
    }

    fn waits(&mut self, _: &Caller, request: &Message<'_>) -> bool {
        match request {
            Message::Small(Small { a, .. }) => *a == 0,
            Message::Named(_) => self.holding,
            _ => false,
        }
    }
}

/// What the parked call waits as: answered by `reply`, or served again.
#[derive(Clone, Copy, PartialEq)]
enum Held {
    Small,
    Named,
}

/// Serves until the endpoint goes. An answered `small` wakes every parked call: a parked `small`
/// is answered 42 by `reply`, a parked `named` is served again with `holding` off. Returns the
/// abandoned-call notices handled and the calls refused for want of admission.
fn serve(ep: Endpoint, abandoned: Arc<AtomicU32>) -> (u32, u32) {
    // One parked call a lone client's share: half of a bucket's 2.
    let mut admission =
        Admission::new(Limits { buckets: 4, in_flight: 2, files: 0, state: 0, requests: 0, pages: 0 })
            .unwrap();
    let mut parked: Parked<Held> = Parked::new(FOREVER);
    let mut server = Waiter { holding: true };
    let mut refused = 0;
    loop {
        match ep.receive(FOREVER, 1) {
            Ok(Event::Call(request)) => {
                let held = match Example::decode(&request.words, &[], 0) {
                    Ok(Message::Small(_)) => Held::Small,
                    _ => Held::Named,
                };
                let charge = (AdmitKey::of(&request.caller), request.caller.badge);
                match typed::serve_parking::<Example, _>(&mut server, request) {
                    Ok(Some(waiting)) => {
                        let now = handle::time_now().unwrap();
                        if let Err(back) = parked.park(&mut admission, waiting, charge, held, now) {
                            answer(back.0, [9, 0, 0, 0]).unwrap();
                            refused += 1;
                        }
                    }
                    // Answered: everything parked is woken.
                    Ok(None) => {
                        server.holding = false;
                        while let Some(call) = parked.resume_first(&mut admission, |_| true) {
                            let (request, held) = call.unwrap();
                            if held == Held::Small {
                                let reply = Reply::Small(SmallReply { c: 42 });
                                typed::reply::<Example>(request, &reply).unwrap();
                            } else {
                                // Read from its lend afresh: the request's buffer is as it came.
                                assert!(
                                    typed::serve_parking::<Example, _>(&mut server, request)
                                        .unwrap()
                                        .is_none()
                                );
                            }
                        }
                        server.holding = true;
                    }
                    Err(_) => {}
                }
            }
            Ok(Event::Abandoned(id)) => {
                abandoned.fetch_add(1, Ordering::Release);
                assert!(
                    parked.abandoned(&mut admission, id, &[0; 4]).is_some(),
                    "a notice for a call not parked"
                );
            }
            Ok(_) | Err(Error::Timeout) => {}
            Err(_) => return (abandoned.load(Ordering::Acquire), refused),
        }
    }
}

/// `named` through a page lend, its reply's id.
fn named(to: Handle, id: u32, timeout: u64) -> Result<u32, Error> {
    let mut lend = Lend::new(1).unwrap();
    let message = Message::Named(Named { id, name: "waits in its lend" });
    let words = message.encode(lend.pages().unwrap()).unwrap();
    let outcome = lend.call(&Endpoint::from_handle(to), &words, &[], timeout);
    let reply = outcome.into_result()?.0;
    match Reply::decode(5, &reply.words, lend.bytes(), 0) {
        Ok(Ok(Reply::Named(NamedReply { id }))) => Ok(id),
        other => panic!("not a named reply: {other:?}"),
    }
}

/// `small` with no lend: its reply's `c`, or the status words of a refusal.
fn small(to: Handle, a: u8, timeout: u64) -> Result<u64, Error> {
    let words = Message::Small(Small { a, b: 1 }).encode(&mut []).unwrap();
    let reply = Endpoint::from_handle(to).call(&words, &[], None, timeout).into_result()?.0;
    Ok(if reply.words[0] == 0 { reply.words[1] } else { 900 + reply.words[0] })
}

#[test]
fn a_waiting_typed_call_is_parked_answered_later_and_freed_when_abandoned() {
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let [a, b, c, d] = [1001, 1002, 1003, 1004].map(|account| f.process(account, &[]));
    let [ha, hb, hc, hd] =
        [(a, 1), (b, 2), (c, 3), (d, 4)].map(|(pid, badge)| f.grant(server, receive, pid, badge));
    let abandoned = Arc::new(AtomicU32::new(0));
    let seen = abandoned.clone();
    let server_thread = f.run(server, move || {
        let (abandoned, refused) = serve(Endpoint::from_handle(receive), seen);
        abandoned * 100 + refused
    });

    // A's `named` and B's `small` wait; C's answered `small` wakes both. A's is served again from
    // its own lend; B's is answered by `reply`.
    let waiter_a = f.run(a, move || named(ha, 0x10, FOREVER).unwrap());
    until("the server holds A's call", || f.open_calls(server) == 1);
    let waiter_b = f.run(b, move || small(hb, 0, FOREVER).unwrap() as u32);
    until("the server holds B's call", || f.open_calls(server) == 2);
    let waker = f.run(c, move || small(hc, 1, FOREVER).unwrap() as u32);
    assert_eq!(waker.join().unwrap(), 2, "C's own call is answered at once");
    assert_eq!(waiter_a.join().unwrap(), 0x10 ^ 1001, "A's request decoded again from its lend");
    assert_eq!(waiter_b.join().unwrap(), 42, "B's answered by reply");

    // D gives up on its parked call: it is freed, and its admission comes back, so D parks again
    // rather than being refused. A call that times out before the server takes it is withdrawn
    // unseen, so D calls again until the server has handled the notice.
    let mut tries = 0;
    while abandoned.load(Ordering::Acquire) == 0 {
        tries += 1;
        assert!(tries <= 100, "D's call was never taken before it timed out, in {tries} calls");
        let gave_up = f.run(d, move || u32::from(small(hd, 0, 100_000) == Err(Error::Timeout)));
        assert_eq!(gave_up.join().unwrap(), 1);
        until("the server answered D's abandoned call", || f.open_calls(server) == 0);
    }
    let again = f.run(d, move || small(hd, 0, FOREVER).unwrap() as u32);
    until("the server holds D's second call", || f.open_calls(server) == 1);
    // A second waiting call from D is over its share: answered at once, not parked.
    let over = f.run(d, move || small(hd, 0, FOREVER).unwrap() as u32);
    assert_eq!(over.join().unwrap(), 909, "the call over D's share is refused at once");
    let woken = f.run(c, move || small(hc, 1, FOREVER).unwrap() as u32);
    assert_eq!(woken.join().unwrap(), 2);
    assert_eq!(again.join().unwrap(), 42);

    f.destroy(server, receive);
    assert_eq!(server_thread.join().unwrap(), 101, "one abandoned call, one refused");
}
