//! `ninep_common` on the 9P skeleton, driven through `answer_common` with a fake minter (no
//! system calls): `new_connection` and `disconnect`, admission and fair shares of minted
//! connections, the file server's grant and disconnect hooks, and hostile requests.

use core::num::NonZeroU64;

use super::*;

/// A kernel for `answer_common`: mints handles 100, 101, ... and remembers the badges and the
/// scope each was minted in; refuses the scope `refuses` as the kernel refuses one not below
/// the server's stamp.
struct FakeKernel {
    minted: Vec<u64>,
    scopes: Vec<Option<Handle>>,
    refuses: Option<Handle>,
    rng: u64,
}

impl FakeKernel {
    fn new() -> FakeKernel {
        FakeKernel { minted: Vec::new(), scopes: Vec::new(), refuses: None, rng: 0x9e37_79b9_7f4a_7c15 }
    }
}

impl Minter for FakeKernel {
    fn mint(&mut self, badge: NonZeroU64, scope: Option<Handle>) -> Result<Handle, Error> {
        if scope.is_some() && scope == self.refuses {
            return Err(Error::NotPermitted);
        }
        self.minted.push(badge.get());
        self.scopes.push(scope);
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
        self.connect_in(k, who, root, quota, &[])
    }

    /// `new_connection` from `who` bringing `handles` (its scope, or what a hostile client
    /// sends): the new connection's badge and id, or the error status. Whatever was brought is
    /// closed either way.
    fn connect_in(
        &mut self,
        k: &mut FakeKernel,
        who: &Caller,
        root: &str,
        quota: u64,
        handles: &[Handle],
    ) -> Result<(u64, u64), u32> {
        let message = ninep_common::Message::NewConnection(ninep_common::NewConnection { root, quota });
        let (outcome, lend) = self.common(k, who, message, handles);
        if outcome.words[0] != 0 {
            assert!(outcome.send.as_slice().is_empty());
            assert_eq!(outcome.close.as_slice(), handles, "a refusal closes what the request brought");
            return Err(outcome.words[0] as u32);
        }
        let sent = outcome.send.as_slice();
        let reply = ninep_common::Reply::decode(2, &outcome.words, &lend, sent.len()).unwrap().unwrap();
        let ninep_common::Reply::NewConnection(reply) = reply else { panic!("{reply:?}") };
        // One handle travels, and the server's own copy is closed once it has, the scope with it.
        assert_eq!(sent, [h(99 + k.minted.len() as u32)]);
        let mut close = handles.to_vec();
        close.extend_from_slice(sent);
        assert_eq!(outcome.close.as_slice(), close);
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

/// The file server is told each connection's id with its badge, the id its requester got, so it
/// can name the connection to others by it (`consoled`'s `[con N]`); the id goes with the badge.
#[test]
fn the_file_server_learns_the_id_the_requester_got() {
    let (mut t, mut k) = (T::new(), FakeKernel::new());
    let a = alice();
    let (b1, id1) = t.connect(&mut k, &a, "", 0).unwrap();
    let (b2, id2) = t.connect(&mut k, &through(&a, b1), "a", 0).unwrap();
    assert_ne!(id1, id2);
    assert_eq!(t.server.fs.ids, [(b1, id1), (b2, id2)]);
    // A refused mint tells the server nothing to keep.
    assert_eq!(t.connect(&mut k, &a, "", REFUSED_QUOTA), Err(REFUSED));
    assert_eq!(t.server.fs.ids, [(b1, id1), (b2, id2)]);
    // Disconnecting the first takes the one minted through it too.
    t.disconnect(&mut k, &a, id1).unwrap();
    assert!(t.server.fs.ids.is_empty());
}

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

/// A mint at the caller's own root is not a read (R25): an unlabelled minter whose root is
/// labelled mints there, learning a handle and an id; the holder's `Tattach` is the first read,
/// refused to an unlabelled holder and allowed to a labelled one; a path into that root is
/// walked, and refused at its first step.
#[test]
fn a_mint_at_its_own_root_reads_nothing() {
    let (mut t, mut k) = (T::new(), FakeKernel::new());
    let vault = caller(ALICE, 1001, &[7]);
    let (inside, _) = t.connect(&mut k, &vault, "vault", 0).unwrap();
    // An unlabelled caller holding a connection rooted in the vault.
    let minter = through(&alice(), inside);
    let (minted, _) = t.connect(&mut k, &minter, "", 0).unwrap();
    let attach = Body::Tattach { fid: 0, afid: NOFID, uname: "", aname: "" };
    assert_eq!(t.err(&through(&alice(), minted), attach), "permission denied");
    let holder = through(&vault, minted);
    t.attach(&holder, 0, "");
    assert_eq!(t.walk(&holder, 0, 1, &["key"]), vec![6]);
    assert_eq!(t.connect(&mut k, &minter, "key", 0), Err(REFUSED));
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

/// A red-team case: a disconnect frees a whole subtree in place, however its connections were
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
    // The attack on R26 (servers/serving.md) on a 9P server: an agent in its sponsor's bucket
    // floods it with fids, through as many connections as it can mint for itself; its sponsor
    // still opens files. The cap is the derived one for the bucket's eight connections at one
    // working fid each beside their roots, 8 + (8 + 1) (servers/serving.md, `admit`).
    let mut t = T::with_limit(17);
    let mut k = FakeKernel::new();
    let steward = caller(9, 0, &[]);
    let (agent_conn, _) = t.connect(&mut k, &steward, "", 0).unwrap();
    let (alice_conn, _) = t.connect(&mut k, &steward, "", 0).unwrap();
    let agent = caller(agent_conn, 1001, &[]);
    let mut mine = vec![agent_conn];
    // Connections the agent mints for itself count in its own share: half the bucket's 8, the
    // connection the steward minted for it among them from its first request (its first
    // `new_connection` here), so three more, not four.
    while let Ok((c, _)) = t.connect(&mut k, &through(&agent, *mine.last().unwrap()), "", 0) {
        mine.push(c);
    }
    assert_eq!(mine.len(), 1 + 3);
    assert_eq!(t.server.admission().held(AdmitKey::of(&agent), Resource::State), 4);
    assert_eq!(t.server.admission().held(AdmitKey::of(&steward), Resource::State), 1, "alice's, pending");
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
    // Each connection's first attach is its root, a fixed cost outside the share, four in all;
    // the first connection took half the working fids while its root was the only one, 8 of 16,
    // and the others' attaches beyond their roots are refused: the agent keeps what it holds,
    // 12 of 17, and takes no more.
    assert_eq!(fid, 12, "the agent's four roots and the eight working fids its first connection took");
    // Alice's root is hers whatever the agent holds, and beside it a third of the twelve working
    // fids the five roots leave: two more attaches and a walk.
    let alice = caller(alice_conn, 1001, &[]);
    for fid in 0..3 {
        t.attach(&alice, fid, "");
    }
    t.walk(&alice, 0, 10, &["notes"]);
    t.open(&alice, 10, mode::OREAD).unwrap();
    assert_eq!(t.read(&alice, 10, 0, 5).unwrap(), b"hello");
}

/// servers/serving.md `admit`: a connection minted for another account is admitted in that
/// account's bucket from its first request; until then it holds one of its minter's pending
/// slots. The steward (account 0, root badge 9) mints eight: while nobody calls, its own share
/// holds all eight and is refused the ninth, the pending pool's bound (R26); each account's first
/// request moves its connection to that account's bucket, so once six accounts have called the
/// steward holds two, and a second connection called on by the first account counts in that
/// account's bucket, beside its first; the last one, never called on, goes back with the
/// steward's disconnect, and its bucket, holding nothing, leaves the table.
#[test]
fn a_connection_minted_for_another_account_is_admitted_there_from_its_first_request() {
    let mut t = T::with_limit(12);
    let mut k = FakeKernel::new();
    let steward = caller(9, 0, &[]);
    let steward_key = AdmitKey::of(&steward);
    let held = |t: &T, key| t.server.admission().held(key, Resource::State);
    let mut minted = Vec::new();
    for _ in 0..8 {
        minted.push(t.connect(&mut k, &steward, "", 0).unwrap());
    }
    assert_eq!(
        t.connect(&mut k, &steward, "", 0),
        Err(REFUSED),
        "the ninth: the minter's share bounds its mints"
    );
    assert_eq!(held(&t, steward_key), 8);
    assert_eq!(t.server.admission().keys(), 1, "all pending, all the steward's");
    for (i, &(badge, _)) in minted[..6].iter().enumerate() {
        let who = caller(badge, 1001 + i as u64, &[]);
        t.attach(&who, 0, "");
        assert_eq!(held(&t, AdmitKey::of(&who)), 1, "account {}'s bucket holds it", who.account);
        assert_eq!(held(&t, steward_key), 7 - i as u32, "and the steward's slot went back");
    }
    assert_eq!(t.server.admission().keys(), 7);
    // The seventh, called on by the first account: its second, in its bucket beside the first.
    let alice_again = caller(minted[6].0, 1001, &[]);
    t.attach(&alice_again, 1, "");
    assert_eq!(held(&t, AdmitKey::of(&alice_again)), 2);
    assert_eq!(held(&t, steward_key), 1, "the eighth, pending");
    // Settled once, a connection stays where it is: another request on it moves nothing.
    t.clunk(&alice_again, 1);
    t.attach(&alice_again, 1, "");
    assert_eq!(held(&t, AdmitKey::of(&alice_again)), 2);
    // Nobody ever calls on the eighth: the minter's disconnect frees its pending slot.
    t.disconnect(&mut k, &steward, minted[7].1).unwrap();
    assert_eq!(held(&t, steward_key), 0);
    assert_eq!(t.server.admission().keys(), 6, "the steward's bucket, holding nothing, is gone");
    // And a settled connection's disconnect gives its account's bucket back, not the steward's.
    t.disconnect(&mut k, &steward, minted[6].1).unwrap();
    assert_eq!(held(&t, AdmitKey::of(&alice_again)), 1);
}

/// A caller whose own bucket has no room is refused its first request, and the connection stays
/// pending in its minter's share for a later request to try again; it is never in nobody's
/// bucket, and the minter's share never keeps what an account has taken over.
#[test]
fn a_first_request_on_a_connection_its_account_cannot_hold_is_refused_and_it_stays_pending() {
    let limits = Limits { buckets: 4, in_flight: 0, files: 12, state: 2, requests: 0, pages: 0 };
    let mut t =
        T { server: NineServer::new(MemFs::new(), limits, TEST_RANDOM).unwrap(), buf: vec![0; MSIZE] };
    let mut k = FakeKernel::new();
    let steward = caller(9, 0, &[]);
    let held = |t: &T, who: &Caller| t.server.admission().held(AdmitKey::of(who), Resource::State);
    let first = t.connect(&mut k, &steward, "", 0).unwrap();
    let second = t.connect(&mut k, &steward, "", 0).unwrap();
    let alice = |badge| caller(badge, 1001, &[]);
    t.attach(&alice(first.0), 0, "");
    t.attach(&alice(second.0), 0, "");
    assert_eq!((held(&t, &alice(0)), held(&t, &steward)), (2, 0), "alice's bucket is at its cap");
    // The steward's share is free again, so it mints a third for her; her bucket cannot take it.
    let third = t.connect(&mut k, &steward, "", 0).unwrap();
    let attach = || Body::Tattach { fid: 0, afid: NOFID, uname: "", aname: "" };
    assert_eq!(t.err(&alice(third.0), attach()), "too many open files");
    assert_eq!((held(&t, &alice(0)), held(&t, &steward)), (2, 1), "still pending, still the steward's");
    assert_eq!(
        t.connect(&mut k, &alice(third.0), "", 0),
        Err(REFUSED),
        "a common request is refused the same"
    );
    // Room made in her bucket, her next request settles it there.
    t.disconnect(&mut k, &steward, first.1).unwrap();
    t.attach(&alice(third.0), 0, "");
    assert_eq!((held(&t, &alice(0)), held(&t, &steward)), (2, 0));
}

/// The attack on R26 (servers/serving.md) from account 0, which is keyed by badge: a system
/// caller mints a chain of connections for itself, each through the last, and floods fids and
/// connections at every link. However deep the chain and whoever holds each link, all of it is
/// charged to the root badge's one bucket, and another system caller is still admitted. What a
/// link gives back, by a failed walk, a failed `new_connection` or a disconnect, goes back to
/// that same bucket: once every link is gone the server holds nothing.
#[test]
fn an_account_0_chain_holds_one_bucket() {
    let mut t = T::with_limit(12);
    let mut k = FakeKernel::new();
    let daemon = caller(9, 0, &[]);
    let mut chain = vec![(daemon.badge, 0)];
    while let Ok(link) = t.connect(&mut k, &through(&daemon, chain.last().unwrap().0), "", 0) {
        chain.push(link);
        // A deep link's failures give back to the root's bucket what they took from it.
        let deep = through(&daemon, link.0);
        assert!(t.connect(&mut k, &deep, "nowhere", 0).is_err());
        t.attach(&deep, 0, "");
        let walk = Body::Twalk { fid: 0, newfid: 1, wnames: Names::new(&["nowhere"]).unwrap() };
        assert_eq!(t.err(&deep, walk), "file does not exist");
        t.clunk(&deep, 0);
    }
    assert_eq!(chain.len(), 1 + 8, "the chain holds one bucket's 8 connections");
    let mut fid = 0;
    let mut fids = Vec::new();
    for &(c, _) in &chain {
        loop {
            let attach = Body::Tattach { fid, afid: NOFID, uname: "", aname: "" };
            if t.rpc(&through(&daemon, c), attach) == (Body::Rerror { ename: "too many open files" }) {
                break;
            }
            fids.push((c, fid));
            fid += 1;
        }
    }
    assert_eq!(fid, 12, "and one bucket's 12 fids");
    assert_eq!(t.server.admission().keys(), 1, "in one bucket");
    // Another system caller, and a link handed to one, are no way round it.
    let other = caller(10, 0, &[]);
    assert_eq!(t.connect(&mut k, &through(&other, chain[3].0), "", 0), Err(REFUSED));
    let (_, other_id) = t.connect(&mut k, &other, "", 0).unwrap();
    t.attach(&other, 0, "");
    assert_eq!(t.server.admission().keys(), 2);
    // Every link disconnected, deepest first, each by the link it was minted through, and the
    // root's own fids clunked: nothing is left held, in either bucket.
    for i in (1..chain.len()).rev() {
        t.disconnect(&mut k, &through(&daemon, chain[i - 1].0), chain[i].1).unwrap();
    }
    for &(c, fid) in fids.iter().filter(|(c, _)| *c == daemon.badge) {
        t.clunk(&through(&daemon, c), fid);
    }
    t.clunk(&other, 0);
    t.disconnect(&mut k, &other, other_id).unwrap();
    let root = t.server.admission();
    assert_eq!(root.held(AdmitKey::of(&daemon), Resource::State), 0);
    assert_eq!(root.held(AdmitKey::of(&daemon), Resource::Files), 0);
    assert_eq!(root.keys(), 0, "no bucket is left behind");
}

/// The same chain minted by the file server (`ipd`'s `grant`): each rooted mint through the last
/// link is charged to the root badge's one bucket, and undoing one gives it back there.
#[test]
fn an_account_0_rooted_chain_holds_one_bucket() {
    let (mut t, mut k) = (T::new(), FakeKernel::new());
    let daemon = caller(9, 0, &[]);
    let root = (0usize, t.server.fs.qid(0));
    let mut last = daemon.badge;
    let mut made = 0;
    while let Ok((_, _, badge)) = t.server.mint_rooted(&through(&daemon, last), root, None, &mut k) {
        (last, made) = (badge, made + 1);
    }
    assert_eq!(made, 8, "the chain holds one bucket's 8 connections");
    assert_eq!(t.server.admission().keys(), 1, "in one bucket");
    t.server.unmint(last);
    assert_eq!(t.server.admission().held(AdmitKey::of(&daemon), Resource::State), 7);
    assert_eq!(t.server.admission().keys(), 1);
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

/// Byte quotas are the file server's (servers/littlefsd.md, "Quotas"); the skeleton hands it every
/// grant and every disconnect.
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

/// `mint_rooted` (`ipd`'s `grant`): a connection rooted where the file server says,
/// minted in the same table as `new_connection`'s, admitted, freed by `disconnect` like any other,
/// and undone by `unmint`.
#[test]
fn a_rooted_mint_is_an_ordinary_connection_rooted_where_the_server_says() {
    let (mut t, mut k) = (T::new(), FakeKernel::new());
    let a = alice();
    t.attach(&a, 0, "");
    // The file server picks the root: `a/b` (node 2), whatever the caller's own root is.
    let root = (2usize, t.server.fs.qid(2));
    let (handle, id, badge) = t.server.mint_rooted(&a, root, None, &mut k).unwrap();
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
    assert_eq!(
        t.err(&child, Body::Tattach { fid: 5, afid: NOFID, uname: "", aname: "" }),
        "no such connection"
    );
    // `unmint` undoes one whose reply was not delivered, admission and all.
    let (_, _, badge) = t.server.mint_rooted(&a, root, None, &mut k).unwrap();
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
    while t.server.mint_rooted(&a, root, None, &mut k).is_ok() {
        made += 1;
        assert!(made <= 8, "the state cap binds");
    }
    assert!(made >= 1);
    assert_eq!(t.server.admission().held(AdmitKey::of(&a), Resource::State), made);
}

/// R41: a connection is minted in the scope its requester sends, the request's one handle
/// slot, which the server passes to the kernel's `mint` as the stamp and closes once minted
/// with; a scope the kernel refuses (not at or below the server's own stamp) is `refused`, with
/// nothing minted and the admission given back; a request bringing more than its one slot does
/// not decode, and is malformed with everything it brought closed.
#[test]
fn a_connection_is_minted_in_the_scope_its_requester_sends() {
    let (mut t, mut k) = (T::new(), FakeKernel::new());
    let a = alice();
    let (scope, other) = (h(7), h(8));
    let (_, id) = t.connect_in(&mut k, &a, "", 0, &[scope]).unwrap();
    assert_eq!(k.scopes, [Some(scope)]);
    t.disconnect(&mut k, &a, id).unwrap();
    // No scope sent: the server's own stamp, as before.
    t.connect(&mut k, &a, "a", 0).unwrap();
    assert_eq!(k.scopes, [Some(scope), None]);
    k.refuses = Some(other);
    let held = t.server.admission().held(AdmitKey::of(&a), Resource::State);
    assert_eq!(t.connect_in(&mut k, &a, "", 0, &[other]), Err(REFUSED));
    assert_eq!(k.minted.len(), 2, "nothing minted");
    assert_eq!(t.server.minted.len(), 1, "and nothing recorded");
    assert_eq!(t.server.admission().held(AdmitKey::of(&a), Resource::State), held, "the admission is back");
    let (mut t, mut k) = (T::new(), FakeKernel::new());
    assert_eq!(t.connect_in(&mut k, &a, "", 0, &[scope, other]), Err(ErrorCode::Malformed.code()));
    assert!(k.minted.is_empty());
}
