//! `ninep_common` on the 9P skeleton, driven through `answer_common` with a fake minter (no
//! system calls): `new_connection` and `disconnect`, admission and fair shares of minted
//! connections, the file server's grant and disconnect hooks, and hostile requests.

use core::num::NonZeroU64;

use super::*;

/// A kernel for `answer_common`: mints handles 100, 101, ... and remembers the badges.
struct FakeKernel {
    minted: Vec<u64>,
    rng: u64,
}

impl FakeKernel {
    fn new() -> FakeKernel { FakeKernel { minted: Vec::new(), rng: 0x9e37_79b9_7f4a_7c15 } }
}

impl Minter for FakeKernel {
    fn mint(&mut self, badge: NonZeroU64) -> Result<Handle, Error> {
        self.minted.push(badge.get());
        Ok(Handle::new(99 + self.minted.len() as u32).unwrap())
    }

    fn random(&mut self) -> Result<u64, Error> {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        Ok(self.rng)
    }
}

fn h(i: u32) -> Handle { Handle::new(i).unwrap() }

impl T {
    /// Answers a `ninep_common` request from `who` bringing `handles`: the outcome and the lend.
    fn common(
        &mut self,
        k: &mut FakeKernel,
        who: &Caller,
        message: ninep_common::Message<'_>,
        handles: &[Handle],
    ) -> (Outcome, Vec<u8>) {
        // `disconnect` is inline: it travels with no lend (one would make it malformed).
        let size = if matches!(message, ninep_common::Message::Disconnect(_)) { 0 } else { 4096 };
        let mut lend = vec![0u8; size];
        let words = message.encode(&mut lend).unwrap();
        let handles = Handles::from_slice(handles).unwrap();
        (self.server.answer_common(who, &words, &handles, &mut lend, k), lend)
    }

    /// `new_connection` from `who`: the new connection's badge and id, or the error status.
    fn connect(
        &mut self,
        k: &mut FakeKernel,
        who: &Caller,
        root: &str,
        quota: u64,
    ) -> Result<(u64, u64), u32> {
        let message = ninep_common::Message::NewConnection(ninep_common::NewConnection { root, quota });
        let (outcome, lend) = self.common(k, who, message, &[]);
        if outcome.words[0] != 0 {
            assert!(outcome.send.as_slice().is_empty() && outcome.close.as_slice().is_empty());
            return Err(outcome.words[0] as u32);
        }
        let sent = outcome.send.as_slice();
        let reply = ninep_common::Reply::decode(2, &outcome.words, &lend, sent.len()).unwrap().unwrap();
        let ninep_common::Reply::NewConnection(reply) = reply else { panic!("{reply:?}") };
        // One handle travels, and the server's own copy is closed once it has.
        assert_eq!(sent, [h(99 + k.minted.len() as u32)]);
        assert_eq!(outcome.close.as_slice(), sent);
        Ok((*k.minted.last().unwrap(), reply.id))
    }

    fn disconnect(&mut self, k: &mut FakeKernel, who: &Caller, id: u64) -> Result<(), u32> {
        let message = ninep_common::Message::Disconnect(ninep_common::Disconnect { id });
        let (outcome, _) = self.common(k, who, message, &[]);
        assert!(outcome.send.as_slice().is_empty() && outcome.close.as_slice().is_empty());
        match outcome.words {
            [0, 0, 0, 0] => Ok(()),
            [status, 0, 0, 0] => Err(status as u32),
            other => panic!("{other:?}"),
        }
    }
}

/// `ninep_common`'s `refused`.
const REFUSED: u32 = 3;

/// The caller using connection `badge`, in `who`'s account and label set.
fn through(who: &Caller, badge: u64) -> Caller { Caller { badge, ..*who } }

#[test]
fn new_connection_is_rooted_below_the_callers_root() {
    let (mut t, mut k) = (T::new(), FakeKernel::new());
    let a = alice();
    let (b, _) = t.connect(&mut k, &a, "a/b/../b", 0).unwrap();
    let child = through(&a, b);
    t.attach(&child, 0, "anything: the badge decides");
    assert_eq!(t.walk(&child, 0, 1, &["..", "..", "f"]), vec![2, 2, 3], "`..` stops at the new root");
    // From there, `..` in the root path cannot climb either.
    let (up, _) = t.connect(&mut k, &child, "../../..", 0).unwrap();
    t.attach(&through(&a, up), 0, "");
    assert_eq!(t.walk(&through(&a, up), 0, 1, &["f"]), vec![3]);
    // A root may be a file, as a console's is.
    let (file, _) = t.connect(&mut k, &a, "notes", 0).unwrap();
    t.attach(&through(&a, file), 0, "");
    t.open(&through(&a, file), 0, mode::OREAD).unwrap();
    assert_eq!(t.read(&through(&a, file), 0, 0, 5).unwrap(), b"hello");
    // Walked as a Twalk is: nothing missing, nothing the caller cannot read, no bad names.
    for root in ["nope", "vault", "vault/key", "secret", "notes/x", "a\0b"] {
        assert_eq!(t.connect(&mut k, &a, root, 0), Err(REFUSED), "{root:?}");
    }
    // A labelled caller may root a connection in the vault.
    let vault = caller(ALICE, 1001, &[7]);
    t.connect(&mut k, &vault, "vault", 0).unwrap();
    // A badge the skeleton never minted is no connection.
    let ghost = through(&a, FIRST_MINTED_BADGE + 1000);
    assert_eq!(t.connect(&mut k, &ghost, "", 0), Err(REFUSED));
    let attach = Body::Tattach { fid: 0, afid: NOFID, uname: "", aname: "" };
    assert_eq!(t.err(&ghost, attach), "no such connection");
}

#[test]
fn a_disconnect_frees_its_fids_and_every_connection_minted_under_it() {
    let (mut t, mut k) = (T::new(), FakeKernel::new());
    let a = alice();
    let (c1, id1) = t.connect(&mut k, &a, "", 0).unwrap();
    let (c3, _) = t.connect(&mut k, &a, "a", 0).unwrap();
    // The child on c1 opens files and mints a connection of its own, which has a child too.
    let child = through(&a, c1);
    t.attach(&child, 0, "");
    t.walk(&child, 0, 1, &["notes"]);
    let (c2, _) = t.connect(&mut k, &child, "a", 0).unwrap();
    let grandchild = through(&a, c2);
    t.attach(&grandchild, 0, "");
    let (c4, _) = t.connect(&mut k, &grandchild, "b", 0).unwrap();
    t.attach(&through(&a, c4), 0, "");
    t.attach(&through(&a, c3), 0, "");
    let key = AdmitKey::of(&a);
    assert_eq!(t.server.admission().held(key, Resource::Files), 5);
    assert_eq!(t.server.admission().held(key, Resource::State), 4);
    t.server.fs.clunked.clear();
    // Only alice holds id1; she frees c1, c2 and c4, and nothing of c3.
    assert_eq!(t.disconnect(&mut k, &a, id1), Ok(()));
    let mut clunked = t.server.fs.clunked.clone();
    clunked.sort();
    assert_eq!(clunked, [0, 1, 2, 4]);
    assert_eq!(t.server.connections(), 1);
    assert_eq!(t.server.admission().held(key, Resource::Files), 1);
    assert_eq!(t.server.admission().held(key, Resource::State), 1);
    for gone in [c1, c2, c4] {
        let who = through(&a, gone);
        let attach = Body::Tattach { fid: 9, afid: NOFID, uname: "", aname: "" };
        assert_eq!(t.err(&who, attach), "no such connection");
        assert_eq!(t.err(&who, Body::Tstat { fid: 0 }), "unknown fid");
        assert_eq!(t.connect(&mut k, &who, "", 0), Err(REFUSED));
    }
    assert_eq!(t.walk(&through(&a, c3), 0, 1, &["b"]), vec![2]);
    // The id is spent.
    assert_eq!(t.disconnect(&mut k, &a, id1), Err(NOT_YOURS));
}

/// Red team C1: a disconnect frees a whole subtree in place, however its connections were
/// interleaved with others when minted, and leaves no connection under one that is gone.
#[test]
fn a_disconnect_frees_an_interleaved_subtree_and_leaves_no_orphan() {
    let (mut t, mut k) = (T::new(), FakeKernel::new());
    let (a, b) = (alice(), caller(2, 2002, &[]));
    let (ca, ida) = t.connect(&mut k, &a, "", 0).unwrap();
    let (cb, _) = t.connect(&mut k, &b, "", 0).unwrap();
    let (ca1, _) = t.connect(&mut k, &through(&a, ca), "", 0).unwrap();
    let (cb1, _) = t.connect(&mut k, &through(&b, cb), "", 0).unwrap();
    let (ca1a, _) = t.connect(&mut k, &through(&a, ca1), "", 0).unwrap();
    let (ca2, _) = t.connect(&mut k, &through(&a, ca), "", 0).unwrap();
    let (cb2, _) = t.connect(&mut k, &through(&b, cb1), "", 0).unwrap();
    // (Alice's share holds four connections: half the bucket's eight.)
    for c in [ca, ca1, ca1a, ca2] {
        t.attach(&through(&a, c), 0, "");
    }
    t.disconnect(&mut k, &a, ida).unwrap();
    let left: Vec<u64> = t.server.fs.grants.iter().map(|(badge, _, _)| *badge).collect();
    assert_eq!(left, [cb, cb1, cb2]);
    assert_eq!(t.server.connections(), 3);
    assert_eq!(t.server.admission().held(AdmitKey::of(&a), Resource::Files), 0);
    assert_eq!(t.server.admission().held(AdmitKey::of(&a), Resource::State), 0);
    for gone in [ca, ca1, ca1a, ca2] {
        assert_eq!(t.connect(&mut k, &through(&a, gone), "", 0), Err(REFUSED));
    }
    // B's subtree is whole and still works.
    t.attach(&through(&b, cb2), 0, "");
}

/// Red team: connections an agent mints for itself add fids to one share, never new shares.
#[test]
fn self_minting_does_not_multiply_the_share() {
    let (mut t, mut k) = (T::new(), FakeKernel::new());
    let steward = caller(9, 0, &[]);
    let (agent_conn, _) = t.connect(&mut k, &steward, "", 0).unwrap();
    let agent = caller(agent_conn, 1001, &[]);
    let mut chain = vec![agent_conn];
    while let Ok((c, _)) = t.connect(&mut k, &through(&agent, *chain.last().unwrap()), "", 0) {
        chain.push(c);
    }
    assert!(chain.len() >= 2);
    for &c in &chain {
        t.attach(&through(&agent, c), 0, "");
    }
    let key = AdmitKey::of(&agent);
    let held = t.server.admission().held(key, Resource::Files);
    assert_eq!(held, chain.len() as u32);
    assert_eq!(t.server.admission().held_by(key, agent_conn, Resource::Files), held);
}

#[test]
fn a_strangers_id_is_refused_like_one_that_does_not_exist() {
    let (mut t, mut k) = (T::new(), FakeKernel::new());
    let a = alice();
    let (c, id) = t.connect(&mut k, &a, "", 0).unwrap();
    t.attach(&through(&a, c), 0, "");
    let strangers = [
        caller(2, 2002, &[]),      // another account
        caller(5, 1001, &[]),      // the same account, another connection
        caller(ALICE, 1001, &[7]), // a copy of alice's handle in another label set
        caller(ALICE, 2002, &[]),  // or in another account
        through(&a, c),            // the connection itself
    ];
    let nobody = t.disconnect(&mut k, &a, id ^ 1);
    assert_eq!(nobody, Err(NOT_YOURS));
    for stranger in &strangers {
        assert_eq!(t.disconnect(&mut k, stranger, id), nobody, "{stranger:?}");
    }
    assert_eq!(t.server.fids(&through(&a, c)), 1);
    assert_eq!(t.disconnect(&mut k, &a, id), Ok(()));
}

#[test]
fn badges_are_never_reused_and_ids_are_random() {
    let (mut t, mut k) = (T::new(), FakeKernel::new());
    let a = alice();
    let mut seen: Vec<(u64, u64)> = Vec::new();
    for _ in 0..20 {
        let (badge, id) = t.connect(&mut k, &a, "", 0).unwrap();
        assert!(badge >= FIRST_MINTED_BADGE && id != 0);
        assert!(seen.iter().all(|(b, i)| *b < badge && *i != id));
        seen.push((badge, id));
        t.disconnect(&mut k, &a, id).unwrap();
        // A handle to the old badge still arriving finds no connection.
        let attach = Body::Tattach { fid: 0, afid: NOFID, uname: "", aname: "" };
        assert_eq!(t.err(&through(&a, badge), attach), "no such connection");
    }
    // Ids are not a counter.
    assert!(seen.windows(2).all(|w| w[1].1 != w[0].1.wrapping_add(1)));
}

#[test]
fn unasked_handles_are_closed_and_other_opcodes_are_malformed() {
    let (mut t, mut k) = (T::new(), FakeKernel::new());
    let a = alice();
    let brought = [h(5), h(6)];
    for message in [
        ninep_common::Message::NewConnection(ninep_common::NewConnection { root: "", quota: 0 }),
        ninep_common::Message::Disconnect(ninep_common::Disconnect { id: 1 }),
    ] {
        let (outcome, _) = t.common(&mut k, &a, message, &brought);
        assert_eq!(outcome.words, MALFORMED);
        assert!(outcome.send.as_slice().is_empty());
        assert_eq!(outcome.close.as_slice(), brought);
    }
    assert!(k.minted.is_empty());
    // NINEP_COMMON_OPCODES (1-15) are ninep_common's; those it does not define are
    // malformed, and their handles closed too.
    let handles = Handles::from_slice(&brought).unwrap();
    for opcode in [1, 4, 15] {
        let mut lend = vec![0u8; 64];
        let outcome = t.server.answer_common(&a, &[opcode, 0, 0, 0], &handles, &mut lend, &mut k);
        assert_eq!((outcome.words, outcome.close.as_slice()), (MALFORMED, &brought[..]));
    }
    assert!(NINEP_COMMON_OPCODES.contains(&15) && !NINEP_COMMON_OPCODES.contains(&16));
    // A new_connection with no lend to carry its root is malformed as well.
    let words = [2, 8, 0, 0];
    let outcome = t.server.answer_common(&a, &words, &Handles::new(), &mut [], &mut k);
    assert_eq!(outcome.words, MALFORMED);
}

#[test]
fn minted_connections_are_admitted_and_fold_into_the_share_they_came_from() {
    // Answer 90's attack on a 9P server: an agent in its sponsor's bucket floods it with fids,
    // through as many connections as it can mint for itself; its sponsor still opens files.
    let mut t = T::with_limit(12);
    let mut k = FakeKernel::new();
    let steward = caller(9, 0, &[]);
    let (agent_conn, _) = t.connect(&mut k, &steward, "", 0).unwrap();
    let (alice_conn, _) = t.connect(&mut k, &steward, "", 0).unwrap();
    let agent = caller(agent_conn, 1001, &[]);
    let mut mine = vec![agent_conn];
    // Connections the agent mints for itself count in its own share: half the bucket's 8.
    while let Ok((c, _)) = t.connect(&mut k, &through(&agent, *mine.last().unwrap()), "", 0) {
        mine.push(c);
    }
    assert_eq!(mine.len(), 1 + 4);
    let mut fid = 0;
    for &c in &mine {
        loop {
            let attach = Body::Tattach { fid, afid: NOFID, uname: "", aname: "" };
            if t.rpc(&through(&agent, c), attach) == (Body::Rerror { ename: "too many open files" }) {
                break;
            }
            fid += 1;
        }
    }
    assert_eq!(fid, 6, "the agent's connections together hold its share: half of 12");
    let alice = caller(alice_conn, 1001, &[]);
    for fid in 0..3 {
        t.attach(&alice, fid, "");
    }
    t.walk(&alice, 0, 10, &["notes"]);
    t.open(&alice, 10, mode::OREAD).unwrap();
    assert_eq!(t.read(&alice, 10, 0, 5).unwrap(), b"hello");
}

#[test]
fn a_client_at_its_connection_cap_costs_the_server_no_walk() {
    // Admission comes before the root is walked, as it does for fids.
    let (mut t, mut k) = (T::new(), FakeKernel::new());
    let a = alice();
    while t.connect(&mut k, &a, "", 0).is_ok() {}
    let walks = t.server.fs.walks;
    for _ in 0..10 {
        assert_eq!(t.connect(&mut k, &a, "a/b/f", 0), Err(REFUSED));
    }
    assert_eq!(t.server.fs.walks, walks);
    // Another account's share is untouched.
    t.connect(&mut k, &caller(2, 2002, &[]), "a/b", 0).unwrap();
}

/// QUESTIONS.md 118: byte quotas are the file server's; the skeleton hands it every grant and
/// every disconnect.
#[test]
fn a_quota_is_the_file_servers_to_grant_and_a_disconnect_reaches_it() {
    let (mut t, mut k) = (T::new(), FakeKernel::new());
    let a = alice();
    let (c, id) = t.connect(&mut k, &a, "a", 60).unwrap();
    assert_eq!(t.server.fs.grants, [(c, 1, 60)]);
    // The server refuses a grant: nothing is minted, the admission comes back.
    assert_eq!(t.connect(&mut k, &a, "", REFUSED_QUOTA), Err(REFUSED));
    assert_eq!(k.minted, [c]);
    assert_eq!(t.server.admission().held(AdmitKey::of(&a), Resource::State), 1);
    // Disconnecting reaches it, for the whole subtree.
    let (g, _) = t.connect(&mut k, &through(&a, c), "b", 0).unwrap();
    assert_eq!(t.server.fs.grants, [(c, 1, 60), (g, 2, 0)]);
    t.disconnect(&mut k, &a, id).unwrap();
    assert!(t.server.fs.grants.is_empty());
}

#[test]
fn random_common_requests_never_panic() {
    let (mut t, mut k) = (T::with_limit(40), FakeKernel::new());
    let mut x = 0x0123_4567_89ab_cdef_u64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let mut ids = vec![0u64];
    for _ in 0..20_000 {
        let r = next();
        let badge = if r & 1 == 0 { r >> 60 } else { FIRST_MINTED_BADGE + (r >> 58) % 32 };
        let who = caller(badge, 1000 + (r >> 62), if r & 2 == 0 { &[] } else { &[7] });
        if r & 4 == 0 {
            let root = ["", "a", "..", "vault", "a/b", "notes", "x"][(r >> 40) as usize % 7];
            let quota = [0, 1, 50, u64::MAX][(r >> 44) as usize % 4];
            let message = if r & 8 == 0 {
                ninep_common::Message::NewConnection(ninep_common::NewConnection { root, quota })
            } else {
                let id = ids[(r >> 48) as usize % ids.len()];
                ninep_common::Message::Disconnect(ninep_common::Disconnect { id })
            };
            let mut lend = vec![0; if r & 8 == 0 { 64 } else { 0 }];
            let words = message.encode(&mut lend).unwrap();
            let outcome = t.server.answer_common(&who, &words, &Handles::new(), &mut lend, &mut k);
            if outcome.words == [0, 8, 0, 0] {
                ids.push(u64::from_le_bytes(lend[..8].try_into().unwrap()));
            }
        } else {
            let words = [r % 5, (r >> 8) % 64, (r >> 20) % 3, 0];
            let mut lend: Vec<u8> = (0..(r >> 30) % 80).map(|_| next() as u8).collect();
            let _ = t.server.answer_common(&who, &words, &Handles::new(), &mut lend, &mut k);
        }
        if r % 7 == 0 {
            let fid = (r >> 32) as u32 % 4;
            let _ = t.rpc(&who, Body::Tattach { fid, afid: NOFID, uname: "", aname: "" });
        }
    }
}

/// `mint_rooted` (answer 174: `ipd`'s `grant`): a connection rooted where the file server says,
/// minted in the same table as `new_connection`'s, admitted, freed by `disconnect` like any other,
/// and undone by `unmint`.
#[test]
fn a_rooted_mint_is_an_ordinary_connection_rooted_where_the_server_says() {
    let (mut t, mut k) = (T::new(), FakeKernel::new());
    let a = alice();
    t.attach(&a, 0, "");
    // The file server picks the root: `a/b` (node 2), whatever the caller's own root is.
    let root = (2usize, t.server.fs.qid(2));
    let (handle, id, badge) = t.server.mint_rooted(&a, root, &mut k).unwrap();
    assert_eq!(handle, h(99 + k.minted.len() as u32));
    assert!(badge >= FIRST_MINTED_BADGE);
    assert_eq!(t.server.connections(), 1);
    let child = through(&a, badge);
    t.attach(&child, 0, "");
    assert_eq!(t.walk(&child, 0, 1, &["f"]), vec![3], "rooted at a/b");
    assert_eq!(t.walk(&child, 0, 2, &[".."]), vec![2], "`..` stops at its root");
    // Its id is an ordinary connection's: `disconnect` frees it and its fids.
    t.disconnect(&mut k, &a, id).unwrap();
    assert_eq!(t.server.connections(), 0);
    assert_eq!(t.err(&child, Body::Tattach { fid: 5, afid: NOFID, uname: "", aname: "" }), "no such connection");
    // `unmint` undoes one whose reply was not delivered, admission and all.
    let (_, _, badge) = t.server.mint_rooted(&a, root, &mut k).unwrap();
    t.server.unmint(badge);
    assert_eq!(t.server.connections(), 0);
    assert_eq!(t.server.admission().held(AdmitKey::of(&a), Resource::State), 0);
}

/// A rooted mint takes admission like `new_connection`, and a caller at its cap is refused.
#[test]
fn a_rooted_mint_is_admitted() {
    let (mut t, mut k) = (T::new(), FakeKernel::new());
    let a = alice();
    let root = (0usize, t.server.fs.qid(0));
    let mut made = 0;
    while t.server.mint_rooted(&a, root, &mut k).is_ok() {
        made += 1;
        assert!(made <= 8, "the state cap binds");
    }
    assert!(made >= 1);
    assert_eq!(t.server.admission().held(AdmitKey::of(&a), Resource::State), made);
}
