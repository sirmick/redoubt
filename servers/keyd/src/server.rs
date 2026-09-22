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
//! hold: capabilities it was granted, which are the shared library's [`Minted`] table, the same
//! one 9P's `new_connection` and `disconnect` keep. Nothing else here outlives a request —
//! `keyd` parks no calls and keeps no per-caller state — so a flood of signing requests makes
//! `keyd` grow by nothing, and is bounded by the kernel's own fair waiting per (account, label
//! set) (R2) and by each request's bounded work ([`crate::ssh::MAX_PART`], [`MAX_RECORD`]).

use redoubt_rt::abi::{Error, Handle, Handles, ReceivedHandles};
use redoubt_rt::ipc::{Caller, Request, Words};
pub use redoubt_rt::server::minted::FIRST_MINTED_BADGE as FIRST_GRANTED_BADGE;
use redoubt_rt::server::minted::{Entry, Kernel, MintError, Minted, Minter};
use redoubt_rt::server::typed::{Answer, Outcome, Protocol, TypedServer, answer, finish};
use redoubt_rt::server::{Access, Admission, AdmitKey, Cost, Limits, Resource, Unsized, check};
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::proto::keyd::{
    ErrorCode, Grant, GrantReply, Holds, HoldsReply, Message, PublicKeyReply, Release, ReleaseReply, Reply,
    SignRecord, SignRecordReply, SignSshExchange, SignSshExchangeReply,
};

use crate::keys::{Key, Keys, Purpose, SIGNATURE_LEN};
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

/// The id `release` takes to mean "everything I granted". [`Minted`] never issues it, so it
/// cannot collide with a real one.
pub const ALL_GRANTS: u64 = 0;

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

/// `keyd`: its keys, the capabilities it has granted, and its admission.
pub struct KeyServer {
    keys: Keys,
    /// Each granted capability carries the index of the key (and so the purpose) it names:
    /// always the granter's own.
    granted: Minted<usize>,
    admission: Admission,
    /// Where a reply's signature is held while the reply borrows it: the request is decoded
    /// from the caller's lend, which the reply is then written over.
    signature: [u8; SIGNATURE_LEN],
}

impl KeyServer {
    /// Serves `keys` under `limits`. Refuses limits that do not leave the open-call headroom or
    /// that cannot seat a fair share ([`Admission::new`]), and limits whose caps at their
    /// ceiling would not fit `budget` bytes (answer 85).
    /// `random` is one word of the kernel's CSPRNG, which is where the granted badges start
    /// (answer 126): a `keyd` that cannot get one does not start, because a predictable first
    /// badge is a hole across a restart (see [`redoubt_rt::server::minted`]).
    pub fn new(
        keys: Keys,
        limits: Limits,
        cost: &Cost,
        budget: u64,
        random: u64,
    ) -> Result<KeyServer, Unsized> {
        if !limits.fits(cost, budget) {
            return Err(Unsized);
        }
        Ok(KeyServer {
            keys,
            granted: Minted::new(random),
            admission: Admission::new(limits)?,
            signature: [0; SIGNATURE_LEN],
        })
    }

    pub fn keys(&self) -> &Keys { &self.keys }

    /// Capabilities granted and not yet released.
    pub fn granted(&self) -> usize { self.granted.len() }

    pub fn admission(&self) -> &Admission { &self.admission }

    /// Answers one call and replies to it.
    pub fn serve(&mut self, mut request: Request) -> Result<(), Error> {
        let (caller, words, handles) = (request.caller, request.words, request.handles);
        self.granted.answering();
        // Minting from the message id keeps the caller's stamp (CAPABILITIES.md) and borrows
        // nothing, so the lend is read on the same path as everything else.
        let mut kernel = Kernel(request.id());
        let outcome = answer_with(self, &caller, &words, &handles, request.lend(), &mut kernel);
        let sent = finish(request, &outcome);
        // grant requires slot 0's capability. Discard or failure to install that handle rolls
        // back provisional state and its admission charge, even when reply itself succeeded.
        if let Some(badge) = self.granted.minted_here() {
            if !sent.as_ref().is_ok_and(|outcome| outcome.accepted(1)) {
                self.forget_badge(badge);
            }
        }
        sent.map(|_| ())
    }

    /// Frees the granted capability with `badge`, and every capability granted under it.
    fn forget_badge(&mut self, badge: u64) {
        let admission = &mut self.admission;
        self.granted.forget(badge, |gone| {
            let (client, share) = gone.charged_to();
            admission.release(client, share, Resource::State);
        });
    }

    /// The key and purpose the caller's badge names, or `not_permitted`: a badge that names
    /// nothing, one granted and since released, and one from before a restart are all the same
    /// answer, which tells a caller only that this badge is not good here.
    fn resolve(&self, caller: &Caller) -> Result<usize, ErrorCode> {
        let key = if caller.badge >= FIRST_GRANTED_BADGE {
            self.granted.get(caller.badge).copied()
        } else {
            self.keys.by_root_badge(caller.badge)
        };
        key.ok_or(ErrorCode::NotPermitted)
    }

    /// Answers one decoded request.
    fn dispatch<'s>(
        &'s mut self,
        caller: &Caller,
        request: Message<'_>,
        kernel: &mut impl Minter,
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
                let key = self.key(index).public();
                Ok(Answer::new(Reply::PublicKey(PublicKeyReply { key })))
            }
            Message::Holds(Holds { key }) => {
                // It answers about every key, not only the badge's, because that is the
                // question `sshd` has: is this login key one of `keyd`'s? The answer is about a
                // public key the asker already holds, and public keys are published, so it
                // tells nobody anything they could not learn by connecting. When keys carry
                // labels (milestone 2) this needs a `check` per key, not the badge's alone.
                let held = u32::from(self.keys.holds(key));
                Ok(Answer::new(Reply::Holds(HoldsReply { held })))
            }
            // `grant`: a fresh capability with the caller's own key and purpose, stamped like
            // the handle the request came through, so a launcher never passes its own on
            // (INIT.md). Nothing granted is ever wider than the badge it came through.
            Message::Grant(Grant {}) => {
                // **Only a root badge may grant.** A granted capability cannot grant again,
                // so grants never chain. Without that rule a system-class caller escapes its
                // cap: `admit` keys account 0 by badge (CONTAINMENT.md, because the budget id
                // a system caller shares does not travel), and `share` folds a capability into
                // its parent's only when the requester and the caller are the same client,
                // which two badges of account 0 never are. One daemon could then open a fresh
                // bucket per chained grant until `LIMITS.buckets` were spent and nobody, the
                // steward included, could grant at all. Milestone 1 needs no chain: only the
                // steward and `sshd` hold `keyd` capabilities, both through root badges, and
                // no session or lease holds `keys` at all (answer 124).
                if caller.badge >= FIRST_GRANTED_BADGE {
                    return Err(ErrorCode::NotPermitted);
                }
                // Admission first, so a client at its cap makes the server do no work for it.
                let (client, share) = (AdmitKey::of(caller), self.granted.share(caller));
                self.admission.admit(client, share, Resource::State).map_err(|_| ErrorCode::TooMany)?;
                let made = self
                    .granted
                    .reserve(caller, share, kernel)
                    .and_then(|ticket| self.granted.commit(ticket, index, kernel));
                let (handle, id, _badge) = made.map_err(|e| {
                    self.admission.release(client, share, Resource::State);
                    match e {
                        MintError::TooMany => ErrorCode::TooMany,
                        MintError::Failed => ErrorCode::Failed,
                    }
                })?;
                let mut handles = Handles::new();
                // Cannot fail: one handle, and a reply may carry `MAX_MSG_HANDLES`.
                let _ = handles.push(handle);
                // The handle was minted for the caller: `keyd` closes its own copy once the
                // reply has copied it, or its table grows by one per grant.
                Ok(Answer { reply: Reply::Grant(GrantReply { id }), handles, close_after_reply: true })
            }
            // `release(id)`: frees the capability and every capability granted under it, for
            // the caller that received `id` and nobody else; the same answer whether the id
            // belongs to somebody else or to nobody, so nothing is revealed.
            //
            // **`release(0)` frees everything this caller granted.** No grant is ever given the
            // id 0, so it cannot name one. It is what a holder asks for when its ids are gone:
            // a server that crashed and was restarted on the same root badge (INIT.md decision
            // 4) comes back knowing nothing, and without this its share would stay full for the
            // life of `keyd`, because only the holder of an id can name a capability.
            Message::Release(Release { id }) => {
                let admission = &mut self.admission;
                let mut freed = |gone: Entry<usize>| {
                    let (client, share) = gone.charged_to();
                    admission.release(client, share, Resource::State);
                };
                if id == ALL_GRANTS {
                    self.granted.disconnect_all(caller, freed);
                } else {
                    self.granted.disconnect(caller, id, &mut freed).map_err(|_| ErrorCode::NotPermitted)?;
                }
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
        // A part over the work bound is `too_many`; a transcript no key exchange could have
        // produced is a bad length, which is `malformed` in every protocol (WIRE.md).
        let hash = ssh::exchange_hash(&transcript, key.public()).map_err(|e| match e {
            ssh::BadTranscript::TooLong => ErrorCode::TooMany,
            ssh::BadTranscript::BadShape => ErrorCode::Malformed,
        })?;
        self.signature = key.sign(&hash);
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
        self.signature = self.key(index).sign(&audit_digest(m.record));
        Ok(Answer::new(Reply::SignRecord(SignRecordReply { signature: &self.signature })))
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
struct Serving<'a, 'k, K: Minter> {
    server: &'a mut KeyServer,
    kernel: &'k mut K,
}

impl<K: Minter> TypedServer<Keyd> for Serving<'_, '_, K> {
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
    kernel: &mut impl Minter,
) -> Outcome {
    answer::<Keyd, _>(&mut Serving { server, kernel }, caller, words, handles, buf)
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
