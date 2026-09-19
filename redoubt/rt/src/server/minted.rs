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

/// The first badge a table gives out; badges below it are the server's own.
pub const FIRST_MINTED_BADGE: u64 = 1 << 63;

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
}

impl<T> Default for Minted<T> {
    fn default() -> Minted<T> { Minted::new() }
}

impl<T> Minted<T> {
    pub const fn new() -> Minted<T> { Minted { entries: Vec::new(), next_badge: FIRST_MINTED_BADGE } }

    /// Capabilities minted and not yet disconnected.
    pub fn len(&self) -> usize { self.entries.len() }

    pub fn is_empty(&self) -> bool { self.entries.is_empty() }

    /// What was attached to the live capability with `badge`, if there is one.
    pub fn get(&self, badge: u64) -> Option<&T> {
        self.entries.iter().find(|e| e.badge == badge).map(|e| &e.value)
    }

    /// The share `caller`'s requests count in: its badge, or, for a capability it minted for
    /// itself, the share of the one it minted it through.
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
    pub fn disconnect(&mut self, caller: &Caller, id: u64, gone: impl FnMut(Entry<T>)) -> Result<(), ()> {
        let requester = Requester::of(caller);
        let i = self.entries.iter().position(|e| e.id == id && e.requester == requester).ok_or(())?;
        self.forget_from(i, gone);
        Ok(())
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
