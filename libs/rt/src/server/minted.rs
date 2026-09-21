//! Capabilities a server mints for its clients, once for every server (CAPABILITIES.md, "a
//! launcher never passes its own connection to a child"): 9P's `new_connection`/`disconnect`
//! and a typed server's `grant`/`release` are the same table with a different payload.
//!
//! - [`Minted::reserve`] then [`Minted::commit`] mint one: a badge from a counter starting at
//!   [`FIRST_MINTED_BADGE`] that is never reused (answer 86), a random id (never a counter: CONTAINMENT.md),
//!   and a handle minted from the message in hand, so it is stamped like the handle the request came through
//!   and dies with it. Between the two the server has the last word (a file server's quota) before any handle
//!   exists.
//! - [`Minted::disconnect`] frees the capability with `id` and everything minted under it, for the client
//!   that received the id and nobody else: the same answer whether the id is somebody else's or nobody's.
//! - A capability a client mints for itself counts in the share of the one it minted it through
//!   ([`Minted::share`]), so minting more badges buys no bigger share (answer 117).
//!
//! Admission is the server's: it admits one [`super::Resource::State`] before reserving, gives it
//! back if the mint fails, and gives it back for each entry [`Minted::disconnect`] hands it.
//!
//! Badges below [`FIRST_MINTED_BADGE`] are the server's own, given meaning by whoever set the
//! server up; a badge at or above it that is not in the table is no capability at all.

use alloc::vec::Vec;
use core::num::NonZeroU64;

use redoubt_sys::{Error, Handle};

use super::AdmitKey;
use crate::ipc::Caller;

/// Badges below this are the server's own, given meaning by whoever set the server up; a table
/// only ever gives out badges at or above it.
pub const FIRST_MINTED_BADGE: u64 = 1 << 63;

/// How wide the draw for a table's first badge is: a quarter of the minted range, so at least
/// 2^62 badges are left to count through afterwards.
const SPAN: u64 = 1 << 62;

/// Where a table's counter starts, from one word of the kernel's CSPRNG (answer 126): uniform
/// over [`SPAN`] badges above [`FIRST_MINTED_BADGE`], so two incarnations of a server agree on
/// a badge with probability 2^-62 per badge they hand out, and each still has 2^62 to give.
pub const fn first_badge(random: u64) -> u64 { FIRST_MINTED_BADGE | (random % SPAN) }

/// The caller did not receive the id it named: the same answer whether the id is somebody
/// else's or nobody's, so nothing is revealed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotYours;

/// What minting needs from the kernel, apart so that every path runs in a host test with no
/// system call.
pub trait Minter {
    /// A handle to the endpoint the request came in on, with `badge`, stamped like the handle
    /// the request came through (CAPABILITIES.md, minting keeps the stamp).
    fn mint(&mut self, badge: NonZeroU64) -> Result<Handle, Error>;
    /// A random `u64`.
    fn random(&mut self) -> Result<u64, Error>;
}

/// The kernel, answering for the request with this message id (`Request::id`).
pub struct Kernel(pub NonZeroU64);

impl Minter for Kernel {
    fn mint(&mut self, badge: NonZeroU64) -> Result<Handle, Error> {
        Ok(crate::ipc::mint_from_message(self.0, badge)?.handle())
    }

    fn random(&mut self) -> Result<u64, Error> { crate::handle::random_u64() }
}

/// Why a mint failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MintError {
    /// The badge space is spent.
    TooMany,
    /// No memory for the record, no randomness for the id, or the kernel refused the handle.
    Failed,
}

/// Who a capability was minted for: the badge it came through and the client that used it. A
/// second line of defence: only that client may disconnect it, whoever else holds a copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Requester {
    badge: u64,
    client: AdmitKey,
}

impl Requester {
    fn of(caller: &Caller) -> Requester { Requester { badge: caller.badge, client: AdmitKey::of(caller) } }
}

/// One minted capability and what the server attached to it.
pub struct Entry<T> {
    pub badge: u64,
    /// The random id its requester was given; only they may disconnect it.
    id: u64,
    requester: Requester,
    /// The share its admission was taken from.
    requester_share: u64,
    /// The badge it was minted through: it goes when that one goes.
    parent: u64,
    pub value: T,
}

impl<T> Entry<T> {
    /// The client whose admission it holds, and the share it was charged to: what to release.
    pub fn charged_to(&self) -> (AdmitKey, u64) { (self.requester.client, self.requester_share) }
}

/// A badge and id drawn and a record reserved, but no handle yet: [`Minted::commit`] mints it.
/// Dropped, nothing was minted.
#[must_use]
pub struct Ticket {
    badge: NonZeroU64,
    id: u64,
    requester: Requester,
    requester_share: u64,
}

impl Ticket {
    pub fn badge(&self) -> u64 { self.badge.get() }
}

/// The table: in mint order (appended, removed with `remove`), so a capability always sits after
/// the one it was minted through, which [`Minted::disconnect`] relies on.
pub struct Minted<T> {
    entries: Vec<Entry<T>>,
    /// The next badge; only ever goes up, so a badge is never reused (answer 86).
    next_badge: u64,
    /// The badge [`Minted::commit`] made since [`Minted::answering`], so a server whose reply
    /// never reached its caller can undo it.
    minted_here: Option<u64>,
}

impl<T> Minted<T> {
    /// A table whose first badge is drawn from `random`, one word of the kernel's CSPRNG
    /// ([`first_badge`]).
    pub const fn new(random: u64) -> Minted<T> {
        Minted { entries: Vec::new(), next_badge: first_badge(random), minted_here: None }
    }

    /// Starts answering one request: forgets what the last one minted.
    pub fn answering(&mut self) { self.minted_here = None; }

    /// The badge minted while answering this request, if any. A server whose reply did not
    /// reach its caller passes it to [`Minted::forget`]: the requester never learnt the id, so
    /// nothing could ever disconnect it, and its admission slot would be held for the life of
    /// the process.
    pub fn minted_here(&self) -> Option<u64> { self.minted_here }

    /// Capabilities minted and not yet disconnected.
    pub fn len(&self) -> usize { self.entries.len() }

    pub fn is_empty(&self) -> bool { self.entries.is_empty() }

    /// What was attached to the live capability with `badge`, if there is one.
    pub fn get(&self, badge: u64) -> Option<&T> {
        self.entries.iter().find(|e| e.badge == badge).map(|e| &e.value)
    }

    /// The share `caller`'s requests count in: its badge, or, for a capability it minted for
    /// itself, the share of the one it minted it through (answer 117), so minting more badges
    /// buys no bigger share. A capability minted *for another client* is a share of its own,
    /// which is what the steward does for a lease's agent.
    ///
    /// **It cannot tell "for itself" from "for another" within account 0.** `AdmitKey` keys
    /// account 0 by badge (CONTAINMENT.md, because the budget a system caller shares does not
    /// travel), so a system caller minting for itself looks, through the new badge, like a
    /// different client: the fold stops and the chain opens a fresh bucket per link. A server
    /// whose clients can chain must say what stops one of them spending every bucket it has —
    /// `keyd` allows no chain at all (only a root badge may grant); the 9P skeleton cannot take
    /// that rule, because minting a connection for a child is how attenuation works there, so
    /// for it this is an open hole, reported with WP-S1 rather than closed here.
    pub fn share(&self, caller: &Caller) -> u64 {
        let client = AdmitKey::of(caller);
        let mut badge = caller.badge;
        // Each step goes to an older badge, so this ends; the bound is only a backstop.
        for _ in 0..=self.entries.len() {
            match self.entries.iter().find(|e| e.badge == badge) {
                Some(e) if e.requester.client == client => badge = e.parent,
                _ => break,
            }
        }
        badge
    }

    /// Draws the badge and id for a capability minted through `caller`'s, charged to `share`
    /// (which the server admitted first), and reserves its record.
    pub fn reserve(
        &mut self,
        caller: &Caller,
        share: u64,
        kernel: &mut impl Minter,
    ) -> Result<Ticket, MintError> {
        let badge = NonZeroU64::new(self.next_badge)
            .filter(|b| b.get() >= FIRST_MINTED_BADGE)
            .ok_or(MintError::TooMany)?;
        let id = self.fresh_id(kernel)?;
        self.entries.try_reserve(1).map_err(|_| MintError::Failed)?;
        Ok(Ticket { badge, id, requester: Requester::of(caller), requester_share: share })
    }

    /// Mints the handle for `ticket` and records it with `value`: the handle to send, its id and
    /// its badge. Whatever happens, the badge is spent.
    pub fn commit(
        &mut self,
        ticket: Ticket,
        value: T,
        kernel: &mut impl Minter,
    ) -> Result<(Handle, u64, u64), MintError> {
        let Ticket { badge, id, requester, requester_share } = ticket;
        // Never reused, whatever happens to this capability (answer 86).
        self.next_badge = badge.get().wrapping_add(1);
        let handle = kernel.mint(badge).map_err(|_| MintError::Failed)?;
        self.minted_here = Some(badge.get());
        self.entries.push(Entry {
            badge: badge.get(),
            id,
            requester,
            requester_share,
            parent: requester.badge,
            value,
        });
        Ok((handle, id, badge.get()))
    }

    /// A random id no live capability has. `Failed` if the kernel's randomness fails, or every
    /// draw collided, which needs a broken CSPRNG.
    fn fresh_id(&self, kernel: &mut impl Minter) -> Result<u64, MintError> {
        for _ in 0..4 {
            let id = kernel.random().map_err(|_| MintError::Failed)?;
            if id != 0 && !self.entries.iter().any(|e| e.id == id) {
                return Ok(id);
            }
        }
        Err(MintError::Failed)
    }

    /// `disconnect(id)` from `caller`: frees the capability and every one minted under it,
    /// handing each to `gone` as it goes (the server releases its admission there). `Err(())`
    /// if the caller is not the one that received `id`.
    ///
    /// It allocates nothing, so it cannot stop halfway. The table is in mint order and a
    /// capability sits after the one it was minted through, so every descendant comes after the
    /// one named: that one goes first, then one forward pass frees each whose parent was minted
    /// here and is now gone, which by then is exactly its descendants.
    pub fn disconnect(
        &mut self,
        caller: &Caller,
        id: u64,
        gone: impl FnMut(Entry<T>),
    ) -> Result<(), NotYours> {
        let requester = Requester::of(caller);
        let i = self.entries.iter().position(|e| e.id == id && e.requester == requester).ok_or(NotYours)?;
        self.forget_from(i, gone);
        Ok(())
    }

    /// Everything `caller` minted, and everything minted under it: what a client asks for when
    /// it has lost its ids, and what a restarted one asks for before it starts again. Without
    /// it a client that died holding ids pins its share for the life of the server, because
    /// only the holder of an id can name a capability.
    ///
    /// The same forward pass as [`Minted::disconnect`], repeated: each round frees one of the
    /// caller's own and the descendants that went with it.
    pub fn disconnect_all(&mut self, caller: &Caller, mut gone: impl FnMut(Entry<T>)) -> usize {
        let requester = Requester::of(caller);
        let mut freed = 0;
        while let Some(i) = self.entries.iter().position(|e| e.requester == requester) {
            let before = self.entries.len();
            self.forget_from(i, &mut gone);
            freed += before - self.entries.len();
        }
        freed
    }

    /// Frees the live capability with `badge` and everything under it, as
    /// [`Minted::disconnect`] does, without asking who is asking: for a server undoing its own
    /// mint (a reply that never went).
    pub fn forget(&mut self, badge: u64, gone: impl FnMut(Entry<T>)) {
        if let Some(i) = self.entries.iter().position(|e| e.badge == badge) {
            self.forget_from(i, gone);
        }
    }

    fn forget_from(&mut self, mut i: usize, mut gone: impl FnMut(Entry<T>)) {
        // `gone` is taken by value; `disconnect_all` passes `&mut gone`, which is itself `FnMut`.
        gone(self.entries.remove(i));
        while i < self.entries.len() {
            let parent = self.entries[i].parent;
            if parent >= FIRST_MINTED_BADGE && !self.entries[..i].iter().any(|e| e.badge == parent) {
                gone(self.entries.remove(i));
            } else {
                i += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use redoubt_sys::Labels;

    use super::*;

    /// A kernel of a few lines: handles 101, 102, ... and a deterministic id sequence, so a
    /// failure reproduces.
    struct Fake {
        next_handle: u32,
        rng: u64,
        minted: alloc::vec::Vec<u64>,
        fails: bool,
    }

    impl Fake {
        fn new() -> Fake {
            Fake {
                next_handle: 100,
                rng: 0x2545_f491_4f6c_dd1d,
                minted: alloc::vec::Vec::new(),
                fails: false,
            }
        }
    }

    impl Minter for Fake {
        fn mint(&mut self, badge: NonZeroU64) -> Result<Handle, Error> {
            if self.fails {
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

    fn caller(badge: u64, account: u64) -> Caller {
        Caller { badge, account, labels: Labels::from_slice(&[]).unwrap() }
    }

    /// Mints one for `who`, charged to `share`; the (handle, id, badge).
    fn mint(t: &mut Minted<u32>, k: &mut Fake, who: &Caller, share: u64, value: u32) -> (Handle, u64, u64) {
        let ticket = t.reserve(who, share, k).unwrap();
        t.commit(ticket, value, k).unwrap()
    }

    /// Answer 126: the first badge is drawn from the kernel's randomness, above 2^63, with room
    /// left to count through. Two incarnations of a server therefore do not hand the same badge
    /// to their first client, which is what a stale handle from before a restart would name.
    #[test]
    fn the_first_badge_is_random_and_leaves_room() {
        for random in [0, 1, u64::MAX, 0x9e37_79b9_7f4a_7c15, SPAN, SPAN - 1] {
            let first = first_badge(random);
            assert!(first >= FIRST_MINTED_BADGE, "{random:#x}");
            assert!(first - FIRST_MINTED_BADGE < SPAN, "{random:#x}: drawn outside the span");
            assert!(u64::MAX - first >= SPAN, "{random:#x}: too few badges left to count");
        }
        assert_ne!(first_badge(0), first_badge(u64::MAX));
        // Every word of the draw reaches the badge, so the whole span is in play.
        assert_eq!(first_badge(SPAN - 1) - FIRST_MINTED_BADGE, SPAN - 1);
        // Two tables, two draws: the first badges differ, so a capability minted before a
        // restart does not name the first one minted after it.
        let (mut before, mut after) = (Minted::<u32>::new(1), Minted::<u32>::new(2));
        let mut k = Fake::new();
        let who = caller(1, 0);
        let (_, _, old) = mint(&mut before, &mut k, &who, 1, 7);
        let (_, _, new) = mint(&mut after, &mut k, &who, 1, 9);
        assert_ne!(old, new);
        assert_eq!(before.get(new), None, "and the old table never knew the new badge");
    }

    #[test]
    fn badges_are_never_reused_and_ids_are_never_zero() {
        let (mut t, mut k) = (Minted::<u32>::new(0), Fake::new());
        let who = caller(1, 0);
        let mut seen = alloc::vec::Vec::new();
        for value in 0..16 {
            let (_, id, badge) = mint(&mut t, &mut k, &who, 1, value);
            assert!(id != 0 && !seen.contains(&(id, badge)));
            seen.push((id, badge));
        }
        // Freeing them all and minting again never gives a badge back.
        let freed: alloc::vec::Vec<u64> = seen.iter().map(|(_, b)| *b).collect();
        t.disconnect_all(&who, |_| {});
        assert!(t.is_empty());
        for value in 0..4 {
            let (_, _, badge) = mint(&mut t, &mut k, &who, 1, value);
            assert!(!freed.contains(&badge), "a badge came back");
        }
    }

    /// A capability a client minted for itself counts in the share of the one it minted it
    /// through, so minting more badges buys no bigger share (answer 117). One minted *for*
    /// another client is a share of its own.
    #[test]
    fn a_chain_a_client_minted_for_itself_folds_into_its_own_share() {
        let (mut t, mut k) = (Minted::<u32>::new(0), Fake::new());
        let alice = caller(1, 1001);
        let (_, _, child) = mint(&mut t, &mut k, &alice, alice.badge, 0);
        let through = Caller { badge: child, ..alice };
        assert_eq!(t.share(&through), alice.badge, "its own chain folds back");
        let share = t.share(&through);
        let (_, _, grandchild) = mint(&mut t, &mut k, &through, share, 0);
        assert_eq!(t.share(&Caller { badge: grandchild, ..alice }), alice.badge);
        // Handed to somebody else, it is a share of its own (question 117).
        let bob = Caller { badge: child, account: 2002, ..alice };
        assert_eq!(t.share(&bob), child);
    }

    #[test]
    fn disconnect_frees_everything_minted_under_it_and_only_for_its_holder() {
        let (mut t, mut k) = (Minted::<u32>::new(0), Fake::new());
        let alice = caller(1, 1001);
        let (_, id, child) = mint(&mut t, &mut k, &alice, 1, 1);
        let through = Caller { badge: child, ..alice };
        let (_, _, grandchild) = mint(&mut t, &mut k, &through, 1, 2);
        let other = caller(1, 2002);
        let (_, other_id, other_badge) = mint(&mut t, &mut k, &other, 1, 3);
        assert_eq!(t.len(), 3);
        // Not the holder: the same answer whether the id is somebody else's or nobody's.
        for (who, id) in [(&other, id), (&alice, other_id), (&alice, 0), (&alice, id ^ 1)] {
            assert_eq!(t.disconnect(who, id, |_| {}), Err(NotYours));
        }
        assert_eq!(t.len(), 3, "and nothing was freed by trying");
        let mut gone = alloc::vec::Vec::new();
        t.disconnect(&alice, id, |e| gone.push(e.badge)).unwrap();
        assert_eq!(gone, [child, grandchild], "the one named, then what was minted under it");
        assert_eq!(t.len(), 1);
        assert_eq!(t.get(other_badge), Some(&3), "another client's is untouched");
    }

    /// `disconnect_all`: what a client asks for when its ids are gone, which is what a restart
    /// on the same badge leaves it with.
    #[test]
    fn disconnect_all_frees_everything_one_holder_minted() {
        let (mut t, mut k) = (Minted::<u32>::new(0), Fake::new());
        let steward = caller(1, 0);
        let mut mine = alloc::vec::Vec::new();
        for value in 0..5 {
            mine.push(mint(&mut t, &mut k, &steward, 1, value).2);
        }
        // One of them minted a chain of its own, which must go too.
        let through = Caller { badge: mine[0], ..steward };
        let (_, _, deep) = mint(&mut t, &mut k, &through, 1, 9);
        let other = caller(2, 0);
        let (_, _, theirs) = mint(&mut t, &mut k, &other, 2, 42);
        assert_eq!(t.len(), 7);
        let mut freed = alloc::vec::Vec::new();
        assert_eq!(t.disconnect_all(&steward, |e| freed.push(e.badge)), 6);
        freed.sort_unstable();
        let mut want = mine.clone();
        want.push(deep);
        want.sort_unstable();
        assert_eq!(freed, want);
        assert_eq!(t.len(), 1);
        assert_eq!(t.get(theirs), Some(&42), "another badge's are untouched");
        assert_eq!(t.disconnect_all(&steward, |_| {}), 0, "asking again frees nothing");
    }

    /// A reply that never reached its caller: the requester never learnt the id, so only the
    /// server can free it.
    #[test]
    fn what_was_minted_here_can_be_undone() {
        let (mut t, mut k) = (Minted::<u32>::new(0), Fake::new());
        let who = caller(1, 0);
        t.answering();
        assert_eq!(t.minted_here(), None);
        let (_, _, badge) = mint(&mut t, &mut k, &who, 1, 5);
        assert_eq!(t.minted_here(), Some(badge));
        let mut gone = alloc::vec::Vec::new();
        t.forget(badge, |e| gone.push(e.badge));
        assert_eq!(gone, [badge]);
        assert!(t.is_empty());
        // The next request starts clean, and forgetting what is not there does nothing.
        t.answering();
        assert_eq!(t.minted_here(), None);
        t.forget(badge, |_| panic!("nothing to forget"));
    }

    /// A reserved ticket that never commits mints nothing and spends no badge; a commit whose
    /// mint fails spends the badge (it may have reached the kernel) but records nothing.
    #[test]
    fn a_mint_that_fails_records_nothing() {
        let (mut t, mut k) = (Minted::<u32>::new(0), Fake::new());
        let who = caller(1, 0);
        let ticket = t.reserve(&who, 1, &mut k).unwrap();
        let spent = ticket.badge();
        drop(ticket);
        assert!(t.is_empty());
        k.fails = true;
        let ticket = t.reserve(&who, 1, &mut k).unwrap();
        assert_eq!(ticket.badge(), spent, "a dropped ticket's badge is not spent");
        assert_eq!(t.commit(ticket, 1, &mut k), Err(MintError::Failed));
        assert!(t.is_empty());
        k.fails = false;
        let (_, _, badge) = mint(&mut t, &mut k, &who, 1, 1);
        assert!(badge > spent, "the failed commit spent its badge all the same");
    }

    /// The badge space runs out rather than wrapping into the server's own badges.
    #[test]
    fn the_badge_space_runs_out_cleanly() {
        let (mut t, mut k) = (Minted::<u32>::new(0), Fake::new());
        t.next_badge = u64::MAX;
        let who = caller(1, 0);
        assert!(t.reserve(&who, 1, &mut k).is_ok());
        let ticket = t.reserve(&who, 1, &mut k).unwrap();
        t.commit(ticket, 0, &mut k).unwrap();
        assert_eq!(t.next_badge, 0, "the counter wrapped");
        assert_eq!(t.reserve(&who, 1, &mut k).err(), Some(MintError::TooMany), "and stops there");
    }
}
