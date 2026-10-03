//! `server::serve`, the one receive loop of a server that takes only calls, against the fake
//! kernel: calls reach the server, a send's handles are closed, notices do not end the loop, and
//! the endpoint's fate is the exit code.

use redoubt_fake_kernel::{answer, fake};
use redoubt_rt::abi::{Error, FOREVER};
use redoubt_rt::exit;
use redoubt_rt::handle::{Budget, Endpoint, Process};
use redoubt_rt::server::serve;

#[test]
fn serve_answers_calls_and_closes_what_a_send_brought() {
    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(5, &[]));
    let receive = f.endpoint(server);
    let badged = f.grant(server, receive, client, 1);
    let before = f.held(server).0;

    let server_thread = f
        .run(server, move || serve(&Endpoint::from_handle(receive), |request| answer(request, [7, 0, 0, 0])));
    f.as_process(client, || {
        let ep = Endpoint::from_handle(badged);
        assert_eq!(ep.call(&[1, 0, 0, 0], &[], None, FOREVER).into_result().unwrap().0.words, [7, 0, 0, 0]);
        let gift = Endpoint::create().unwrap();
        ep.send(&[2, 0, 0, 0], &[gift.handle(), gift.handle()], None, FOREVER).map_err(|(e, _)| e).unwrap();
        // The next call is taken after the send, so the send's handles are closed by now.
        assert_eq!(ep.call(&[3, 0, 0, 0], &[], None, FOREVER).into_result().unwrap().0.words, [7, 0, 0, 0]);
        gift.close().unwrap();
    });
    assert_eq!(f.held(server).0, before, "what the send brought was closed");
    f.destroy(server, receive);
    server_thread.join().unwrap();
}

/// An abandoned-call notice and an exit notice do not end the loop: a call the handler kept is
/// given up by its caller, a child of the server's exits, and the next call is still answered.
/// An endpoint's receive never returns an interrupt (here and in the kernel only an IRQ handle's
/// does), so there is none to deliver.
#[test]
fn serve_survives_abandoned_and_exit_notices() {
    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(5, &[]));
    let receive = f.endpoint(server);
    let badged = f.grant(server, receive, client, 1);
    let budget = f.budget(server);
    let child = f.as_process(server, || {
        Process::create(&Budget::from_handle(budget), &Endpoint::from_handle(receive)).unwrap().handle()
    });

    let server_thread = f.run(server, move || {
        // A call whose word 0 is 1 is kept unanswered until the loop ends; any other is answered.
        let mut kept = Vec::new();
        let code = serve(&Endpoint::from_handle(receive), |request| {
            if request.words[0] == 1 {
                kept.push(request);
            } else {
                let _ = answer(request, [7, 0, 0, 0]);
            }
        });
        drop(kept);
        code
    });
    f.as_process(client, || {
        let ep = Endpoint::from_handle(badged);
        assert_eq!(ep.call(&[1, 0, 0, 0], &[], None, 500_000).status, Err(Error::Timeout));
    });
    f.exit(server, child, 0);
    // The kernel delivers notices before messages, so both were received before this call.
    f.as_process(client, || {
        let ep = Endpoint::from_handle(badged);
        assert_eq!(ep.call(&[2, 0, 0, 0], &[], None, FOREVER).into_result().unwrap().0.words, [7, 0, 0, 0]);
    });
    f.destroy(server, receive);
    assert_eq!(server_thread.join().unwrap(), exit::OK);
    // The kept call, the abandoned notice, the exit notice, the answered call, and the death.
    assert_eq!(f.calls(server).iter().filter(|c| **c == "receive").count(), 5);
}

#[test]
fn serve_exits_ok_when_the_endpoint_dies() {
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let code =
        f.run(server, move || serve(&Endpoint::from_handle(receive), |request| answer(request, [0; 4])));
    f.destroy(server, receive);
    assert_eq!(code.join().unwrap(), exit::OK);
}

#[test]
fn serve_exits_receive_failed_on_any_other_error() {
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    f.refuse(server, "receive", Error::BadHandle);
    let code =
        f.run(server, move || serve(&Endpoint::from_handle(receive), |request| answer(request, [0; 4])));
    assert_eq!(code.join().unwrap(), exit::RECEIVE_FAILED);
    f.destroy(server, receive);
}
