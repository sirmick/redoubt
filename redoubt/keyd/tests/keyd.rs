//! `keyd`, the whole program, as a fake process: it gets a startup block written with
//! `StartupBuilder`, exactly as `init` would, and runs its entry function against the runtime's
//! fake kernel. Then clients — one honest, one hostile — call it over the real IPC path, so
//! minting, replying and closing are exercised as they are on the machine.
//!
//! The property tests that do not need a kernel are in `src/server_tests.rs`.

#[path = "../../rt/tests/common/mod.rs"]
mod common;

use common::fake;
use ed25519_compact::{PublicKey, Signature};
use redoubt_keyd::keys::Keys;
use redoubt_keyd::server::{BUDGET, COST, KeyServer, LIMITS, audit_digest};
use redoubt_rt::abi::{FOREVER, Handle};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Buffer, Event, Words};
use redoubt_rt::server::MALFORMED;
use redoubt_rt::startup::{Startup, StartupBuilder};
use redoubt_rt::wire::proto::keyd::{
    ErrorCode, Grant, Holds, Message, PublicKey as PublicKeyRequest, Release, Reply, SignRecord,
    SignSshExchange,
};

/// A transcript for the host key's one operation.
fn transcript() -> SignSshExchange<'static> {
    SignSshExchange {
        v_c: b"SSH-2.0-client",
        v_s: b"SSH-2.0-redoubt",
        i_c: b"client kexinit",
        i_s: b"server kexinit",
        q_c: &[7; 32],
        q_s: &[8; 32],
        k: &[9; 32],
    }
}

#[path = "../src/bin/keyd.rs"]
mod keyd;

const HOST_SEED: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const AUDIT_SEED: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const HOST_BADGE: u64 = 1;
const AUDIT_BADGE: u64 = 2;

/// Runs `main` as `pid` with a startup block naming `handles`.
fn launch(pid: usize, block: Vec<u8>, main: fn(&Startup) -> u32) -> std::thread::JoinHandle<u32> {
    fake().run(pid, move || {
        let startup = Startup::parse(&block).expect("the launcher's block parses");
        redoubt_rt::start::note_console(&startup);
        main(&startup)
    })
}

/// The block `init` writes for `keyd`: the endpoint it receives on, and one argument per key.
fn keyd_block(receive: Handle, keys: &[&str]) -> Vec<u8> {
    let mut builder = StartupBuilder::new(receive.index());
    builder.handle("keyd", receive);
    for key in keys {
        builder.arg(key);
    }
    builder.finish().unwrap()
}

fn default_keys() -> Vec<String> {
    vec![format!("host,ssh_host,{HOST_SEED}"), format!("audit,audit,{AUDIT_SEED}")]
}

/// A running `keyd` and a client process given a capability with `badge`.
struct Box_ {
    server: usize,
    receive: Handle,
    thread: std::thread::JoinHandle<u32>,
}

fn start_keyd(keys: &[String]) -> Box_ {
    let f = fake();
    let server = f.process(0, &[]);
    let receive = f.endpoint(server);
    let args: Vec<&str> = keys.iter().map(String::as_str).collect();
    let thread = launch(server, keyd_block(receive, &args), keyd::serve);
    Box_ { server, receive, thread }
}

impl Box_ {
    /// A client process holding a capability with `badge`.
    fn client(&self, account: u64, labels: &[u64], badge: u64) -> (usize, Handle) {
        let f = fake();
        let client = f.process(account, labels);
        let handle = f.grant(self.server, self.receive, client, badge);
        (client, handle)
    }

    fn stop(self) -> u32 {
        fake().destroy(self.server, self.receive);
        self.thread.join().unwrap()
    }
}

/// One call from inside a fake process: encodes `request`, lends a buffer, and gives back the
/// reply words, the reply's bytes and the handles it carried.
fn call(handle: Handle, request: &Message<'_>) -> (Words, Vec<u8>, Vec<Handle>) {
    let endpoint = Endpoint::from_handle(handle);
    let inline = matches!(request, Message::Grant(_) | Message::Release(_));
    let mut lend = Buffer::new(16).unwrap();
    let words = if inline { request.encode(&mut []).unwrap() } else { request.encode(&mut lend).unwrap() };
    let reply = if inline {
        endpoint.call(&words, &[], None, FOREVER).unwrap()
    } else {
        endpoint.call(&words, &[], Some(&mut lend), FOREVER).unwrap()
    };
    let handles: Vec<Handle> = reply.handles.as_slice().iter().flatten().copied().collect();
    (reply.words, lend.to_vec(), handles)
}

/// `call`, decoded as a reply of this protocol.
fn ask(handle: Handle, request: &Message<'_>) -> Result<Vec<u8>, Option<ErrorCode>> {
    let opcode = redoubt_rt::wire::typed::opcode(&request.encode(&mut [0u8; 65536]).unwrap()).unwrap();
    let inline = matches!(request, Message::Grant(_) | Message::Release(_));
    let (words, body, handles) = call(handle, request);
    if words == MALFORMED {
        return Err(None);
    }
    // An inline reply has no buffer: the lend comes back untouched, and a decoder handed it
    // would refuse the reply (WIRE.md).
    let body: &[u8] = if inline { &[] } else { &body };
    match Reply::decode(opcode, &words, body, handles.len()) {
        Ok(Ok(Reply::SignRecord(r))) => Ok(r.signature.to_vec()),
        Ok(Ok(Reply::SignSshExchange(r))) => Ok(r.signature.to_vec()),
        Ok(Ok(Reply::PublicKey(r))) => Ok(r.key.to_vec()),
        Ok(Ok(Reply::Holds(r))) => Ok(vec![r.held as u8]),
        Ok(Ok(Reply::Grant(r))) => Ok(r.id.to_le_bytes().to_vec()),
        Ok(Ok(Reply::Release(_))) => Ok(vec![]),
        Ok(Err(code)) => Err(Some(code)),
        Err(_) => Err(None),
    }
}

fn verifies(public: &[u8], message: &[u8], signature: &[u8]) -> bool {
    let (Ok(public), Ok(signature)) = (<[u8; 32]>::try_from(public), <[u8; 64]>::try_from(signature)) else {
        return false;
    };
    PublicKey::new(public).verify(message, &Signature::new(signature)).is_ok()
}

/// The steward signs an audit record through its badge, and what comes back verifies under the
/// public key `keyd` will show anyone — over the bytes `keyd` built, not the bytes sent.
#[test]
fn a_signature_round_trips_over_the_real_ipc_path() {
    let keyd = start_keyd(&default_keys());
    let (steward, capability) = keyd.client(0, &[], AUDIT_BADGE);
    let record = b"2026-09-19 lease to agent-7";
    fake().as_process(steward, || {
        let public = ask(capability, &Message::PublicKey(PublicKeyRequest {})).unwrap();
        let signature = ask(capability, &Message::SignRecord(SignRecord { record })).unwrap();
        assert!(verifies(&public, &audit_digest(record), &signature));
        assert!(!verifies(&public, record, &signature));
        // The key is here, and a key that is not is not.
        assert_eq!(ask(capability, &Message::Holds(Holds { key: &public })), Ok(vec![1]));
        let mut other = public.clone();
        other[0] ^= 1;
        assert_eq!(ask(capability, &Message::Holds(Holds { key: &other })), Ok(vec![0]));
    });
    assert_eq!(keyd.stop(), 0, "and it exits cleanly when its endpoint goes");
}

/// A launcher asks for a fresh capability for each child, and disconnects it when the child is
/// gone (INIT.md, "Launching gives fresh connections"). The handle really arrives, really works
/// for the same key and purpose, and really stops working when released — and `keyd` keeps no
/// handle of its own per grant.
#[test]
fn a_launcher_grants_a_fresh_capability_and_releases_it() {
    let keyd = start_keyd(&default_keys());
    let (steward, capability) = keyd.client(0, &[], AUDIT_BADGE);
    let before = fake().held(keyd.server).0;
    fake().as_process(steward, || {
        let (words, _, handles) = call(capability, &Message::Grant(Grant {}));
        assert_eq!(words[0], 0, "granted");
        assert_eq!(handles.len(), 1, "the capability arrived as a handle");
        let Ok(Ok(Reply::Grant(granted))) = Reply::decode(5, &words, &[], handles.len()) else {
            panic!("the grant reply decodes")
        };
        let id = granted.id;
        assert_ne!(id, 0, "and its id is random, not a counter");
        let child = handles[0];
        // It works, for the same key and purpose and no other.
        assert!(ask(child, &Message::SignRecord(SignRecord { record: b"x" })).is_ok());
        // Released by the one that asked for it, the child's handle stops working at once.
        assert_eq!(ask(capability, &Message::Release(Release { id })), Ok(vec![]));
        assert_eq!(
            ask(child, &Message::SignRecord(SignRecord { record: b"x" })),
            Err(Some(ErrorCode::NotPermitted))
        );
        // Releasing it twice says the same as releasing a stranger's.
        assert_eq!(ask(capability, &Message::Release(Release { id })), Err(Some(ErrorCode::NotPermitted)));
        // `grant` and `release` are inline messages, so a caller that lends with one is not
        // sending one: refused, like an inline message with a buffer on any other opcode.
        let endpoint = Endpoint::from_handle(capability);
        let mut lend = Buffer::new(1).unwrap();
        for request in [Message::Grant(Grant {}), Message::Release(Release { id })] {
            let words = request.encode(&mut []).unwrap();
            let reply = endpoint.call(&words, &[], Some(&mut lend), FOREVER).unwrap();
            assert_eq!(reply.words, MALFORMED);
            assert!(reply.handles.as_slice().iter().all(Option::is_none), "and nothing was minted");
        }
    });
    assert_eq!(fake().held(keyd.server).0, before, "keyd kept no handle per grant");
    assert_eq!(keyd.stop(), 0);
}

/// A hostile client hammers `keyd` with junk and with requests its badge does not allow.
/// `keyd` survives, keeps answering an honest client, and never grows.
#[test]
fn a_hostile_client_does_not_hurt_keyd_or_other_clients() {
    let keyd = start_keyd(&default_keys());
    let (attacker, hostile) = keyd.client(666, &[], HOST_BADGE);
    let (steward, capability) = keyd.client(0, &[], AUDIT_BADGE);
    let handles_before = fake().held(keyd.server).0;

    let attack = fake().run(attacker, move || {
        let endpoint = Endpoint::from_handle(hostile);
        let mut lend = Buffer::new(1).unwrap();
        for round in 0..200u64 {
            // Words that are not a request of this protocol, with handles to fill the table.
            let junk = Endpoint::create().unwrap();
            let words = [round % 11, round.wrapping_mul(7), round, 3];
            let reply =
                endpoint.call(&words, &[junk.handle(), junk.handle()], Some(&mut lend), FOREVER).unwrap();
            assert_ne!(reply.words[0], 0, "nothing junk was ever accepted");
            let _ = junk.close();
            // And an operation its badge does not name: the host badge asking for a record.
            let request = Message::SignRecord(SignRecord { record: b"give me a signature" });
            let words = request.encode(&mut lend).unwrap();
            let reply = endpoint.call(&words, &[], Some(&mut lend), FOREVER).unwrap();
            assert_eq!(reply.words[0], u64::from(ErrorCode::NotPermitted.code()));
        }
        0
    });
    assert_eq!(attack.join().unwrap(), 0);

    // The verdict comes from the victim, not the attacker: the steward still gets its work
    // done, with the answer it expected.
    fake().as_process(steward, || {
        let public = ask(capability, &Message::PublicKey(PublicKeyRequest {})).unwrap();
        let signature = ask(capability, &Message::SignRecord(SignRecord { record: b"still here" })).unwrap();
        assert!(verifies(&public, &audit_digest(b"still here"), &signature));
    });
    assert_eq!(fake().held(keyd.server).0, handles_before, "no handle of the attacker's stuck");
    assert_eq!(keyd.stop(), 0, "and keyd was alive the whole time");
}

/// A `keyd` whose manifest arguments are wrong does not start: fail closed and loudly, rather
/// than serving with a key silently missing.
#[test]
fn bad_key_arguments_stop_keyd_starting() {
    let f = fake();
    for bad in [
        vec![String::from("host")],
        vec![format!("host,login,{HOST_SEED}")],
        vec![format!("host,ssh_host,{}", &HOST_SEED[1..])],
        vec![format!("a,ssh_host,{HOST_SEED}"), format!("b,audit,{HOST_SEED}")],
        vec![format!("Host,ssh_host,{HOST_SEED}")],
    ] {
        let pid = f.process(0, &[]);
        let receive = f.endpoint(pid);
        let args: Vec<&str> = bad.iter().map(String::as_str).collect();
        let code = launch(pid, keyd_block(receive, &args), keyd::serve).join().unwrap();
        assert_eq!(code, keyd::BAD_KEYS, "{bad:?}");
    }
    // And a block with no endpoint for it.
    let pid = f.process(0, &[]);
    let code = launch(pid, StartupBuilder::new(0).finish().unwrap(), keyd::serve).join().unwrap();
    assert_eq!(code, keyd::NO_ENDPOINT);
}

/// A `send` reaches `keyd` (every message of this protocol is a `call`), is dropped, and what
/// it brought is closed, so a client cannot grow the handle table that way either.
#[test]
fn a_one_way_message_is_dropped_and_its_handles_closed() {
    let keyd = start_keyd(&default_keys());
    let (client, capability) = keyd.client(1001, &[], AUDIT_BADGE);
    let before = fake().held(keyd.server).0;
    let sender = fake().run(client, move || {
        let endpoint = Endpoint::from_handle(capability);
        for _ in 0..20 {
            let junk = Endpoint::create().unwrap();
            let _ = endpoint.send(&[2, 0, 0, 0], &[junk.handle()], None, FOREVER);
            let _ = junk.close();
        }
        0
    });
    assert_eq!(sender.join().unwrap(), 0);
    // Then an ordinary call, so the sends have certainly been taken.
    fake().as_process(client, || {
        assert!(ask(capability, &Message::PublicKey(PublicKeyRequest {})).is_ok());
    });
    assert_eq!(fake().held(keyd.server).0, before);
    assert_eq!(keyd.stop(), 0);
}

/// The red team's attack, turned into its refutation. Endpoints outlive servers (INIT.md
/// decision 4) and a server keeps no state across a restart, so a capability granted before a
/// restart is still a live handle afterwards. When both incarnations started their badges at
/// 2^63, the restarted `keyd` handed that very badge to its first new client, and the stale
/// handle silently became a capability for whatever that client asked for — an agent's audit
/// grant turning into the box's SSH host key, with no id anyone could use to evict it.
///
/// Now each incarnation draws its first badge at random above 2^63 (answer 126), so the badge
/// the restarted `keyd` gives out is not the one the stale handle carries, and the stale handle
/// names nothing at all.
#[test]
fn a_stale_grant_does_not_name_a_new_key_after_a_restart() {
    let f = fake();
    let keys = default_keys();
    let args: Vec<&str> = keys.iter().map(String::as_str).collect();

    // keyd, first incarnation: it answers one call (a grant) and then exits.
    let server1 = f.process(0, &[]);
    let receive1 = f.endpoint(server1);
    let block1 = keyd_block(receive1, &args);
    let first = f.run(server1, move || {
        let startup = Startup::parse(&block1).unwrap();
        let handle = startup.handle("keyd").unwrap();
        let keys = Keys::from_args(startup.args()).unwrap();
        // The first incarnation's own draw.
        let mut server = KeyServer::new(keys, LIMITS, &COST, BUDGET, 0x1111_1111_1111_1111).unwrap();
        let endpoint = Endpoint::from_handle(handle);
        match endpoint.receive(FOREVER, 0) {
            Ok(Event::Call(request)) => {
                let _ = server.serve(request);
            }
            other => panic!("expected a call, got {other:?}"),
        }
        7
    });

    // The steward grants an audit capability and hands it to an agent, as it would for a lease.
    let steward = f.process(0, &[]);
    let steward_cap = f.grant(server1, receive1, steward, AUDIT_BADGE);
    let agent = f.process(1001, &[]);
    let stale = f.as_process(steward, || {
        let (words, _, handles) = call(steward_cap, &Message::Grant(Grant {}));
        assert_eq!(words[0], 0, "granted");
        f.copy(steward, handles[0], agent)
    });
    assert_eq!(first.join().unwrap(), 7, "keyd's first incarnation is gone");

    // `init` restarts keyd on the same endpoint, from the same manifest arguments. Its draw is
    // a different word, which is the whole of the fix.
    let server2 = f.process(0, &[]);
    let receive2 = f.copy(server1, receive1, server2);
    let block2 = keyd_block(receive2, &args);
    let second = f.run(server2, move || {
        let startup = Startup::parse(&block2).unwrap();
        let handle = startup.handle("keyd").unwrap();
        let keys = Keys::from_args(startup.args()).unwrap();
        let mut server = KeyServer::new(keys, LIMITS, &COST, BUDGET, 0x2222_2222_2222_2222).unwrap();
        let endpoint = Endpoint::from_handle(handle);
        loop {
            match endpoint.receive(FOREVER, 0) {
                Ok(Event::Call(request)) => {
                    let _ = server.serve(request);
                }
                Ok(_) => {}
                Err(_) => return 0,
            }
        }
    });

    // sshd asks the restarted keyd for a fresh capability for the host key.
    let sshd = f.process(0, &[]);
    let sshd_cap = f.grant(server1, receive1, sshd, HOST_BADGE);
    f.as_process(sshd, || {
        let (words, _, handles) = call(sshd_cap, &Message::Grant(Grant {}));
        assert_eq!(words[0], 0, "granted");
        assert_eq!(handles.len(), 1);
    });

    // The agent's stale capability names nothing: not the host key, not the audit key, nothing.
    f.as_process(agent, || {
        assert_eq!(
            ask(stale, &Message::PublicKey(PublicKeyRequest {})),
            Err(Some(ErrorCode::NotPermitted)),
            "a badge from before the restart names no key"
        );
        assert_eq!(
            ask(stale, &Message::SignSshExchange(transcript())),
            Err(Some(ErrorCode::NotPermitted)),
            "and certainly does not speak as the box"
        );
    });

    f.destroy(server1, receive1);
    assert_eq!(second.join().unwrap(), 0);
}
