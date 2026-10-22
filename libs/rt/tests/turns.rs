//! A server's turn against the fake kernel (kernel/ipc.md, "A server's turn"): a held reply is
//! made with the loop's next receive as one call, a turn refused before anything is done still
//! answers the call, and a send that waits for an interrupt hands back its pages when the send
//! fails.

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{Error, FOREVER, Handles};
use redoubt_rt::handle::{Endpoint, Irq};
use redoubt_rt::ipc::{Buffer, Event, Sent};
use redoubt_rt::server::serve;
use redoubt_rt::server::typed::{Outcome, finish_held};

/// The handler holds each handle-free answer: the first call is taken by a plain receive, every
/// later one by the turn that made the answer before it, and no reply is made alone.
#[test]
fn a_held_reply_is_made_with_the_next_receive() {
    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(5, &[]));
    let receive = f.endpoint(server);
    let badged = f.grant(server, receive, client, 1);

    let server_thread = f.run(server, move || {
        serve(&Endpoint::from_handle(receive), |request| {
            let words = [request.words[0] + 1, 0, 0, 0];
            finish_held(request, &Outcome { words, send: Handles::new(), close: Handles::new() })
        })
    });
    f.as_process(client, || {
        let ep = Endpoint::from_handle(badged);
        for n in 1..=3 {
            assert_eq!(
                ep.call(&[n, 0, 0, 0], &[], None, FOREVER).into_result().unwrap().0.words,
                [n + 1, 0, 0, 0]
            );
        }
    });
    f.destroy(server, receive);
    server_thread.join().unwrap();
    // The last turn answers the third call and finds the endpoint dead.
    assert_eq!(f.calls(server), ["receive", "reply_receive", "reply_receive", "reply_receive"]);
}

/// A turn the kernel refuses before anything is done leaves the call open: the loop answers it
/// alone, then receives alone, and the caller is answered as before.
#[test]
fn a_refused_turn_still_answers_the_call() {
    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(5, &[]));
    let receive = f.endpoint(server);
    let badged = f.grant(server, receive, client, 1);
    f.refuse(server, "reply_receive", Error::Busy);

    let server_thread = f.run(server, move || {
        serve(&Endpoint::from_handle(receive), |request| {
            let words = [request.words[0] + 1, 0, 0, 0];
            finish_held(request, &Outcome { words, send: Handles::new(), close: Handles::new() })
        })
    });
    f.as_process(client, || {
        let ep = Endpoint::from_handle(badged);
        assert_eq!(ep.call(&[1, 0, 0, 0], &[], None, FOREVER).into_result().unwrap().0.words, [2, 0, 0, 0]);
        assert_eq!(ep.call(&[2, 0, 0, 0], &[], None, FOREVER).into_result().unwrap().0.words, [3, 0, 0, 0]);
    });
    f.destroy(server, receive);
    server_thread.join().unwrap();
    assert_eq!(f.calls(server), ["receive", "reply_receive", "reply", "receive", "reply_receive"]);
    assert_eq!(f.open_calls(server), 0, "no call is left open");
}

/// The message is taken, and then the interrupt answers the wait the same call made.
#[test]
fn a_send_taken_then_waits_for_the_interrupt() {
    let f = fake();
    let (driver, taker) = (f.process(0, &[]), f.process(0, &[]));
    let receive = f.endpoint(taker);
    let to = f.grant(taker, receive, driver, 1);
    let (_mmio, irq) = f.device(driver, 4096);

    let driver_thread = f.run(driver, move || {
        match Endpoint::from_handle(to).send_then_wait(
            &[9, 0, 0, 0],
            &[],
            None,
            FOREVER,
            &Irq::from_handle(irq),
            FOREVER,
        ) {
            Sent::Taken(Ok(())) => 0,
            other => panic!("{other:?}"),
        }
    });
    f.as_process(taker, || match Endpoint::from_handle(receive).receive(FOREVER, 0) {
        Ok(Event::Send(delivery)) => assert_eq!(delivery.words, [9, 0, 0, 0]),
        other => panic!("{other:?}"),
    });
    f.fire(driver, irq);
    assert_eq!(driver_thread.join().unwrap(), 0);
    assert_eq!(f.calls(driver), ["send_receive"]);
}

/// A send that fails waits for nothing, and its transfer is the sender's again.
#[test]
fn a_failed_send_gives_its_pages_back_and_waits_for_nothing() {
    let f = fake();
    let (driver, taker) = (f.process(0, &[]), f.process(0, &[]));
    let receive = f.endpoint(taker);
    let to = f.grant(taker, receive, driver, 1);
    let (_mmio, irq) = f.device(driver, 4096);
    f.destroy(taker, receive);

    let driver_thread = f.run(driver, move || {
        let pages = Buffer::new(1).unwrap();
        match Endpoint::from_handle(to).send_then_wait(
            &[9, 0, 0, 0],
            &[],
            Some(pages),
            FOREVER,
            &Irq::from_handle(irq),
            FOREVER,
        ) {
            Sent::Not(Error::Dead, Some(pages)) => pages.npages() as u32 - 1,
            other => panic!("{other:?}"),
        }
    });
    assert_eq!(driver_thread.join().unwrap(), 0, "the page is back");
    // The page is unmapped when the buffer handed back is dropped.
    assert_eq!(f.calls(driver), ["map_anon", "send_receive", "unmap"]);
}
