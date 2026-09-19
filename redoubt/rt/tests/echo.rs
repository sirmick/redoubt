//! The echo pair (`src/bin/echo-server.rs`, `src/bin/echo-client.rs`), unchanged, as two fake
//! processes: each gets a startup block written with `StartupBuilder`, exactly as a launcher
//! would, and runs its entry function. Then a hostile client attacks the same server.

mod common;

#[path = "../src/bin/echo-client.rs"]
mod echo_client;
#[path = "../src/bin/echo-server.rs"]
mod echo_server;

use common::fake;
use redoubt_rt::abi::{FOREVER, Handle, PAGE_SIZE};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Buffer;
use redoubt_rt::server::MALFORMED;
use redoubt_rt::server::ninep::WORDS_9P;
use redoubt_rt::startup::{Startup, StartupBuilder};
use redoubt_rt::wire::ninep::{Body, Message};

/// Runs `main` as `pid` with a startup block naming `handles`.
fn launch(pid: usize, block: Vec<u8>, main: fn(&Startup) -> u32) -> std::thread::JoinHandle<u32> {
    fake().run(pid, move || {
        let startup = Startup::parse(&block).expect("the launcher's block parses");
        redoubt_rt::start::note_console(&startup);
        main(&startup)
    })
}

/// A server process receiving on a new endpoint, and a client process given `/` -> that
/// endpoint with `badge`.
fn pair(account: u64, badge: u64) -> (usize, Handle, usize, Handle) {
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let client = f.process(account, &[]);
    let connection = f.grant(server, receive, client, badge);
    (server, receive, client, connection)
}

fn server_block(receive: Handle) -> Vec<u8> {
    StartupBuilder::new(receive.index()).handle("echo", receive).finish().unwrap()
}

#[test]
fn echo_pair_runs_on_the_runtime() {
    let (server, receive, client, connection) = pair(1001, 1);
    let server_thread = launch(server, server_block(receive), echo_server::serve);
    let block =
        StartupBuilder::new(connection.index()).namespace("/", connection).arg("-v").finish().unwrap();
    assert_eq!(launch(client, block, echo_client::run).join().unwrap(), 0);
    // A second client, another account, on its own connection.
    let f = fake();
    let other = f.process(2002, &[]);
    let connection = f.grant(server, receive, other, 2);
    let block = StartupBuilder::new(connection.index()).namespace("/", connection).finish().unwrap();
    assert_eq!(launch(other, block, echo_client::run).join().unwrap(), 0);
    f.destroy(server, receive);
    assert_eq!(server_thread.join().unwrap(), 0);
}

#[test]
fn a_client_without_its_namespace_fails_cleanly() {
    let f = fake();
    let pid = f.process(3, &[]);
    assert_eq!(launch(pid, StartupBuilder::new(0).finish().unwrap(), echo_client::run).join().unwrap(), 10);
    assert_eq!(
        launch(pid, StartupBuilder::new(0).finish().unwrap(), echo_server::serve).join().unwrap(),
        echo_server::NO_ENDPOINT
    );
}

#[test]
fn a_hostile_client_does_not_hurt_the_server_or_other_clients() {
    let (server, receive, attacker, connection) = pair(666, 3);
    let f = fake();
    let server_thread = launch(server, server_block(receive), echo_server::serve);
    let handles_before = f.held(server).0;
    let code = f.run(attacker, move || {
        let ep = Endpoint::from_handle(connection);
        // Words that are not 9P, with and without a lend and with handles to fill the server's
        // table: refused, and the handles closed.
        let mut lend = Buffer::new(1).unwrap();
        for _ in 0..50 {
            let junk = Endpoint::create().unwrap();
            let reply =
                ep.call(&[9, 9, 9, 9], &[junk.handle(), junk.handle()], Some(&mut lend), FOREVER).unwrap();
            assert_eq!(reply.words, MALFORMED);
            junk.close().unwrap();
        }
        // 9P with no lend at all.
        assert_eq!(ep.call(&WORDS_9P, &[], None, FOREVER).unwrap().words, MALFORMED);
        // Garbage in the lend: an Rerror, never a crash.
        let mut x = 0x1234_5678_u64;
        for round in 0..300 {
            for b in lend.iter_mut() {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                *b = x as u8;
            }
            if round % 2 == 0 {
                // A plausible header, so the body parser is reached.
                lend[..4].copy_from_slice(&((x % 200) as u32 + 7).to_le_bytes());
                lend[4] = 100 + (x % 28) as u8;
            }
            let reply = ep.call(&WORDS_9P, &[], Some(&mut lend), FOREVER).unwrap();
            assert_eq!(reply.words, WORDS_9P);
            assert!(Message::decode(&lend).is_ok());
        }
        // Exhausting fids: bounded, refused with an error, and nobody else pays for it.
        let mut refused = 0;
        for fid in 0..200 {
            lend.fill(0);
            let attach = Body::Tattach { fid, afid: u32::MAX, uname: "", aname: "" };
            Message { tag: 1, body: attach }.encode(&mut lend).unwrap();
            ep.call(&WORDS_9P, &[], Some(&mut lend), FOREVER).unwrap();
            if matches!(Message::decode(&lend).unwrap().body, Body::Rerror { .. }) {
                refused += 1;
            }
        }
        assert!(refused >= 200 - 32, "the account's fid limit holds");
        0
    });
    assert_eq!(code.join().unwrap(), 0);
    // Every handle the attacker sent was closed.
    assert_eq!(f.held(server).0, handles_before);
    assert!(f.held(server).1 < 64 * 1024 / PAGE_SIZE + 64, "the server's memory stays bounded");
    // An honest client on another account is served normally.
    let honest = f.process(1001, &[]);
    let connection = f.grant(server, receive, honest, 4);
    let block = StartupBuilder::new(connection.index()).namespace("/", connection).finish().unwrap();
    assert_eq!(launch(honest, block, echo_client::run).join().unwrap(), 0);
    f.destroy(server, receive);
    assert_eq!(server_thread.join().unwrap(), 0);
}
