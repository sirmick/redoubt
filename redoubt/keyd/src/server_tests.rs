//! Every security property `keyd` claims, with a test that tries to break it
//! (BUILD-PLAN.md, WP-S1; TENETS.md 6).
//!
//! These drive [`answer_with`] against a fake kernel of a few lines, so every path — minting
//! included — runs with no system call. `tests/keyd.rs` runs the whole program instead, against
//! the runtime's fake kernel, and `tests/timing.rs` is the timing check.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use ed25519_compact::{PublicKey, Signature};
use redoubt_rt::abi::Labels;
use redoubt_rt::server::{AdmitKey, Resource};
use redoubt_rt::wire::proto::keyd::{Holds, PublicKey as PublicKeyRequest, SignRecord, SignSshExchange};

use super::*;
use crate::keys::{PUBLIC_KEY_LEN, SEED_HEX_LEN};

/// Seeds for the two keys every test starts with. Public and not for production.
const HOST_SEED: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const AUDIT_SEED: &str = "2222222222222222222222222222222222222222222222222222222222222222";
/// A key that is *not* in `keyd`: what a person logs in with.
const LOGIN_SEED: &str = "3333333333333333333333333333333333333333333333333333333333333333";

/// The word the tests draw the first granted badge from, so a failure reproduces; on the
/// machine it is one word of the kernel's CSPRNG (answer 126).
const TEST_RANDOM: u64 = 0x1234_5678_9abc_def0;

/// The root badges, in manifest order (INIT.md).
const HOST_BADGE: u64 = 1;
const AUDIT_BADGE: u64 = 2;

fn server() -> KeyServer {
    let args = [format!("host,ssh_host,{HOST_SEED}"), format!("audit,audit,{AUDIT_SEED}")];
    let keys = Keys::from_args(args.iter().map(String::as_str)).unwrap();
    KeyServer::new(keys, LIMITS, &COST, BUDGET, TEST_RANDOM).unwrap()
}

fn caller(badge: u64, account: u64, labels: &[u64]) -> Caller {
    Caller { badge, account, labels: Labels::from_slice(labels).unwrap() }
}

/// A kernel of a few lines: it hands out handle numbers, counts what was closed, and draws ids
/// from a fixed sequence so a failure reproduces.
struct FakeKernel {
    next_handle: u32,
    rng: u64,
    minted: Vec<u64>,
    /// Makes the next `mint` fail, for the path where a grant cannot be made.
    mint_fails: bool,
}

impl FakeKernel {
    fn new() -> FakeKernel {
        FakeKernel { next_handle: 100, rng: 0x2545_f491_4f6c_dd1d, minted: Vec::new(), mint_fails: false }
    }
}

impl Minter for FakeKernel {
    fn mint(&mut self, badge: NonZeroU64) -> Result<Handle, Error> {
        if self.mint_fails {
            return Err(Error::OutOfMemory);
        }
        self.minted.push(badge.get());
        self.next_handle += 1;
        Ok(Handle::new(self.next_handle).unwrap())
    }

    fn random(&mut self) -> Result<u64, Error> {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        Ok(self.rng)
    }
}

/// What one request answered with: the reply, or the error code, or `Malformed`.
#[derive(Debug, PartialEq, Eq)]
enum Answered {
    Ok(OwnedReply),
    Err(ErrorCode),
    /// Status 1 in every protocol: the request did not decode.
    Malformed,
}

/// A reply, owned, so it outlives the buffer it was decoded from.
#[derive(Debug, PartialEq, Eq)]
enum OwnedReply {
    Signature(Vec<u8>),
    PublicKey(String, Vec<u8>),
    Holds(u32),
    Granted(u64, Vec<Handle>),
    Released,
}

/// Sends `request` from `caller` and takes the answer apart. `handles` is what the request
/// carried (the protocol asks for none).
fn ask(
    server: &mut KeyServer,
    kernel: &mut FakeKernel,
    caller: &Caller,
    request: &Message<'_>,
    handles: &[Handle],
) -> Answered {
    let mut buf = vec![0u8; 64 * 1024];
    let words = request.encode(&mut buf).expect("the request encodes");
    let opcode = redoubt_rt::wire::typed::opcode(&words).unwrap();
    let received: Vec<Option<Handle>> = handles.iter().copied().map(Some).collect();
    let received = ReceivedHandles::from_slice(&received).unwrap();
    // An inline message carries no buffer, so its caller lends nothing (WIRE.md); a client
    // that lends anyway is refused, which `malformed_requests_are_refused` checks.
    if inline(request) {
        let outcome = answer_with(server, caller, &words, &received, &mut [], kernel);
        return decode(opcode, &outcome, &[]);
    }
    let outcome = answer_with(server, caller, &words, &received, &mut buf, kernel);
    decode(opcode, &outcome, &buf)
}

/// Whether the message's layout is inline, so it travels in its words alone.
fn inline(request: &Message<'_>) -> bool { matches!(request, Message::Grant(_) | Message::Release(_)) }

/// Sends raw words and buffer bytes: for requests no encoder would produce.
fn ask_raw(
    server: &mut KeyServer,
    kernel: &mut FakeKernel,
    caller: &Caller,
    words: &Words,
    body: &[u8],
) -> Answered {
    let mut buf = vec![0u8; 64 * 1024];
    buf[..body.len()].copy_from_slice(body);
    let outcome = answer_with(server, caller, words, &ReceivedHandles::new(), &mut buf, kernel);
    // The opcode may be nonsense; decode against the one the caller meant, or report Malformed.
    decode(redoubt_rt::wire::typed::opcode(words).unwrap_or(0), &outcome, &buf)
}

fn decode(opcode: u32, outcome: &Outcome, buf: &[u8]) -> Answered {
    let handles = outcome.send.as_slice();
    if outcome.words[0] == u64::from(redoubt_rt::wire::typed::MALFORMED) {
        return Answered::Malformed;
    }
    match Reply::decode(opcode, &outcome.words, buf, handles.len()) {
        Ok(Ok(reply)) => Answered::Ok(match reply {
            Reply::SignSshExchange(r) => OwnedReply::Signature(r.signature.to_vec()),
            Reply::SignRecord(r) => OwnedReply::Signature(r.signature.to_vec()),
            Reply::PublicKey(r) => OwnedReply::PublicKey(r.algorithm.into(), r.key.to_vec()),
            Reply::Holds(r) => OwnedReply::Holds(r.held),
            Reply::Grant(r) => OwnedReply::Granted(r.id, handles.to_vec()),
            Reply::Release(_) => OwnedReply::Released,
        }),
        Ok(Err(code)) => Answered::Err(code),
        Err(_) => Answered::Malformed,
    }
}

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

fn signature_of(answered: &Answered) -> &[u8] {
    match answered {
        Answered::Ok(OwnedReply::Signature(bytes)) => bytes,
        other => panic!("expected a signature, got {other:?}"),
    }
}

/// Whether `signature` is a valid Ed25519 signature by `public` over `message`.
fn verifies(public: &[u8; PUBLIC_KEY_LEN], message: &[u8], signature: &[u8]) -> bool {
    let Ok(signature) = <[u8; 64]>::try_from(signature) else { return false };
    PublicKey::new(*public).verify(message, &Signature::new(signature)).is_ok()
}

// ---------------------------------------------------------------- the happy path

/// A signature round-trips through a badge-scoped capability, and it is a signature over what
/// `keyd` built, not over what the caller sent (BUILD-PLAN.md, WP-S1).
#[test]
fn a_signature_round_trips_through_a_badge_scoped_capability() {
    let (mut s, mut k) = (server(), FakeKernel::new());
    let audit = caller(AUDIT_BADGE, 1001, &[]);
    let record = b"lease granted to agent-7";
    let answered = ask(&mut s, &mut k, &audit, &Message::SignRecord(SignRecord { record }), &[]);
    let signature = signature_of(&answered);
    let public = *s.keys().get(1).unwrap().public();

    let signed = audit_digest(record);
    assert!(verifies(&public, &signed, signature), "the signature is over the record's audit digest");
    assert!(!verifies(&public, record, signature), "and not over the record itself");
    // Every signature `keyd` makes is over 32 bytes it hashed itself, so no container that
    // covers longer messages — a bundle tar, a package, an SSH user-auth request — can be what
    // one covers.
    assert_eq!(signed.len(), 32);
    let mut preimage = Vec::from(AUDIT_DOMAIN);
    preimage.extend_from_slice(&(record.len() as u64).to_le_bytes());
    preimage.extend_from_slice(record);
    assert!(!verifies(&public, &preimage, signature), "not over the digest's preimage either");

    // The SSH host key's operation, the same way: over the exchange hash `keyd` computed.
    let host = caller(HOST_BADGE, 0, &[]);
    let answered = ask(&mut s, &mut k, &host, &Message::SignSshExchange(transcript()), &[]);
    let signature = signature_of(&answered);
    let host_public = *s.keys().get(0).unwrap().public();
    let t = transcript();
    let hash = crate::ssh::exchange_hash(
        &crate::ssh::Transcript {
            v_c: t.v_c,
            v_s: t.v_s,
            i_c: t.i_c,
            i_s: t.i_s,
            q_c: t.q_c,
            q_s: t.q_s,
            k: t.k,
        },
        &host_public,
    )
    .unwrap();
    assert!(verifies(&host_public, &hash, signature));
}

#[test]
fn public_key_and_holds_answer_for_the_key_the_badge_names() {
    let (mut s, mut k) = (server(), FakeKernel::new());
    let host_public = s.keys().get(0).unwrap().public().to_vec();
    let audit_public = s.keys().get(1).unwrap().public().to_vec();
    let ask_public = |s: &mut KeyServer, k: &mut FakeKernel, badge| {
        ask(s, k, &caller(badge, 0, &[]), &Message::PublicKey(PublicKeyRequest {}), &[])
    };
    assert_eq!(
        ask_public(&mut s, &mut k, HOST_BADGE),
        Answered::Ok(OwnedReply::PublicKey(ALGORITHM.into(), host_public.clone()))
    );
    assert_eq!(
        ask_public(&mut s, &mut k, AUDIT_BADGE),
        Answered::Ok(OwnedReply::PublicKey(ALGORITHM.into(), audit_public))
    );
    let holds = |s: &mut KeyServer, k: &mut FakeKernel, algorithm: &str, key: &[u8]| {
        ask(s, k, &caller(HOST_BADGE, 0, &[]), &Message::Holds(Holds { algorithm, key }), &[])
    };
    assert_eq!(holds(&mut s, &mut k, ALGORITHM, &host_public), Answered::Ok(OwnedReply::Holds(1)));
    let login = login_key();
    assert_eq!(holds(&mut s, &mut k, ALGORITHM, &login), Answered::Ok(OwnedReply::Holds(0)));
    assert_eq!(holds(&mut s, &mut k, "ssh-rsa", &host_public), Answered::Ok(OwnedReply::Holds(0)));
    assert_eq!(holds(&mut s, &mut k, ALGORITHM, b""), Answered::Ok(OwnedReply::Holds(0)));
}

/// The public half of the key a person logs in with: never in `keyd`.
fn login_key() -> Vec<u8> {
    let keys = Keys::from_args([format!("k,audit,{LOGIN_SEED}")].iter().map(String::as_str)).unwrap();
    keys.get(0).unwrap().public().to_vec()
}

// ---------------------------------------------------------------- attack cases

/// **A caller cannot sign with a key its badge does not name, or for a purpose it does not
/// name.** The two root badges name one key each, with one purpose each; asking the other way
/// round is refused, and no answer ever comes back signed by the other key.
#[test]
fn a_badge_signs_only_its_own_key_and_only_its_own_purpose() {
    let (mut s, mut k) = (server(), FakeKernel::new());
    let host = caller(HOST_BADGE, 0, &[]);
    let audit = caller(AUDIT_BADGE, 1001, &[]);

    // The host badge asking for an audit record, and the audit badge asking for an exchange
    // hash: each is the other's purpose.
    let record = SignRecord { record: b"anything" };
    assert_eq!(
        ask(&mut s, &mut k, &host, &Message::SignRecord(record), &[]),
        Answered::Err(ErrorCode::NotPermitted)
    );
    assert_eq!(
        ask(&mut s, &mut k, &audit, &Message::SignSshExchange(transcript()), &[]),
        Answered::Err(ErrorCode::NotPermitted)
    );

    // A badge that names no key at all: past the end of the manifest, the receive right, a
    // granted badge that was never granted. All the same answer, which says nothing more.
    for badge in [0, 3, 4, u64::MAX, FIRST_GRANTED_BADGE, FIRST_GRANTED_BADGE + 9] {
        for request in [Message::SignRecord(record), Message::PublicKey(PublicKeyRequest {})] {
            assert_eq!(
                ask(&mut s, &mut k, &caller(badge, 1001, &[]), &request, &[]),
                Answered::Err(ErrorCode::NotPermitted),
                "badge {badge}"
            );
        }
    }

    // And what the audit badge does get is by the audit key, never the host key.
    let answered = ask(&mut s, &mut k, &audit, &Message::SignRecord(record), &[]);
    let signature = signature_of(&answered);
    let host_public = *s.keys().get(0).unwrap().public();
    assert!(!verifies(&host_public, &audit_digest(b"anything"), signature), "the host key signed nothing");
}

/// **A request to sign arbitrary bytes is refused.** There is no operation that signs bytes as
/// they came. The attack that matters is a hijacked holder of a `keys` capability relaying an
/// SSH user-authentication request from its peer (answer 95): whatever it does with the blob,
/// no answer verifies as a signature over it, so its peer cannot log in anywhere as the
/// sponsor.
#[test]
fn a_relayed_ssh_user_auth_blob_is_never_what_gets_signed() {
    let (mut s, mut k) = (server(), FakeKernel::new());
    // RFC 4252 §7: string session_id, byte SSH_MSG_USERAUTH_REQUEST, string user, string
    // service, string "publickey", boolean, string algorithm, string key.
    let mut blob = Vec::new();
    let ssh_string = |bytes: &[u8], out: &mut Vec<u8>| {
        out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        out.extend_from_slice(bytes);
    };
    ssh_string(&[0x5a; 32], &mut blob);
    blob.push(50);
    ssh_string(b"alice", &mut blob);
    ssh_string(b"ssh-connection", &mut blob);
    ssh_string(b"publickey", &mut blob);
    blob.push(1);
    ssh_string(ALGORITHM.as_bytes(), &mut blob);
    ssh_string(&[0xab; 32], &mut blob);

    let host_public = *s.keys().get(0).unwrap().public();
    let audit_public = *s.keys().get(1).unwrap().public();

    // Through the audit badge, as a record: signed under the domain string, so not over the
    // blob.
    let answered = ask(
        &mut s,
        &mut k,
        &caller(AUDIT_BADGE, 1001, &[]),
        &Message::SignRecord(SignRecord { record: &blob }),
        &[],
    );
    assert!(!verifies(&audit_public, &blob, signature_of(&answered)));
    assert!(!verifies(&host_public, &blob, signature_of(&answered)));

    // Through the host badge, with the blob pushed into every field of the transcript in turn:
    // what comes back is a signature over a 32-byte hash, and a user-auth blob is never 32
    // bytes.
    let host = caller(HOST_BADGE, 0, &[]);
    let base = transcript();
    let attempts = [
        SignSshExchange { v_c: &blob, ..base },
        SignSshExchange { i_c: &blob, ..base },
        SignSshExchange { i_s: &blob, ..base },
        SignSshExchange { q_c: &blob, ..base },
        SignSshExchange { k: &blob, ..base },
    ];
    for attempt in attempts {
        let answered = ask(&mut s, &mut k, &host, &Message::SignSshExchange(attempt), &[]);
        assert!(!verifies(&host_public, &blob, signature_of(&answered)));
        // Nor over any prefix of it: the signed message is 32 bytes and nothing else.
        for len in [0, 1, 32, 36, blob.len()] {
            assert!(!verifies(&host_public, &blob[..len], signature_of(&answered)), "len {len}");
        }
    }

    // And the caller cannot name another host key in the transcript: `keyd` puts its own in.
    let other = Keys::from_args([format!("k,ssh_host,{LOGIN_SEED}")].iter().map(String::as_str)).unwrap();
    let answered = ask(&mut s, &mut k, &host, &Message::SignSshExchange(base), &[]);
    let with_other = crate::ssh::exchange_hash(
        &crate::ssh::Transcript {
            v_c: base.v_c,
            v_s: base.v_s,
            i_c: base.i_c,
            i_s: base.i_s,
            q_c: base.q_c,
            q_s: base.q_s,
            k: base.k,
        },
        other.get(0).unwrap().public(),
    )
    .unwrap();
    assert!(!verifies(&host_public, &with_other, signature_of(&answered)));
}

/// **No export operation exists at all.** The protocol's opcodes are exactly the six of the
/// table; every other opcode is `Malformed`, so there is nothing to find by looking. And no
/// reply any of the six can produce carries a byte of a seed or of the expanded secret scalar.
#[test]
fn nothing_in_the_protocol_returns_a_key() {
    let (mut s, mut k) = (server(), FakeKernel::new());
    let known: [u32; 6] = [1, 2, 3, 4, 5, 6];
    for opcode in 0..64u32 {
        let mut words = [0u64; 4];
        words[0] = u64::from(opcode);
        let answered = ask_raw(&mut s, &mut k, &caller(HOST_BADGE, 0, &[]), &words, &[]);
        if !known.contains(&opcode) {
            assert_eq!(answered, Answered::Malformed, "opcode {opcode} is not in the table");
        }
    }

    // Every reply of every operation, from every badge, searched for the secrets.
    let seeds: Vec<Vec<u8>> = [HOST_SEED, AUDIT_SEED]
        .iter()
        .map(|hex| {
            (0..SEED_HEX_LEN / 2).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()).collect()
        })
        .collect();
    let requests = [
        Message::SignSshExchange(transcript()),
        Message::SignRecord(SignRecord { record: b"record" }),
        Message::PublicKey(PublicKeyRequest {}),
        Message::Holds(Holds { algorithm: ALGORITHM, key: &[0; 32] }),
        Message::Grant(Grant {}),
        Message::Release(Release { id: 1 }),
    ];
    for badge in [HOST_BADGE, AUDIT_BADGE] {
        for request in requests {
            let mut buf = vec![0u8; 64 * 1024];
            let words = request.encode(&mut buf).unwrap();
            let c = caller(badge, 0, &[]);
            let lend: &mut [u8] = if inline(&request) { &mut [] } else { &mut buf };
            let outcome = answer_with(&mut s, &c, &words, &ReceivedHandles::new(), lend, &mut k);
            let reply: Vec<u8> =
                outcome.words.iter().flat_map(|w| w.to_le_bytes()).chain(buf.iter().copied()).collect();
            for seed in &seeds {
                assert!(
                    !reply.windows(seed.len()).any(|window| window == &seed[..]),
                    "a reply carried a seed"
                );
            }
            // The expanded scalar, too: the first half of SHA-512(seed), clamped, is what
            // signing multiplies by.
            for index in 0..s.keys().len() {
                let scalar = expanded_scalar(&s, index);
                assert!(!reply.windows(32).any(|window| window == scalar), "a reply carried a scalar");
            }
        }
    }
}

/// The clamped secret scalar of the key at `index`: what a leak would look like.
fn expanded_scalar(server: &KeyServer, index: usize) -> [u8; 32] {
    let mut hash = ed25519_compact::sha512::Hash::hash(seed_of(index));
    hash[0] &= 248;
    hash[31] &= 63;
    hash[31] |= 64;
    let mut scalar = [0u8; 32];
    scalar.copy_from_slice(&hash[..32]);
    let _ = server;
    scalar
}

fn seed_of(index: usize) -> [u8; 32] {
    let hex = [HOST_SEED, AUDIT_SEED][index];
    let mut seed = [0u8; 32];
    for (byte, i) in seed.iter_mut().zip(0..) {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
    }
    seed
}

/// **Enrolling a login key is refused**, because there is no enrolment: nothing `keyd` serves
/// adds, replaces or removes a key, so the set it holds after any number of requests is the set
/// the signed manifest gave it. `holds` is how `init` and `sshd` find out what that is
/// (CAPABILITIES.md, approvals).
#[test]
fn no_request_can_put_a_key_into_keyd() {
    let (mut s, mut k) = (server(), FakeKernel::new());
    let login = login_key();
    let before: Vec<Vec<u8>> =
        (0..s.keys().len()).map(|i| s.keys().get(i).unwrap().public().to_vec()).collect();

    // Try to enrol it every way the wire allows: as a record, as a transcript part, as the
    // argument of `holds`, and as raw bytes under every opcode.
    let host = caller(HOST_BADGE, 0, &[]);
    let audit = caller(AUDIT_BADGE, 0, &[]);
    ask(&mut s, &mut k, &audit, &Message::SignRecord(SignRecord { record: &login }), &[]);
    ask(
        &mut s,
        &mut k,
        &host,
        &Message::SignSshExchange(SignSshExchange { q_c: &login, ..transcript() }),
        &[],
    );
    ask(&mut s, &mut k, &host, &Message::Holds(Holds { algorithm: ALGORITHM, key: &login }), &[]);
    for opcode in 0..64u32 {
        let mut words = [0u64; 4];
        words[0] = u64::from(opcode);
        words[1] = login.len() as u64;
        ask_raw(&mut s, &mut k, &host, &words, &login);
    }

    let after: Vec<Vec<u8>> =
        (0..s.keys().len()).map(|i| s.keys().get(i).unwrap().public().to_vec()).collect();
    assert_eq!(before, after, "the key set never changes");
    assert!(!s.keys().holds(ALGORITHM, &login), "the login key is still not held");
    assert_eq!(
        ask(&mut s, &mut k, &host, &Message::Holds(Holds { algorithm: ALGORITHM, key: &login }), &[]),
        Answered::Ok(OwnedReply::Holds(0))
    );
}

/// **A malformed request is refused** with status 1 in this protocol as in every other
/// (WIRE.md), and the handles it brought — which this protocol never asks for — are closed, so
/// a client cannot grow `keyd`'s handle table.
#[test]
fn malformed_requests_are_refused_and_their_handles_closed() {
    let (mut s, mut k) = (server(), FakeKernel::new());
    let host = caller(HOST_BADGE, 0, &[]);
    // Words that are not a request at all.
    for words in [[0u64; 4], [7, 0, 0, 0], [u64::from(u32::MAX), 0, 0, 0], [1, u64::MAX, 0, 0], [5, 1, 1, 1]]
    {
        assert_eq!(ask_raw(&mut s, &mut k, &host, &words, &[]), Answered::Malformed, "{words:?}");
    }
    // A buffer message whose declared length runs past what it carries, and one with trailing
    // bytes the layout does not account for.
    let mut buf = vec![0u8; 64 * 1024];
    let good = Message::SignRecord(SignRecord { record: b"x" }).encode(&mut buf).unwrap();
    let body = buf[..good[1] as usize].to_vec();
    for length in [0u64, 1, 3, good[1] - 1, good[1] + 1, u64::MAX] {
        let words = [good[0], length, 0, 0];
        assert_eq!(ask_raw(&mut s, &mut k, &caller(AUDIT_BADGE, 0, &[]), &words, &body), Answered::Malformed);
    }

    // A request carrying handles the table does not name does not decode, so it is malformed
    // like any other, and the handles come back to be closed rather than staying in `keyd`'s
    // table (CONTAINMENT.md: a client cannot grow a server's handle table).
    let brought = [Some(Handle::new(41).unwrap()), Some(Handle::new(42).unwrap())];
    let received = ReceivedHandles::from_slice(&brought).unwrap();
    let mut buf = vec![0u8; 64 * 1024];
    let words = Message::PublicKey(PublicKeyRequest {}).encode(&mut buf).unwrap();
    let outcome = answer_with(&mut s, &host, &words, &received, &mut buf, &mut k);
    assert_eq!(outcome.words, redoubt_rt::server::MALFORMED);
    assert_eq!(outcome.close.as_slice(), [Handle::new(41).unwrap(), Handle::new(42).unwrap()]);
    assert!(outcome.send.as_slice().is_empty());
    // A handle missing from its slot (revoked on the way, R10) is the same answer.
    let revoked = ReceivedHandles::from_slice(&[Some(Handle::new(41).unwrap()), None]).unwrap();
    let outcome = answer_with(&mut s, &host, &words, &revoked, &mut buf, &mut k);
    assert_eq!(outcome.words, redoubt_rt::server::MALFORMED);
    assert_eq!(outcome.close.as_slice(), [Handle::new(41).unwrap()]);
    // And an inline request that came with a lend all the same: inline messages have no buffer.
    let words = Message::Grant(Grant {}).encode(&mut []).unwrap();
    let outcome = answer_with(&mut s, &host, &words, &ReceivedHandles::new(), &mut buf, &mut k);
    assert_eq!(outcome.words, redoubt_rt::server::MALFORMED);
}

/// **Flooding.** `grant` is the one thing a client can make `keyd` hold, and it is capped per
/// (account, label set) with a fair share per badge, so an agent cannot lock out the sponsor it
/// shares a bucket with, and another account is not affected at all. Signing requests hold
/// nothing, so a flood of them leaves `keyd` exactly as big as it was.
#[test]
fn a_flood_takes_only_the_flooders_share() {
    let (mut s, mut k) = (server(), FakeKernel::new());
    let agent = caller(AUDIT_BADGE, 1001, &[]);
    let sponsor = Caller { badge: HOST_BADGE, ..agent };
    let grant = Message::Grant(Grant {});

    // The agent grants as hard as it can. Alone in the bucket, its share is half the cap.
    let mut agent_got = 0;
    while let Answered::Ok(OwnedReply::Granted(..)) = ask(&mut s, &mut k, &agent, &grant, &[]) {
        agent_got += 1;
        assert!(agent_got <= LIMITS.state, "the cap holds");
    }
    assert_eq!(agent_got, LIMITS.state / 2, "half the bucket, so the sponsor still fits");
    assert!(matches!(ask(&mut s, &mut k, &agent, &grant, &[]), Answered::Err(ErrorCode::TooMany)));
    // The sponsor, sharing the bucket, still gets a share of its own.
    assert!(matches!(ask(&mut s, &mut k, &sponsor, &grant, &[]), Answered::Ok(OwnedReply::Granted(..))));
    // Another account is untouched.
    let other = caller(AUDIT_BADGE, 2002, &[]);
    assert!(matches!(ask(&mut s, &mut k, &other, &grant, &[]), Answered::Ok(OwnedReply::Granted(..))));

    // A flood of signing requests makes `keyd` hold nothing.
    let held = s.granted();
    for _ in 0..500 {
        let answered = ask(&mut s, &mut k, &agent, &Message::SignRecord(SignRecord { record: b"spam" }), &[]);
        assert!(matches!(answered, Answered::Ok(OwnedReply::Signature(_))));
    }
    assert_eq!(s.granted(), held, "signing holds nothing");
}

/// **Only a root badge may grant.** A granted capability cannot grant again, so grants never
/// chain. That is what stops a system-class caller escaping its cap: `admit` keys account 0 by
/// badge, and `share` folds a capability into its parent's only when the requester and the
/// caller are the same client, which two badges of account 0 never are — so a chain would open
/// a fresh bucket per link until `LIMITS.buckets` were spent and nobody could grant at all.
#[test]
fn a_granted_capability_cannot_grant_again() {
    let (mut s, mut k) = (server(), FakeKernel::new());
    let grant = Message::Grant(Grant {});
    let daemon = caller(HOST_BADGE, 0, &[]);
    let Answered::Ok(OwnedReply::Granted(..)) = ask(&mut s, &mut k, &daemon, &grant, &[]) else {
        panic!("grant")
    };
    let badge = *k.minted.last().unwrap();
    let through = Caller { badge, ..daemon };
    // It still does everything its purpose allows, but it cannot make another.
    assert!(matches!(
        ask(&mut s, &mut k, &through, &Message::PublicKey(PublicKeyRequest {}), &[]),
        Answered::Ok(OwnedReply::PublicKey(..))
    ));
    assert_eq!(ask(&mut s, &mut k, &through, &grant, &[]), Answered::Err(ErrorCode::NotPermitted));

    // The red team's attack: one account-0 badge chaining grants until every bucket is spent.
    // It gets no further than its own root badge's share.
    let mut frontier = vec![HOST_BADGE];
    let mut made = 0;
    while let Some(badge) = frontier.pop() {
        let who = caller(badge, 0, &[]);
        while let Answered::Ok(OwnedReply::Granted(..)) = ask(&mut s, &mut k, &who, &grant, &[]) {
            made += 1;
            frontier.push(*k.minted.last().unwrap());
            assert!(made < 100, "it should have run out long before this");
        }
    }
    assert_eq!(s.admission().keys(), 1, "one bucket, not sixteen");
    // And every other client can still grant.
    for who in [caller(AUDIT_BADGE, 0, &[]), caller(AUDIT_BADGE, 2002, &[]), caller(HOST_BADGE, 3003, &[])] {
        assert!(matches!(ask(&mut s, &mut k, &who, &grant, &[]), Answered::Ok(OwnedReply::Granted(..))));
    }
}

/// `grant` and `release`: a fresh capability per client, naming the same key and the same
/// purpose and no more, released by the one that asked for it and by nobody else. Badge numbers
/// never come back (answer 86).
#[test]
fn a_granted_capability_names_the_same_key_and_dies_with_release() {
    let (mut s, mut k) = (server(), FakeKernel::new());
    let steward = caller(AUDIT_BADGE, 0, &[]);
    let grant = Message::Grant(Grant {});

    let Answered::Ok(OwnedReply::Granted(id, handles)) = ask(&mut s, &mut k, &steward, &grant, &[]) else {
        panic!("grant")
    };
    assert_eq!(handles.len(), 1, "the capability travels as a handle");
    let badge = *k.minted.last().unwrap();
    assert!(badge >= FIRST_GRANTED_BADGE, "granted badges are never root badges");
    // The same key and the same purpose, and no other.
    let through = Caller { badge, ..steward };
    assert!(matches!(
        ask(&mut s, &mut k, &through, &Message::SignRecord(SignRecord { record: b"r" }), &[]),
        Answered::Ok(OwnedReply::Signature(_))
    ));
    assert!(matches!(
        ask(&mut s, &mut k, &through, &Message::SignSshExchange(transcript()), &[]),
        Answered::Err(ErrorCode::NotPermitted)
    ));

    // A stranger cannot release it, and neither can the same client through another badge: the
    // same answer whether the id is somebody else's or nobody's.
    for (who, id) in
        [(caller(AUDIT_BADGE, 2002, &[]), id), (caller(AUDIT_BADGE, 0, &[5]), id), (steward, id ^ 1)]
    {
        assert_eq!(
            ask(&mut s, &mut k, &who, &Message::Release(Release { id }), &[]),
            Answered::Err(ErrorCode::NotPermitted)
        );
    }
    assert_eq!(s.granted(), 1, "and nothing was freed by trying");

    assert_eq!(
        ask(&mut s, &mut k, &steward, &Message::Release(Release { id }), &[]),
        Answered::Ok(OwnedReply::Released)
    );
    assert_eq!(s.granted(), 0);
    assert_eq!(
        ask(&mut s, &mut k, &Caller { badge, ..steward }, &Message::PublicKey(PublicKeyRequest {}), &[]),
        Answered::Err(ErrorCode::NotPermitted),
        "the released badge names nothing"
    );
    // The next grant gets a new badge, never that one.
    let _ = ask(&mut s, &mut k, &steward, &grant, &[]);
    assert!(*k.minted.last().unwrap() > badge, "badges only go up");
}

/// **`release(0)` frees everything this caller granted.** A holder that crashed and was
/// restarted on the same root badge (INIT.md decision 4) knows none of its ids, and only the
/// holder of an id can name a capability, so without this its share would stay full for the
/// life of `keyd`. No grant is ever given the id 0, so it can name nothing else.
#[test]
fn release_zero_frees_everything_this_caller_granted() {
    let (mut s, mut k) = (server(), FakeKernel::new());
    let grant = Message::Grant(Grant {});
    let steward = caller(AUDIT_BADGE, 0, &[]);
    let other = caller(HOST_BADGE, 0, &[]);
    let mut mine = 0;
    while let Answered::Ok(OwnedReply::Granted(..)) = ask(&mut s, &mut k, &steward, &grant, &[]) {
        mine += 1;
    }
    assert_eq!(mine, LIMITS.state, "account 0 has a bucket of its own, undivided");
    let Answered::Ok(OwnedReply::Granted(theirs, _)) = ask(&mut s, &mut k, &other, &grant, &[]) else {
        panic!("grant")
    };
    assert_eq!(s.granted(), mine as usize + 1);

    // The steward crashes; `init` restarts it with the same root handle, so the same badge,
    // account and labels: it asks for everything back.
    assert_eq!(
        ask(&mut s, &mut k, &steward, &Message::Release(Release { id: ALL_GRANTS }), &[]),
        Answered::Ok(OwnedReply::Released)
    );
    assert_eq!(s.granted(), 1, "only the other badge's is left");
    assert_eq!(s.admission().held(AdmitKey::of(&steward), Resource::State), 0, "its slots came back");
    // And it can grant again, as it could before it died.
    assert!(matches!(ask(&mut s, &mut k, &steward, &grant, &[]), Answered::Ok(OwnedReply::Granted(..))));
    // Asking again frees nothing, and never somebody else's.
    assert_eq!(
        ask(&mut s, &mut k, &other, &Message::Release(Release { id: ALL_GRANTS }), &[]),
        Answered::Ok(OwnedReply::Released)
    );
    assert_eq!(
        ask(&mut s, &mut k, &other, &Message::Release(Release { id: theirs }), &[]),
        Answered::Err(ErrorCode::NotPermitted),
        "it is gone, and a stale id is still nobody's"
    );
}

/// A grant whose reply never reaches its caller is undone. The requester never learns the id,
/// and `release` answers only the holder of an id, so the record and its admission slot would
/// otherwise be held for the life of the process; a client could fill its own bucket by dying
/// mid-grant. `serve` calls this when `finish` says the reply did not go, and the 9P skeleton
/// does the same with the same table.
#[test]
fn a_grant_whose_reply_never_arrives_is_undone() {
    let (mut s, mut k) = (server(), FakeKernel::new());
    let who = caller(AUDIT_BADGE, 1001, &[]);
    let key = AdmitKey::of(&who);
    assert!(matches!(ask(&mut s, &mut k, &who, &Message::Grant(Grant {}), &[]), Answered::Ok(_)));
    let badge = s.granted.minted_here().expect("the grant records what it made");
    assert_eq!(s.granted(), 1);
    assert_eq!(s.admission().held(key, Resource::State), 1);

    s.forget_badge(badge);
    assert_eq!(s.granted(), 0);
    assert_eq!(s.admission().held(key, Resource::State), 0, "its slot came back");
}

/// A grant that cannot be minted leaves nothing behind: no record, and the admission it took
/// back where it was.
#[test]
fn a_grant_that_fails_gives_its_admission_back() {
    let (mut s, mut k) = (server(), FakeKernel::new());
    let who = caller(AUDIT_BADGE, 1001, &[]);
    k.mint_fails = true;
    assert_eq!(ask(&mut s, &mut k, &who, &Message::Grant(Grant {}), &[]), Answered::Err(ErrorCode::Failed));
    assert_eq!(s.granted(), 0);
    k.mint_fails = false;
    let mut got = 0;
    while let Answered::Ok(OwnedReply::Granted(..)) =
        ask(&mut s, &mut k, &who, &Message::Grant(Grant {}), &[])
    {
        got += 1;
    }
    assert_eq!(got, LIMITS.state / 2, "the failed attempt cost nothing");
}

/// The label check on every request (CONTAINMENT.md). `keyd`'s keys are unlabelled in milestone
/// 1, so a labelled caller may read a public key (no read up: ∅ ⊆ anything) and may not sign,
/// grant or release (no write down). That is a stated milestone 1 consequence, in INIT.md: a
/// vault session that must sign needs a labelled key, which is milestone 2.
#[test]
fn a_labelled_caller_reads_but_does_not_sign() {
    let (mut s, mut k) = (server(), FakeKernel::new());
    let vault = caller(AUDIT_BADGE, 1001, &[5]);
    assert!(matches!(
        ask(&mut s, &mut k, &vault, &Message::PublicKey(PublicKeyRequest {}), &[]),
        Answered::Ok(OwnedReply::PublicKey(..))
    ));
    assert!(matches!(
        ask(&mut s, &mut k, &vault, &Message::Holds(Holds { algorithm: ALGORITHM, key: &[0; 32] }), &[]),
        Answered::Ok(OwnedReply::Holds(0))
    ));
    for request in [
        Message::SignRecord(SignRecord { record: b"secret" }),
        Message::Grant(Grant {}),
        Message::Release(Release { id: 7 }),
    ] {
        assert_eq!(ask(&mut s, &mut k, &vault, &request, &[]), Answered::Err(ErrorCode::NotPermitted));
    }
    // The unlabelled side of the same account is unaffected.
    let plain = caller(AUDIT_BADGE, 1001, &[]);
    assert!(matches!(
        ask(&mut s, &mut k, &plain, &Message::SignRecord(SignRecord { record: b"secret" }), &[]),
        Answered::Ok(OwnedReply::Signature(_))
    ));
}

/// One request's work is bounded by something stated, not by the size of the buffer that
/// carried it.
#[test]
fn one_requests_work_is_bounded() {
    let (mut s, mut k) = (server(), FakeKernel::new());
    let audit = caller(AUDIT_BADGE, 0, &[]);
    let long = vec![0u8; MAX_RECORD + 1];
    assert_eq!(
        ask(&mut s, &mut k, &audit, &Message::SignRecord(SignRecord { record: &long }), &[]),
        Answered::Err(ErrorCode::TooMany)
    );
    let ok = vec![0u8; MAX_RECORD];
    assert!(matches!(
        ask(&mut s, &mut k, &audit, &Message::SignRecord(SignRecord { record: &ok }), &[]),
        Answered::Ok(OwnedReply::Signature(_))
    ));
    let host = caller(HOST_BADGE, 0, &[]);
    let long = vec![0u8; crate::ssh::MAX_PART + 1];
    assert_eq!(
        ask(
            &mut s,
            &mut k,
            &host,
            &Message::SignSshExchange(SignSshExchange { i_c: &long, ..transcript() }),
            &[]
        ),
        Answered::Err(ErrorCode::TooMany)
    );
}

/// The limits this build ships are the ones CONTAINMENT.md asks for: every bucket at its cap
/// fits the budget, the open calls they allow leave the headroom, and a cap can seat a share.
#[test]
fn the_limits_are_sized_as_containment_says() {
    assert!(LIMITS.fits(&COST, BUDGET));
    let keys = || Keys::from_args([format!("k,audit,{AUDIT_SEED}")].iter().map(String::as_str)).unwrap();
    assert!(KeyServer::new(keys(), LIMITS, &COST, BUDGET, TEST_RANDOM).is_ok());
    // Every bucket at its cap: what the budget must cover, and one byte less is refused rather
    // than rounded (answer 85).
    let need = u64::from(LIMITS.buckets) * u64::from(LIMITS.state) * COST.state;
    assert!(need <= BUDGET, "the shipped budget covers the shipped caps");
    assert!(KeyServer::new(keys(), LIMITS, &COST, need, TEST_RANDOM).is_ok());
    assert!(KeyServer::new(keys(), LIMITS, &COST, need - 1, TEST_RANDOM).is_err());
    // A cap of one cannot seat a share and its sponsor.
    assert!(KeyServer::new(keys(), Limits { state: 1, ..LIMITS }, &COST, BUDGET, TEST_RANDOM).is_err());
    // `keyd` parks no calls, so it asks for no open calls beyond the ones it is answering.
    assert_eq!(LIMITS.in_flight, 0);
    assert_eq!(LIMITS.open_calls(), 0);
}

/// Random words, from every badge, never panic and never answer anything but a reply of this
/// protocol or one of its errors.
#[test]
fn random_requests_never_panic() {
    let (mut s, mut k) = (server(), FakeKernel::new());
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let mut body = vec![0u8; 4096];
    for round in 0..20_000 {
        let r = next();
        let words = [r % 9, next() % 4096, next(), next()];
        for (i, byte) in body.iter_mut().enumerate() {
            *byte = (r >> (i % 56)) as u8 ^ i as u8;
        }
        let badge = [0, HOST_BADGE, AUDIT_BADGE, FIRST_GRANTED_BADGE, next()][round % 5];
        let c = caller(badge, round as u64 % 3, &[]);
        let _ = ask_raw(&mut s, &mut k, &c, &words, &body);
    }
    // And nothing accumulated.
    assert_eq!(s.granted(), 0);
}
