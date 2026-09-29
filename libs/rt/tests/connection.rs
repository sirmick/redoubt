//! The 9P client's split: one [`Connection`] shared by threads, each lending its own [`Lend`],
//! and a lend a call consumed replaced by fresh pages on its next call.

#[path = "../src/bin/echo-server.rs"]
mod echo_server;

use std::sync::Arc;

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{Error, FOREVER, PAGE_SIZE};
use redoubt_rt::client::{ClientError, Connection, Lend};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Event;
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::{Startup, StartupBuilder};

/// Two threads of one process share a connection: each walks, opens, writes and reads its own
/// fid at once, with its own lend, and each gets back exactly its own bytes.
#[test]
fn threads_share_one_connection_with_their_own_lends() {
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let client = f.process(1001, &[]);
    let conn = f.grant(server, receive, client, 1);
    let block = StartupBuilder::new(receive.index()).handle("echo", receive).finish().unwrap();
    let serving = f.run(server, move || echo_server::serve(&Startup::parse(&block).unwrap()));

    let shared = Arc::new(Connection::new(Endpoint::from_handle(conn)));
    f.as_process(client, || {
        let mut lend = Lend::new(1).unwrap();
        shared.version(&mut lend).unwrap();
        shared.attach(&mut lend, 0, "").unwrap();
    });
    let threads: Vec<_> = (1..=2u32)
        .map(|i| {
            let conn = Arc::clone(&shared);
            f.run(client, move || {
                let mut lend = Lend::new(1).unwrap();
                let text = [b'0' + i as u8; 100];
                let offset = u64::from(i) * 1000;
                for _ in 0..20 {
                    conn.walk(&mut lend, 0, i, "echo").unwrap();
                    conn.open(&mut lend, i, mode::ORDWR).unwrap();
                    assert_eq!(conn.write(&mut lend, i, offset, &text).unwrap(), text.len());
                    let mut back = [0u8; 100];
                    assert_eq!(conn.read(&mut lend, i, offset, &mut back).unwrap(), back.len());
                    assert_eq!(back, text, "thread {i} read another's bytes");
                    conn.clunk(&mut lend, i).unwrap();
                }
                0
            })
        })
        .collect();
    for thread in threads {
        assert_eq!(thread.join().unwrap(), 0);
    }
    f.destroy(server, receive);
    assert_eq!(serving.join().unwrap(), 0);
}

/// A call its server took and its caller then gave up consumes the lend (R3): the pages are the
/// server's now. The next call with the same `Lend` maps fresh pages rather than touching the
/// old address, and succeeds.
#[test]
fn a_consumed_lend_is_replaced_by_fresh_pages() {
    let f = fake();
    let holder = f.process(0, &[]);
    let held = f.endpoint(holder);
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let client = f.process(1001, &[]);
    let to_holder = f.grant(holder, held, client, 1);
    let to_echo = f.grant(server, receive, client, 1);
    let block = StartupBuilder::new(receive.index()).handle("echo", receive).finish().unwrap();
    let serving = f.run(server, move || echo_server::serve(&Startup::parse(&block).unwrap()));
    // Takes one call and holds it until its caller gives up; then the drop answers it.
    let holding = f.run(holder, move || {
        let endpoint = Endpoint::from_handle(held);
        let Ok(Event::Call(request)) = endpoint.receive(FOREVER, 0) else { return 1 };
        let Ok(Event::Abandoned(id)) = endpoint.receive(FOREVER, 0) else { return 2 };
        u32::from(id != request.id()) * 3
    });

    f.as_process(client, || {
        let mut lend = Lend::new(1).unwrap();
        let mut stuck = Connection::new(Endpoint::from_handle(to_holder));
        stuck.timeout = 50_000;
        assert_eq!(stuck.attach(&mut lend, 0, ""), Err(ClientError::Sys(Error::Timeout)));
        assert_eq!(f.held(client).1, 0, "the consumed page is no longer the client's");
        let echo = Connection::new(Endpoint::from_handle(to_echo));
        echo.attach(&mut lend, 0, "").unwrap();
        assert_eq!(f.held(client).1, 1, "one fresh page");
        assert_eq!(lend.iounit(), PAGE_SIZE - redoubt_rt::wire::ninep::IOHDRSZ);
    });
    assert_eq!(holding.join().unwrap(), 0);
    f.destroy(server, receive);
    assert_eq!(serving.join().unwrap(), 0);
}
