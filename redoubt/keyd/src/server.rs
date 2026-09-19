//! The `keyd` server: the typed protocol of INIT.md, over `redoubt-rt`'s shared server library.
//!
//! **A typed protocol, not 9P.** `keyd` serves six fixed operations and no namespace. 9P would
//! give it files, and a file that answers `Tread` is the export this server must not have; the
//! skeleton's fids, walks and directory reads would all be machinery for something there is
//! nothing to name. WIRE.md lists `keyd` among the typed protocols, and this is why.
//!
//! **Every request** resolves the caller's badge to one key and one purpose, applies
//! [`check`] to the key's labels, and only then does any work. A badge that names no key, that
//! names a key whose purpose does not allow the operation, or that fails the label check, all
//! get `not_permitted`, which says no more than that.
//!
//! **Admission** ([`Admission`], CONTAINMENT.md) counts the one thing a client can make `keyd`
//! hold: capabilities it was granted. Nothing else here outlives a request — `keyd` parks no
//! calls and keeps no per-caller state — so a flood of signing requests makes `keyd` grow by
//! nothing, and is bounded by the kernel's own fair waiting per (account, label set) (R2) and
//! by each request's bounded work ([`crate::ssh::MAX_PART`], [`MAX_RECORD`]).

use alloc::vec::Vec;
use core::num::NonZeroU64;

use redoubt_rt::abi::{Error, Handle, Handles, ReceivedHandles};
use redoubt_rt::ipc::{Caller, Request, Words};
use redoubt_rt::server::typed::{Answer, Outcome, Protocol, TypedServer, answer, finish};
use redoubt_rt::server::{Access, Admission, AdmitKey, Cost, Limits, Resource, Unsized, check};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::keyd::{
    ErrorCode, Grant, GrantReply, Holds, HoldsReply, Message, PublicKeyReply, Release, ReleaseReply, Reply,
    SignRecord, SignRecordReply, SignSshExchange, SignSshExchangeReply,
};

use crate::keys::{ALGORITHM, Key, Keys, Purpose, SIGNATURE_LEN};
use crate::ssh::{self, Transcript};

/// The domain string an audit record is hashed under, so a signature made for one purpose
/// cannot be presented as one made for another.
///
/// The domain alone is not enough, and that is why [`audit_digest`] exists. A prefix in front
/// of caller bytes only separates protocols whose own messages cannot start with it, and a
/// signature container that covers raw bytes — the boot bundle's is `signature || tar`, with no
/// domain of its own (VERIFIED-BOOT.md) — has no such guarantee: 17 bytes of domain and 8 of
/// length sit inside a `ustar` header's 100-byte name field, and the attacker picks the rest.
/// So `keyd` does not sign `domain || record` at all: it signs the SHA-256 of it, 32 bytes,
/// which no container whose messages are longer can ever be.
pub const AUDIT_DOMAIN: &[u8] = b"redoubt.audit.v1\0";

/// The most an audit record may be: enough for a record the steward writes, small enough that
/// one request's work is bounded by a number stated here.
pub const MAX_RECORD: usize = 8 * 1024;

/// The first badge [`KeyServer`] grants; below it are the manifest's root badges (INIT.md).
/// The same boundary the 9P skeleton uses for minted connections, for the same reason: a root
/// badge and a granted one can never be confused.
pub const FIRST_GRANTED_BADGE: u64 = 1 << 63;

/// What a client may hold in `keyd` at once, per (account, label set) (CONTAINMENT.md).
///
/// - `buckets`: the (account, label set)s `keyd` serves at once — `sshd` and the steward (account 0, one
///   bucket each by badge), and a bucket per logged-in principal and per labelled session of one. Sized for
///   more than milestone 1 has, so the cap does not bind in normal use (answer 118).
/// - `in_flight` is 0: no call is ever parked here; every request is answered as it is taken.
/// - `files` is 0: `keyd` has no files.
/// - `state`: capabilities `grant` has made and `release` has not freed.
pub const LIMITS: Limits = Limits { buckets: 16, in_flight: 0, files: 0, state: 8 };

/// What one of those costs `keyd`, in bytes: a granted capability is its record here and a
/// badged handle in the kernel, rounded up generously.
pub const COST: Cost = Cost { in_flight: 0, file: 0, state: 512 };

/// The bytes of `keyd`'s budget its clients may use between them; its manifest entry gives it
/// the budget, and [`KeyServer::new`] refuses limits that would not fit
/// (`LIMITS.fits(&COST, BUDGET)`).
pub const BUDGET: u64 = 256 * 1024;

/// What the server needs from the kernel, apart so that every path can be driven in a host test
/// with no system call.
pub trait Kernel {
    /// A handle to the endpoint the request came in on, with `badge`, **stamped like the handle
    /// the request came through** (CAPABILITIES.md, minting keeps the stamp), so a granted
    /// capability dies when what the caller holds dies.
    fn mint(&mut self, badge: NonZeroU64) -> Result<Handle, Error>;
    /// A random `u64`, for a grant's id: unpredictable, never a counter (CONTAINMENT.md).
    fn random(&mut self) -> Result<u64, Error>;
}

/// The kernel, answering the call `grant` came in on.
pub struct FromCall<'a>(pub &'a Request);

impl Kernel for FromCall<'_> {
    fn mint(&mut self, badge: NonZeroU64) -> Result<Handle, Error> {
        self.0.mint(badge, None).map(|endpoint| endpoint.handle())
    }

    fn random(&mut self) -> Result<u64, Error> { redoubt_rt::handle::random_u64() }
}

/// The kernel on the path that reads the caller's lend, where nothing may hold the request, so
/// minting is impossible here. Only `grant` mints, and [`KeyServer::serve`] answers it on the
/// other path: the structure, not a check, is what keeps the two apart.
pub struct NoMint;

impl Kernel for NoMint {
    fn mint(&mut self, _badge: NonZeroU64) -> Result<Handle, Error> { Err(Error::NotPermitted) }

    fn random(&mut self) -> Result<u64, Error> { redoubt_rt::handle::random_u64() }
}

/// Who a capability was granted to: the badge it came through and the client that used it. A
/// second line of defence, as the 9P skeleton's connection key is: only that client may release
/// it, whoever else holds a copy of the handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Holder {
    badge: u64,
    client: AdmitKey,
}

impl Holder {
    fn of(caller: &Caller) -> Holder { Holder { badge: caller.badge, client: AdmitKey::of(caller) } }
}

/// One capability `grant` made.
struct Granted {
    badge: u64,
    /// The random id the requester was given; only it may `release` this.
    id: u64,
    /// The key (and so the purpose) it names: always the granter's own.
    key: usize,
    requester: Holder,
    /// The share its [`Resource::State`] charge came from.
    requester_share: u64,
    /// The badge it was granted through: it goes when that one goes.
    parent: u64,
}

/// `keyd`: its keys, the capabilities it has granted, and its admission.
pub struct KeyServer {
    keys: Keys,
    /// In grant order: appended, and removed with `remove`, so a capability always sits after
    /// the one it was granted through ([`KeyServer::release`] relies on that).
    granted: Vec<Granted>,
    /// The next badge `grant` mints; only ever goes up, so a badge is never reused (answer 86).
    next_badge: u64,
    admission: Admission,
    /// Where a reply's bytes are held while the reply borrows them: the request is decoded from
    /// the caller's lend, which the reply is then written over.
    signature: [u8; SIGNATURE_LEN],
    public: [u8; crate::keys::PUBLIC_KEY_LEN],
    /// The badge `grant` made while answering the call in hand, so that a reply that never
    /// reaches its caller can be undone: the requester never learns the id, so nothing could
    /// ever `release` it.
    granted_here: Option<u64>,
}

impl KeyServer {
    /// Serves `keys` under `limits`. Refuses limits that do not leave the open-call headroom or
    /// that cannot seat a fair share ([`Admission::new`]), and limits whose caps at their
    /// ceiling would not fit `budget` bytes (answer 85).
    pub fn new(keys: Keys, limits: Limits, cost: &Cost, budget: u64) -> Result<KeyServer, Unsized> {
        if !limits.fits(cost, budget) {
            return Err(Unsized);
        }
        Ok(KeyServer {
            keys,
            granted: Vec::new(),
            next_badge: FIRST_GRANTED_BADGE,
            admission: Admission::new(limits)?,
            signature: [0; SIGNATURE_LEN],
            public: [0; crate::keys::PUBLIC_KEY_LEN],
            granted_here: None,
        })
    }

    pub fn keys(&self) -> &Keys { &self.keys }

    /// Capabilities granted and not yet released.
    pub fn granted(&self) -> usize { self.granted.len() }

    pub fn admission(&self) -> &Admission { &self.admission }

    /// Answers one call and replies to it.
    pub fn serve(&mut self, mut request: Request) -> Result<(), Error> {
        let (caller, words, handles) = (request.caller, request.words, request.handles);
        self.granted_here = None;
        // Whether the caller lent anything, read here where the borrow ends at once: the
        // `grant` path below holds the request itself, and so cannot look at the lend.
        let lent = !request.lend().is_empty();
        let outcome = if matches!(redoubt_rt::wire::typed::opcode(&words), Ok(GRANT_OPCODE)) && !lent {
            // `grant` mints, and a minted handle keeps the caller's stamp only when it is
            // minted from this very call (CAPABILITIES.md), so the kernel here borrows the
            // request. `grant` has no fields and an inline reply, so it never needs the lend,
            // which would borrow the request too. A `grant` that did arrive with a lend is not
            // an inline message, so it goes the other way and the codec refuses it, like an
            // inline message with a buffer on any other opcode.
            let mut kernel = FromCall(&request);
            answer_with(self, &caller, &words, &handles, &mut [], &mut kernel)
        } else {
            let mut kernel = NoMint;
            answer_with(self, &caller, &words, &handles, request.lend(), &mut kernel)
        };
        let sent = finish(request, &outcome);
        // A reply that could not be sent (the caller died, or the kernel refused it) leaves a
        // granted capability nobody can ever name: its id went nowhere, and `release` answers
        // only the holder of an id. Undo it, so a client cannot fill its own bucket by dying
        // mid-grant.
        if let Some(badge) = self.granted_here.take() {
            if sent.is_err() {
                self.forget_badge(badge);
            }
        }
        sent
    }

    /// Frees the granted capability with `badge`, and every capability granted under it.
    fn forget_badge(&mut self, badge: u64) {
        let found = self.granted.iter().find(|g| g.badge == badge).map(|g| (g.requester, g.id));
        if let Some((requester, id)) = found {
            let _ = self.release_as(requester, id);
        }
    }

    /// The key and purpose the caller's badge names, or `not_permitted`: a badge that names
    /// nothing, one granted and since released, and one from before a restart are all the same
    /// answer, which tells a caller only that this badge is not good here.
    fn resolve(&self, caller: &Caller) -> Result<usize, ErrorCode> {
        let key = if caller.badge >= FIRST_GRANTED_BADGE {
            self.granted.iter().find(|g| g.badge == caller.badge).map(|g| g.key)
        } else {
            self.keys.by_root_badge(caller.badge)
        };
        key.ok_or(ErrorCode::NotPermitted)
    }

    /// The share this caller's requests count in: its own badge, or, for a capability it
    /// granted itself, the share of the one it granted it through, so granting more badges
    /// cannot escape a fair share (answer 117, as the 9P skeleton does it).
    fn share(&self, caller: &Caller) -> u64 {
        let client = AdmitKey::of(caller);
        let mut badge = caller.badge;
        // Each step goes to an older badge, so this ends; the bound is only a backstop.
        for _ in 0..=self.granted.len() {
            match self.granted.iter().find(|g| g.badge == badge) {
                Some(g) if g.requester.client == client => badge = g.parent,
                _ => break,
            }
        }
        badge
    }

    /// Answers one decoded request.
    fn dispatch<'s>(
        &'s mut self,
        caller: &Caller,
        request: Message<'_>,
        kernel: &mut impl Kernel,
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        let index = self.resolve(caller)?;
        // The label check on every request (CONTAINMENT.md). Signing and granting put the
        // caller's data into something the key vouches for, or make new state, so they are
        // writes and need equal labels; reading a public key is a read.
        let access = match request {
            Message::PublicKey(_) | Message::Holds(_) => Access::Read,
            Message::SignSshExchange(_)
            | Message::SignRecord(_)
            | Message::Grant(_)
            | Message::Release(_) => Access::Write,
        };
        check(caller.labels.as_slice(), self.keys.labels(index), access)
            .map_err(|_| ErrorCode::NotPermitted)?;
        match request {
            Message::SignSshExchange(m) => self.sign_ssh_exchange(index, &m),
            Message::SignRecord(m) => self.sign_record(index, &m),
            Message::PublicKey(_) => {
                let public = *self.key(index).public();
                self.public = public;
                Ok(Answer::new(Reply::PublicKey(PublicKeyReply { algorithm: ALGORITHM, key: &self.public })))
            }
            Message::Holds(Holds { algorithm, key }) => {
                // It answers about every key, not only the badge's, because that is the
                // question `sshd` has: is this login key one of `keyd`'s? The answer is about a
                // public key the asker already holds, and public keys are published, so it
                // tells nobody anything they could not learn by connecting. When keys carry
                // labels (milestone 2) this needs a `check` per key, not the badge's alone.
                let held = u32::from(self.keys.holds(algorithm, key));
                Ok(Answer::new(Reply::Holds(HoldsReply { held })))
            }
            Message::Grant(Grant {}) => self.grant(caller, index, kernel),
            Message::Release(Release { id }) => {
                self.release(caller, id)?;
                Ok(Answer::new(Reply::Release(ReleaseReply {})))
            }
        }
    }

    /// The key at `index`, which [`KeyServer::resolve`] found, so it is there.
    fn key(&self, index: usize) -> &Key {
        self.keys.get(index).expect("resolve only returns keys that are here")
    }

    /// The SSH host key's one operation: `keyd` computes the exchange hash from the transcript
    /// and its own public key, and signs that. See [`crate::ssh`].
    fn sign_ssh_exchange<'s>(
        &'s mut self,
        index: usize,
        m: &SignSshExchange<'_>,
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        let key = self.key(index);
        if key.purpose() != Purpose::SshHost {
            return Err(ErrorCode::NotPermitted);
        }
        let transcript =
            Transcript { v_c: m.v_c, v_s: m.v_s, i_c: m.i_c, i_s: m.i_s, q_c: m.q_c, q_s: m.q_s, k: m.k };
        let hash = ssh::exchange_hash(&transcript, key.public()).map_err(|_| ErrorCode::TooMany)?;
        let signature = key.sign(&hash);
        self.signature = signature;
        Ok(Answer::new(Reply::SignSshExchange(SignSshExchangeReply { signature: &self.signature })))
    }

    /// The audit key's one operation: a signature over [`audit_digest`] of the record.
    fn sign_record<'s>(
        &'s mut self,
        index: usize,
        m: &SignRecord<'_>,
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        if self.key(index).purpose() != Purpose::Audit {
            return Err(ErrorCode::NotPermitted);
        }
        if m.record.len() > MAX_RECORD {
            return Err(ErrorCode::TooMany);
        }
        let signature = self.key(index).sign(&audit_digest(m.record));
        self.signature = signature;
        Ok(Answer::new(Reply::SignRecord(SignRecordReply { signature: &self.signature })))
    }

    /// `grant`: a fresh capability with the caller's own key and purpose, stamped like the
    /// handle the request came through, so a launcher never passes its own on (INIT.md).
    fn grant<'s>(
        &'s mut self,
        caller: &Caller,
        index: usize,
        kernel: &mut impl Kernel,
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        // Admission first, so a client at its cap makes the server do no work for it.
        let (requester, share) = (Holder::of(caller), self.share(caller));
        self.admission.admit(requester.client, share, Resource::State).map_err(|_| ErrorCode::TooMany)?;
        match self.make_grant(caller, index, share, kernel) {
            Ok((id, handle)) => {
                let mut handles = Handles::new();
                // Cannot fail: one handle, and a reply may carry `MAX_MSG_HANDLES`.
                let _ = handles.push(handle);
                // The handle was minted for the caller: `keyd` closes its own copy once the
                // reply has copied it, or its table grows by one per grant.
                Ok(Answer { reply: Reply::Grant(GrantReply { id }), handles, close_after_reply: true })
            }
            Err(code) => {
                self.admission.release(requester.client, share, Resource::State);
                Err(code)
            }
        }
    }

    /// Mints the capability itself: the new id and the handle, borrowing nothing from `self`,
    /// so the caller can still give the admission back if this fails.
    fn make_grant(
        &mut self,
        caller: &Caller,
        index: usize,
        requester_share: u64,
        kernel: &mut impl Kernel,
    ) -> Result<(u64, Handle), ErrorCode> {
        let badge = NonZeroU64::new(self.next_badge)
            .filter(|b| b.get() >= FIRST_GRANTED_BADGE)
            .ok_or(ErrorCode::TooMany)?;
        let id = self.fresh_id(kernel)?;
        self.granted.try_reserve(1).map_err(|_| ErrorCode::Failed)?;
        let handle = kernel.mint(badge).map_err(|_| ErrorCode::Failed)?;
        // Never reused, whatever happens to this capability (answer 86).
        self.next_badge = badge.get().wrapping_add(1);
        self.granted.push(Granted {
            badge: badge.get(),
            id,
            key: index,
            requester: Holder::of(caller),
            requester_share,
            parent: caller.badge,
        });
        self.granted_here = Some(badge.get());
        Ok((id, handle))
    }

    /// A random id no live capability has (CONTAINMENT.md: never a counter, which would tell
    /// every principal how many the others made).
    fn fresh_id(&self, kernel: &mut impl Kernel) -> Result<u64, ErrorCode> {
        for _ in 0..4 {
            let id = kernel.random().map_err(|_| ErrorCode::Failed)?;
            if id != 0 && !self.granted.iter().any(|g| g.id == id) {
                return Ok(id);
            }
        }
        Err(ErrorCode::Failed)
    }

    /// `release(id)`: frees the capability and every capability granted under it. `Err` if the
    /// caller is not the one that received `id` — the same answer whether the id belongs to
    /// somebody else or to nobody, so nothing is revealed.
    ///
    /// It allocates nothing, so it cannot stop halfway. `granted` is in grant order and a
    /// capability is granted after the one it came through, so every descendant sits after the
    /// one named: that one goes first, then one forward pass frees each whose parent was
    /// granted here and is now gone, which by then is exactly its descendants.
    fn release(&mut self, caller: &Caller, id: u64) -> Result<(), ErrorCode> {
        self.release_as(Holder::of(caller), id)
    }

    /// [`KeyServer::release`], with the holder already worked out.
    fn release_as(&mut self, requester: Holder, id: u64) -> Result<(), ErrorCode> {
        let mut i = self
            .granted
            .iter()
            .position(|g| g.id == id && g.requester == requester)
            .ok_or(ErrorCode::NotPermitted)?;
        self.forget_at(i);
        while i < self.granted.len() {
            let parent = self.granted[i].parent;
            if parent >= FIRST_GRANTED_BADGE && !self.granted[..i].iter().any(|g| g.badge == parent) {
                self.forget_at(i);
            } else {
                i += 1;
            }
        }
        Ok(())
    }

    /// Frees the capability at `index` and gives its admission back. Not its children. Keeps
    /// `granted` in grant order.
    fn forget_at(&mut self, index: usize) {
        let gone = self.granted.remove(index);
        self.admission.release(gone.requester.client, gone.requester_share, Resource::State);
    }
}

/// What an audit record is signed as: `SHA-256(AUDIT_DOMAIN || length || record)`, the length
/// being a little-endian `u64`, so a record cannot be extended or split without changing it.
/// Whoever checks an audit signature computes this the same way.
///
/// Hashing it rather than signing it directly is what keeps **every** signature `keyd` makes
/// exactly 32 bytes long, and always over a digest `keyd` computed itself: this one, or the SSH
/// exchange hash. A signature container that covers longer messages — the boot bundle's tar, a
/// package, an SSH user-authentication request — therefore cannot be what a `keyd` signature
/// covers, whatever the caller put in the record. See [`AUDIT_DOMAIN`] for why a prefix alone
/// does not give that.
pub fn audit_digest(record: &[u8]) -> [u8; crate::sha256::DIGEST] {
    let mut hash = crate::sha256::Sha256::new();
    hash.update(AUDIT_DOMAIN);
    hash.update(&(record.len() as u64).to_le_bytes());
    hash.update(record);
    hash.finish()
}

/// The opcode of `grant`, which [`KeyServer::serve`] answers apart.
const GRANT_OPCODE: u32 = 5;

/// The protocol, for `redoubt-rt`'s typed dispatch.
pub struct Keyd;

impl Protocol for Keyd {
    type Error = ErrorCode;
    type Reply<'a> = Reply<'a>;
    type Request<'a> = Message<'a>;

    fn decode<'a>(words: &Words, buf: &'a [u8], handles: usize) -> Result<Message<'a>, WireError> {
        Message::decode(words, buf, handles)
    }

    fn encode_reply(reply: &Reply<'_>, buf: &mut [u8]) -> Result<Words, WireError> { reply.encode(buf) }

    fn error_words(error: ErrorCode) -> Words { error.encode() }
}

/// The server and the kernel together, which is what the dispatch trait needs: `handle` may
/// borrow only from `self`, and minting needs the kernel.
struct Serving<'a, 'k, K: Kernel> {
    server: &'a mut KeyServer,
    kernel: &'k mut K,
}

impl<K: Kernel> TypedServer<Keyd> for Serving<'_, '_, K> {
    fn handle<'s>(
        &'s mut self,
        caller: &Caller,
        request: Message<'_>,
        handles: &[Handle],
    ) -> Result<Answer<Reply<'s>>, ErrorCode> {
        // No message of this protocol carries a handle, and the codec refuses a request whose
        // handle count is not its layout's, so a request that brought one never reaches here:
        // it is malformed, and the dispatch closes what it brought, so a client cannot grow
        // `keyd`'s handle table (CONTAINMENT.md, the shared server library).
        debug_assert!(handles.is_empty(), "the codec refuses handles this protocol does not name");
        let _ = handles;
        self.server.dispatch(caller, request, self.kernel)
    }
}

/// Answers one request without making any system call but the ones `kernel` makes: the entry
/// host tests drive, and what [`KeyServer::serve`] calls.
pub fn answer_with(
    server: &mut KeyServer,
    caller: &Caller,
    words: &Words,
    handles: &ReceivedHandles,
    buf: &mut [u8],
    kernel: &mut impl Kernel,
) -> Outcome {
    answer::<Keyd, _>(&mut Serving { server, kernel }, caller, words, handles, buf)
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
