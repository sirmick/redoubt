//! The runtime's system-call paths against the fake kernel: IPC with lends, transfers and
//! handles, `mint`, and exits.

use std::num::NonZeroU64;

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{Error, FOREVER, Handles, PAGE_SIZE};
use redoubt_rt::handle::{self, Endpoint};
use redoubt_rt::ipc::{Buffer, Event};
use redoubt_rt::server::typed::{Outcome, finish};

fn nz(v: u64) -> NonZeroU64 { NonZeroU64::new(v).unwrap() }

#[test]
fn call_lend_and_reply() {
    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(1001, &[3, 1]));
    let receive = f.endpoint(server);
    let badged = f.grant(server, receive, client, 42);

    let server_thread = f.run(server, move || {
        let ep = Endpoint::from_handle(receive);
        let Ok(Event::Call(mut request)) = ep.receive(FOREVER, 0) else { return 1 };
        // What the kernel attaches: badge, account, labels.
        assert_eq!((request.caller.badge, request.caller.account), (42, 1001));
        assert_eq!(request.caller.labels.as_slice(), &[3, 1]);
        assert_eq!(request.words, [7, 8, 9, 10]);
        // The handle it brought is ours now; the lend is readable and writable.
        let brought = request.handles.as_slice()[0].unwrap();
        let lend = request.lend();
        assert_eq!(lend.len(), 2 * PAGE_SIZE);
        assert_eq!(&lend[..5], b"hello");
        lend[..5].copy_from_slice(b"HELLO");
        // A handle to the same endpoint, minted from the message, goes back in the reply.
        let minted = request.mint(nz(77), None).unwrap();
        // The reply carries copies; `finish` closes ours once it has sent them.
        let both = Handles::from_slice(&[minted.handle(), brought]).unwrap();
        finish(request, &Outcome { words: [1, 2, 3, u64::from(u32::MAX)], send: both, close: both }).unwrap();
        0
    });

    let code = f.run(client, move || {
        let ep = Endpoint::from_handle(badged);
        let mut lend = Buffer::new(2).unwrap();
        lend[..5].copy_from_slice(b"hello");
        let extra = Endpoint::create().unwrap();
        let (reply, returned) =
            ep.call(&[7, 8, 9, 10], &[extra.handle()], Some(lend), FOREVER).into_result().unwrap();
        let lend = returned.unwrap();
        assert_eq!(reply.words, [1, 2, 3, u64::from(u32::MAX)]);
        assert_eq!(reply.handles.as_slice().len(), 2);
        assert_eq!(&lend[..5], b"HELLO");
        0
    });
    assert_eq!(code.join().unwrap(), 0);
    assert_eq!(server_thread.join().unwrap(), 0);
}

#[test]
fn send_transfers_pages_for_good() {
    let f = fake();
    let (server, client) = (f.process(0, &[]), f.process(5, &[]));
    let receive = f.endpoint(server);
    let badged = f.grant(server, receive, client, 1);
    let before = f.held(server).1;

    let server_thread = f.run(server, move || {
        let ep = Endpoint::from_handle(receive);
        let Ok(Event::Send(delivery)) = ep.receive(FOREVER, 4) else { return 1 };
        let transfer = delivery.transfer.expect("pages");
        assert_eq!(&transfer[..4], b"gift");
        assert_eq!(delivery.words, [9, 0, 0, 0]);
        let mapped = fake().held(server).1;
        drop(transfer);
        // Dropping the buffer unmaps it: the pages were ours.
        assert_eq!(fake().held(server).1, mapped - 3);
        0
    });
    let client_thread = f.run(client, move || {
        let mut gift = Buffer::new(3).unwrap();
        gift[..4].copy_from_slice(b"gift");
        let held = fake().held(client).1;
        Endpoint::from_handle(badged)
            .send(&[9, 0, 0, 0], &[], Some(gift), FOREVER)
            .map_err(|(e, _)| e)
            .unwrap();
        // Gone from the sender.
        assert_eq!(fake().held(client).1, held - 3);
        0
    });
    assert_eq!(client_thread.join().unwrap(), 0);
    assert_eq!(server_thread.join().unwrap(), 0);
    assert_eq!(f.held(server).1, before);
}

#[test]
fn timeouts_dead_endpoints_and_refusals() {
    let f = fake();
    let pid = f.process(0, &[]);
    let receive = f.endpoint(pid);
    let badged = f.grant(pid, receive, pid, 9);
    f.as_process(pid, || {
        let ep = Endpoint::from_handle(receive);
        assert!(matches!(ep.receive(1000, 0), Err(Error::Timeout)));
        assert_eq!(handle::sleep(1000), Ok(()));
        let other = Endpoint::from_handle(badged);
        // Only the receive right receives, and only it mints.
        assert!(matches!(other.receive(0, 0), Err(Error::NotPermitted)));
        assert_eq!(other.mint(nz(1), None), Err(Error::NotPermitted));
        // A call nobody takes times out.
        assert_eq!(other.call(&[0; 4], &[], None, 1000).status, Err(Error::Timeout));
        let t0 = handle::time_now().unwrap();
        assert!(handle::time_now().unwrap() >= t0);
        assert_ne!(handle::random_u64().unwrap(), handle::random_u64().unwrap());
        // Closing a handle makes it unusable.
        other.close().unwrap();
        assert_eq!(Endpoint::from_handle(badged).call(&[0; 4], &[], None, 0).status, Err(Error::BadHandle));
    });
    f.destroy(pid, receive);
    f.as_process(pid, || {
        assert!(matches!(Endpoint::from_handle(receive).receive(FOREVER, 0), Err(Error::Dead)))
    });
}

#[test]
fn exit_codes_reach_the_parent() {
    let f = fake();
    let pid = f.process(0, &[]);
    assert_eq!(f.run(pid, || handle::process_exit(12345)).join().unwrap(), 12345);
    assert_eq!(f.run(pid, || redoubt_rt::exit::OK).join().unwrap(), 0);
}
