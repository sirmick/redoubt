//! A lend's and a transfer's pages are given up for the message (docs/kernel/ipc.md, Messages):
//! the fake kernel refuses every call of the giver's that names them, as the kernel does, until
//! the message gives them back (refused or timed out while queued, or replied to), and for good
//! once a transfer is taken or a taken call is abandoned.

use std::num::NonZeroUsize;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use redoubt_fake_kernel::{answer, fake};
use redoubt_rt::Transport;
use redoubt_rt::abi::{
    BODY_SLOTS, Body, Call, CallOutcome, Error, FOREVER, Handle, Handles, LendDisposition, MemFlags,
    PAGE_SIZE, Pages, Return, WORDS,
};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Buffer, Event};

const REFUSED: Result<(), Error> = Err(Error::InvalidArgument);

/// An empty body record.
fn empty_body() -> [u64; BODY_SLOTS] { Body { words: [0; WORDS], handles: Handles::new() }.encode() }

/// One page at `addr`.
fn page(addr: usize) -> Pages { Pages { addr, npages: NonZeroUsize::MIN } }

/// `call` with its body record at `body_rec`, straight to the fake: the status alone.
fn call(endpoint: Handle, body_rec: usize, lend: Option<Pages>) -> Result<(), Error> {
    match fake().call(&Call::Call { endpoint, body_rec, lend, timeout: FOREVER }) {
        Ok(Return::Call(CallOutcome { status, .. })) => status,
        other => panic!("call returned {other:?}"),
    }
}

/// `send` with a fresh body record, straight to the fake.
fn send(endpoint: Handle, transfer: Pages) -> Result<(), Error> {
    let rec = empty_body();
    let send = Call::Send { endpoint, body_rec: rec.as_ptr() as usize, transfer: Some(transfer), timeout: 0 };
    fake().call(&send).map(drop)
}

/// `receive` from nothing with its record at `received_rec`: `Timeout` once the record passes.
fn receive(received_rec: usize) -> Result<(), Error> {
    fake().call(&Call::Receive { from: None, timeout: 0, max_transfer: 0, received_rec }).map(drop)
}

fn unmap(pages: Pages) -> Result<(), Error> {
    fake().call(&Call::Unmap { addr: pages.addr, len: PAGE_SIZE }).map(drop)
}

#[test]
fn a_lent_page_is_refused_to_every_call_until_the_call_returns() {
    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(1001, &[]));
    let receive_right = f.endpoint(server);
    let badged = f.grant(server, receive_right, client, 7);
    let (budget, exits) = (f.budget(client), f.endpoint(client));
    let (mut lend, child) = f.as_process(client, || {
        let child = f.call(&Call::ProcessCreate { budget, exit_endpoint: exits });
        (Buffer::new(1).unwrap(), child)
    });
    let Ok(Return::Handle(child)) = child else { panic!("process_create: {child:?}") };
    let lent = page(lend.as_ptr() as usize);
    // A body record at the page's start, for the calls below whose record lies in it.
    for (bytes, slot) in lend.chunks_exact_mut(8).zip(empty_body()) {
        bytes.copy_from_slice(&slot.to_le_bytes());
    }
    let (body_in_it, received_in_it) = (lent.addr, lent.addr + BODY_SLOTS * 8);
    let start = move |handles_rec| Call::ProcessStart {
        process: child,
        entry: 0,
        sp: 0,
        arg: 0,
        handles_rec,
        count: 0,
    };

    let server_thread = f.run(server, move || {
        let endpoint = Endpoint::from_handle(receive_right);
        let Ok(Event::Call(request)) = endpoint.receive(FOREVER, 0) else { return 1 };
        // The call is open: its caller has given the page up, and every call naming it fails.
        let refused = f.as_process(client, || {
            let mut rec = empty_body();
            let map = Call::ProcessMap {
                process: child,
                src: lent.addr,
                dst: 0x1000,
                len: PAGE_SIZE,
                flags: MemFlags::READ,
            };
            [
                call(badged, rec.as_mut_ptr() as usize, Some(lent)),
                send(badged, lent),
                unmap(lent),
                f.call(&map).map(drop),
                call(badged, body_in_it, None),
                receive(received_in_it),
                f.call(&start(received_in_it)).map(drop),
            ]
        });
        if refused != [REFUSED; 7] {
            eprintln!("while the call was open: {refused:?}");
            return 2;
        }
        if answer(request, [0; WORDS]).is_err() {
            return 3;
        }
        // The lend of the page the caller makes next, with its body inside it.
        let Ok(Event::Call(request)) = endpoint.receive(FOREVER, 0) else { return 4 };
        answer(request, [0; WORDS]).map_or(5, |_| 0)
    });
    let client_thread = f.run(client, move || {
        let returned =
            Endpoint::from_handle(badged).call(&[0; WORDS], &[], Some(lend), FOREVER).into_result();
        let Ok((_, Some(lend))) = returned else { return 1 };
        // Returned, the page is the caller's again: the same calls pass.
        let accepted = [
            call(badged, body_in_it, Some(lent)),
            receive(received_in_it).or_else(|e| if e == Error::Timeout { Ok(()) } else { Err(e) }),
            f.call(&start(received_in_it)).map(drop),
            unmap(lent),
        ];
        // Unmapped just above.
        std::mem::forget(lend);
        if accepted != [Ok(()); 4] {
            eprintln!("after the return: {accepted:?}");
            return 2;
        }
        0
    });
    assert_eq!(client_thread.join().unwrap(), 0);
    assert_eq!(server_thread.join().unwrap(), 0);
}

#[test]
fn a_transferred_page_is_refused_to_its_sender_once_taken() {
    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(1001, &[]));
    let receive_right = f.endpoint(server);
    let badged = f.grant(server, receive_right, client, 7);
    let transfer = f.as_process(client, || Buffer::new(1)).unwrap();
    let sent = page(transfer.as_ptr() as usize);

    let server_thread = f.run(server, move || {
        let Ok(Event::Send(delivery)) = Endpoint::from_handle(receive_right).receive(FOREVER, 1) else {
            return 1;
        };
        // Taken: the page is the receiver's, and its sender may name it in nothing.
        let refused = f.as_process(client, || {
            let mut rec = empty_body();
            [call(badged, rec.as_mut_ptr() as usize, Some(sent)), send(badged, sent), unmap(sent)]
        });
        // The receiver's to unmap.
        drop(delivery);
        if refused != [REFUSED; 3] {
            eprintln!("after the take: {refused:?}");
            return 2;
        }
        0
    });
    let client_thread = f.run(client, move || {
        Endpoint::from_handle(badged).send(&[0; WORDS], &[], Some(transfer), FOREVER).map_or(1, |()| 0)
    });
    assert_eq!(client_thread.join().unwrap(), 0);
    assert_eq!(server_thread.join().unwrap(), 0);
}

#[test]
fn an_abandoned_lend_is_refused_to_its_caller_until_the_server_replies() {
    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(1001, &[]));
    let receive_right = f.endpoint(server);
    let badged = f.grant(server, receive_right, client, 7);
    let (budget, exits) = (f.budget(client), f.endpoint(client));
    let (lend, child) = f.as_process(client, || {
        let child = f.call(&Call::ProcessCreate { budget, exit_endpoint: exits });
        (Buffer::new(1).unwrap(), child)
    });
    let Ok(Return::Handle(child)) = child else { panic!("process_create: {child:?}") };
    let lent = page(lend.as_ptr() as usize);
    let (checked, replied) = mpsc::channel();

    let server_thread = f.run(server, move || {
        let Ok(Event::Call(request)) = Endpoint::from_handle(receive_right).receive(FOREVER, 0) else {
            return 1;
        };
        // Hold the call until its caller has timed out and tried the lend.
        replied.recv().unwrap();
        // The reply reaches nobody, and frees the lend (R3).
        match answer(request, [0; WORDS]) {
            Ok(outcome) if !outcome.delivered => 0,
            _ => 2,
        }
    });
    let client_thread = f.run(client, move || {
        // Long enough for the server to take the call first.
        let outcome = Endpoint::from_handle(badged).call(&[0; WORDS], &[], Some(lend), 1_000_000);
        if outcome.status != Err(Error::Timeout) || outcome.buffer.is_some() {
            return 1;
        }
        // Consumed: the server's until its reply, and nothing of the caller's to name.
        let mut rec = empty_body();
        let map = Call::ProcessMap {
            process: child,
            src: lent.addr,
            dst: 0x1000,
            len: PAGE_SIZE,
            flags: MemFlags::READ,
        };
        let refused =
            [unmap(lent), call(badged, rec.as_mut_ptr() as usize, Some(lent)), f.call(&map).map(drop)];
        checked.send(()).unwrap();
        if refused != [REFUSED; 3] {
            eprintln!("after the abandonment: {refused:?}");
            return 2;
        }
        0
    });
    assert_eq!(client_thread.join().unwrap(), 0);
    assert_eq!(server_thread.join().unwrap(), 0);
    assert_eq!(f.held(server).1, 0, "the server's reply freed the lend");
}

/// Waits until `n` messages are queued on the endpoint `owner`'s `handle` names.
fn until_queued(owner: usize, handle: Handle, n: usize) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while fake().queued(owner, handle) != n {
        assert!(Instant::now() < deadline, "the message was never queued");
        std::thread::yield_now();
    }
}

#[test]
fn queued_pages_are_refused_until_the_message_times_out_and_gives_them_back() {
    let f = fake();
    // `idle` owns an endpoint nobody receives on; `client` sends there, and serves `peer`.
    let (idle, client, peer) = (f.process(0, &[]), f.process(1001, &[]), f.process(1002, &[]));
    let nowhere = f.endpoint(idle);
    let toward = f.grant(idle, nowhere, client, 7);
    let served = f.endpoint(client);
    let from_peer = f.grant(client, served, peer, 9);
    // Held by its address alone, and unmapped at the end.
    let given = f.as_process(client, || {
        let buffer = Buffer::new(1).unwrap();
        let pages = page(buffer.as_ptr() as usize);
        std::mem::forget(buffer);
        pages
    });

    // `peer`'s call, which `client` holds open to try a reply whose record is in the page.
    let peer_thread = f.run(peer, move || {
        Endpoint::from_handle(from_peer).call(&[0; WORDS], &[], None, FOREVER).into_result().map_or(1, |_| 0)
    });
    let Ok(Event::Call(request)) = f.as_process(client, || Endpoint::from_handle(served).receive(FOREVER, 0))
    else {
        panic!("no call from the peer")
    };
    let open = request.id();

    // A lending call, then a transferring send, each queued where nobody takes it until it times
    // out: long enough for the checks between.
    for is_call in [true, false] {
        let sender = f.run(client, move || {
            let rec = empty_body();
            let (body_rec, timeout) = (rec.as_ptr() as usize, 2_000_000);
            if is_call {
                let lend = Call::Call { endpoint: toward, body_rec, lend: Some(given), timeout };
                match fake().call(&lend) {
                    Ok(Return::Call(CallOutcome {
                        status: Err(Error::Timeout),
                        lend: LendDisposition::Returned,
                        ..
                    })) => 0,
                    _ => 1,
                }
            } else {
                let send = Call::Send { endpoint: toward, body_rec, transfer: Some(given), timeout };
                if fake().call(&send) == Err(Error::Timeout) { 0 } else { 1 }
            }
        });
        until_queued(idle, nowhere, 1);
        // Queued: the page is still mapped in its sender, and given up all the same.
        let refused = f.as_process(client, || {
            let mut rec = empty_body();
            [
                unmap(given),
                call(toward, rec.as_mut_ptr() as usize, Some(given)),
                f.call(&Call::Reply { msg_id: open, body_rec: given.addr }).map(drop),
            ]
        });
        assert_eq!(refused, [REFUSED; 3], "while queued");
        assert_eq!(sender.join().unwrap(), 0);
    }

    // Timed out while queued, the page came back: the reply and the unmap pass.
    f.as_process(client, || {
        assert!(answer(request, [0; WORDS]).is_ok());
        assert_eq!(unmap(given), Ok(()));
    });
    assert_eq!(peer_thread.join().unwrap(), 0);
}

#[test]
fn a_destroyed_endpoint_abandons_a_taken_lend_to_the_server_until_it_replies() {
    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(1001, &[]));
    let receive_right = f.endpoint(server);
    let badged = f.grant(server, receive_right, client, 7);
    let lend = f.as_process(client, || Buffer::new(1)).unwrap();
    let lent = page(lend.as_ptr() as usize);
    let (checked, replied) = mpsc::channel();

    let server_thread = f.run(server, move || {
        let Ok(Event::Call(request)) = Endpoint::from_handle(receive_right).receive(FOREVER, 0) else {
            return 1;
        };
        f.destroy(server, receive_right);
        // Hold the call until its caller has seen `Dead` and tried the lend.
        replied.recv().unwrap();
        // The reply reaches nobody, and frees the lend (R3).
        match answer(request, [0; WORDS]) {
            Ok(outcome) if !outcome.delivered => 0,
            _ => 2,
        }
    });
    let client_thread = f.run(client, move || {
        let outcome = Endpoint::from_handle(badged).call(&[0; WORDS], &[], Some(lend), FOREVER);
        // Taken, then abandoned: the lend is consumed, not returned.
        if outcome.status != Err(Error::Dead) || outcome.buffer.is_some() {
            return 1;
        }
        let mut rec = empty_body();
        let refused = [unmap(lent), call(badged, rec.as_mut_ptr() as usize, Some(lent))];
        checked.send(()).unwrap();
        if refused != [REFUSED; 2] {
            eprintln!("after the abandonment: {refused:?}");
            return 2;
        }
        0
    });
    assert_eq!(client_thread.join().unwrap(), 0);
    assert_eq!(server_thread.join().unwrap(), 0);
    assert_eq!(f.held(server).1, 0, "the server's reply freed the lend");
}
