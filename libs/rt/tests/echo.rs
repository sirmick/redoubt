//! The echo pair (`src/bin/echo-server.rs`, `src/bin/echo-client.rs`), unchanged, as two fake
//! processes: each gets a startup block written with `StartupBuilder`, exactly as a launcher
//! would, and runs its entry function. Then a hostile client attacks the same server, and a
//! launcher gives a child a fresh connection and disconnects it.

#[path = "../src/bin/echo-client.rs"]
mod echo_client;
#[path = "../src/bin/echo-server.rs"]
mod echo_server;

use core::num::NonZeroU64;

use redoubt_fake_kernel::fake;
use redoubt_rt::abi::{BudgetSpec, Error, FOREVER, Handle, Labels, PAGE_SIZE};
use redoubt_rt::client::{ClientError, Connection, Lend};
use redoubt_rt::handle::{Budget, Endpoint};
use redoubt_rt::ipc::Buffer;
use redoubt_rt::server::MALFORMED;
use redoubt_rt::server::ninep::WORDS_9P;
use redoubt_rt::startup::{Startup, StartupBuilder};
use redoubt_rt::wire::ninep::{Body, ErrorName, Message};

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
        // table: refused, and the handles closed. Opcode 9 is in ninep_common's range, which it
        // does not define; 16 is a typed opcode, which the echo server does not serve.
        let mut lend = Buffer::new(1).unwrap();
        for opcode in [9, 16].repeat(25) {
            let junk = Endpoint::create().unwrap();
            let (reply, returned) = ep
                .call(&[opcode, 9, 9, 9], &[junk.handle(), junk.handle()], Some(lend), FOREVER)
                .into_result()
                .unwrap();
            lend = returned.unwrap();
            assert_eq!(reply.words, MALFORMED);
            junk.close().unwrap();
        }
        // 9P with no lend at all.
        assert_eq!(ep.call(&WORDS_9P, &[], None, FOREVER).into_result().unwrap().0.words, MALFORMED);
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
            let (reply, returned) = ep.call(&WORDS_9P, &[], Some(lend), FOREVER).into_result().unwrap();
            lend = returned.unwrap();
            assert_eq!(reply.words, WORDS_9P);
            assert!(Message::decode(&lend).is_ok());
        }
        // Exhausting fids: bounded, refused with an error, and nobody else pays for it.
        let mut refused = 0;
        for fid in 0..200 {
            lend.fill(0);
            let attach = Body::Tattach { fid, afid: u32::MAX, uname: "", aname: "" };
            Message { tag: 1, body: attach }.encode(&mut lend).unwrap();
            let (_, returned) = ep.call(&WORDS_9P, &[], Some(lend), FOREVER).into_result().unwrap();
            lend = returned.unwrap();
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

#[test]
fn the_echo_servers_caps_fit_its_budget() {
    assert!(echo_server::LIMITS.fits(&echo_server::COST, echo_server::BUDGET));
}

/// servers/init.md, "Fresh connections per child": a launcher asks the server for a connection for
/// its child, passes that, and disconnects it when the child is done. Only the launcher can.
#[test]
fn a_launcher_gives_its_child_a_fresh_connection_and_disconnects_it() {
    let (server, receive, launcher, connection) = pair(1001, 5);
    let f = fake();
    let server_thread = launch(server, server_block(receive), echo_server::serve);
    let child = f.process(1001, &[]);
    let stranger = f.process(1001, &[]);
    let strangers_conn = f.grant(server, receive, stranger, 6);
    let handles_before = f.held(server).0;

    let (fresh, id) = f.as_process(launcher, || {
        let client = Connection::new(Endpoint::from_handle(connection));
        let mut lend = Lend::new(1).unwrap();
        // A name that is not there is NotFound, whether the first name misses (the server's
        // `file does not exist`) or a later one (a walk that stops short).
        client.attach(&mut lend, 90, "").unwrap();
        assert_eq!(client.walk(&mut lend, 90, 91, "nope"), Err(ClientError::Rerror(ErrorName::NotFound)));
        assert_eq!(
            client.walk(&mut lend, 90, 91, "echo/nope"),
            Err(ClientError::Rerror(ErrorName::NotFound))
        );
        client.clunk(&mut lend, 90).unwrap();
        let (fresh, id) = client.new_connection(&mut lend, "", 0, None).unwrap();
        // Rooted below the launcher's root; a file that is not there is refused.
        assert_eq!(client.new_connection(&mut lend, "nope", 0, None).err(), Some(ClientError::Remote));
        (fresh.handle(), id)
    });
    // The server kept no handle: its copy of the minted one was closed once the reply carried it.
    assert_eq!(f.held(server).0, handles_before);
    let fresh = f.copy(launcher, fresh, child);
    let block = StartupBuilder::new(fresh.index()).namespace("/", fresh).finish().unwrap();
    assert_eq!(launch(child, block, echo_client::run).join().unwrap(), 0);

    // Someone else cannot disconnect the child, even knowing its id.
    f.as_process(stranger, || {
        assert_eq!(
            Connection::new(Endpoint::from_handle(strangers_conn)).disconnect(id, FOREVER),
            Err(ClientError::Remote)
        );
    });
    f.as_process(child, || {
        let client = Connection::new(Endpoint::from_handle(fresh));
        let mut lend = Lend::new(1).unwrap();
        client.attach(&mut lend, 7, "").unwrap(); // the echo client still holds fid 0
    });
    // The launcher can; then the child's connection is gone, its fid with it.
    f.as_process(launcher, || {
        let client = Connection::new(Endpoint::from_handle(connection));
        client.disconnect(id, FOREVER).unwrap();
        assert_eq!(client.disconnect(id, FOREVER), Err(ClientError::Remote), "an id is spent once used");
    });
    f.as_process(child, || {
        let client = Connection::new(Endpoint::from_handle(fresh));
        let mut lend = Lend::new(1).unwrap();
        assert!(matches!(client.attach(&mut lend, 1, ""), Err(ClientError::Rerror(_))));
        // A ninep_common call bringing handles it did not ask for: malformed, and closed.
        let junk = Endpoint::create().unwrap();
        let ep = Endpoint::from_handle(fresh);
        let reply = ep.call(&[3, 0, 0, 0], &[junk.handle(), junk.handle()], None, FOREVER);
        assert_eq!(reply.into_result().unwrap().0.words, MALFORMED);
        junk.close().unwrap();
    });
    assert_eq!(f.held(server).0, handles_before);
    f.destroy(server, receive);
    assert_eq!(server_thread.join().unwrap(), 0);
}

/// A zero-limit budget: a revocation scope (kernel/budgets.md).
fn scope_spec() -> BudgetSpec {
    BudgetSpec {
        pages: 0,
        processes: 0,
        weight: 0,
        labels: Labels::from_slice(&[]).unwrap(),
        account: 0,
        deadline: FOREVER,
    }
}

/// R41 (servers/steward.md): a launcher mints a child's connection in a revocation scope,
/// `new_connection(root, quota, scope)`, and the server stamps the connection's handle with it.
/// Destroying the scope ends the connection in every table at once: the child's next request
/// finds its handle gone, while a sibling connection in another scope on the same server is
/// untouched. The server holds nothing for the scope (its copy is closed once minted with) and
/// cannot see the destruction: its record stays until the launcher's `disconnect`, which is
/// the cleanup and never the cutoff. A scope not below the server's own stamp is refused.
#[test]
fn a_connection_minted_in_a_scope_ends_when_the_scope_is_destroyed() {
    let f = fake();
    let server = f.process(0, &[]);
    // The budget the server's connections are stamped with: for a shared server, `users`.
    let users = f.budget(server);
    f.runs_in(server, users);
    let receive = f.endpoint(server);
    let launcher = f.process(1001, &[]);
    let connection = f.grant(server, receive, launcher, 5);
    let users_at_launcher = f.copy(server, users, launcher);
    let server_thread = launch(server, server_block(receive), echo_server::serve);
    let (child_s, child_t) = (f.process(1001, &[]), f.process(1001, &[]));
    let held_before = f.held(server).0;
    let elsewhere = f.budget(launcher);

    let (in_s, id_s, in_t, id_t, scope_s) = f.as_process(launcher, || {
        let client = Connection::new(Endpoint::from_handle(connection));
        let mut lend = Lend::new(1).unwrap();
        let users = Budget::from_handle(users_at_launcher);
        let (s, t) = (users.create_child(&scope_spec()).unwrap(), users.create_child(&scope_spec()).unwrap());
        // A scope that is not below the server's stamp: the kernel refuses the mint, the
        // server answers `refused`, and nothing was made.
        let elsewhere = Budget::from_handle(elsewhere);
        assert_eq!(
            client.new_connection(&mut lend, "", 0, Some(&elsewhere)).err(),
            Some(ClientError::Remote)
        );
        let (in_s, id_s) = client.new_connection(&mut lend, "", 0, Some(&s)).unwrap();
        let (in_t, id_t) = client.new_connection(&mut lend, "", 0, Some(&t)).unwrap();
        // One more request, answered after the server has finished the mints: it closes its
        // copy of a minted handle after the reply that carried it.
        client.attach(&mut lend, 90, "").unwrap();
        client.clunk(&mut lend, 90).unwrap();
        (in_s.handle(), id_s, in_t.handle(), id_t, s.handle())
    });
    // The server kept nothing: neither its copies of the minted handles nor the scopes.
    assert_eq!(f.held(server).0, held_before);
    let in_s = f.copy(launcher, in_s, child_s);
    let in_t = f.copy(launcher, in_t, child_t);
    let attach = |pid: usize, conn: Handle, fid: u32| {
        f.as_process(pid, || {
            Connection::new(Endpoint::from_handle(conn)).attach(&mut Lend::new(1).unwrap(), fid, "")
        })
    };
    attach(child_s, in_s, 1).unwrap();
    attach(child_t, in_t, 1).unwrap();

    // The scope goes: the connection in it is dead in the child's table (and the launcher's copy
    // with it) until closed (I17), with no word to the server; the sibling still serves.
    f.as_process(launcher, || Budget::from_handle(scope_s).destroy().unwrap());
    assert_eq!(attach(child_s, in_s, 2), Err(ClientError::Sys(Error::Dead)));
    attach(child_t, in_t, 2).unwrap();
    f.as_process(child_s, || redoubt_rt::handle::close(in_s).unwrap());
    assert_eq!(f.held(child_s).0, 0, "the child's only handle is gone once closed");

    // The server still holds the dead connection's record (its fids, its admission): the
    // launcher's `disconnect` frees it, by the id, once.
    f.as_process(launcher, || {
        let client = Connection::new(Endpoint::from_handle(connection));
        client.disconnect(id_s, FOREVER).unwrap();
        assert_eq!(client.disconnect(id_s, FOREVER), Err(ClientError::Remote), "freed once");
        client.disconnect(id_t, FOREVER).unwrap();
    });
    f.destroy(server, receive);
    assert_eq!(server_thread.join().unwrap(), 0);
}

/// The fake kernel's `mint` keeps the kernel's stamp rule (kernel/objects.md R9, `mint`): a
/// mint from a receive right is stamped like the right, or with a budget named at or below that
/// stamp; one named elsewhere is `NotPermitted`. The test above relies on it for a mint from a
/// call.
#[test]
fn the_fake_kernel_s_mint_only_narrows() {
    let f = fake();
    let owner = f.process(0, &[]);
    let top = f.budget(owner);
    f.runs_in(owner, top);
    let right = f.endpoint(owner);
    let elsewhere = f.budget(owner);
    f.as_process(owner, || {
        let right = Endpoint::from_handle(right);
        let below = Budget::from_handle(top).create_child(&scope_spec()).unwrap();
        let badge = NonZeroU64::new(9).unwrap();
        assert!(right.mint(badge, None).is_ok());
        assert!(right.mint(badge, Some(&Budget::from_handle(top))).is_ok(), "at the stamp");
        let minted = right.mint(badge, Some(&below)).unwrap();
        assert_eq!(right.mint(badge, Some(&Budget::from_handle(elsewhere))).err(), Some(Error::NotPermitted));
        // Destroying the budget named takes the handle minted in it and the budget's own, and
        // no other: of the seven held (top, the right, elsewhere, below and three mints), the
        // destroy closes the budget's own, and the mint's stays, dead, until closed (I17).
        assert_eq!(f.held(owner).0, 7);
        below.destroy().unwrap();
        assert_eq!(f.held(owner).0, 6);
        assert_eq!(Endpoint::from_handle(minted.handle()).mint(badge, None).err(), Some(Error::Dead));
        redoubt_rt::handle::close(minted.handle()).unwrap();
        assert_eq!(f.held(owner).0, 5);
    });
}
