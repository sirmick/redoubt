//! The byte quotas per attach root (servers/littlefsd.md, "Quotas"; R48): what each live root holds
//! and keeps in reserve, in bytes. Nothing here reads the medium: the server counts a root's
//! directory when it first goes live and tells the ledger every change, and nothing is stored.

use alloc::string::String;
use alloc::vec::Vec;

/// The volume root's id (the server's `ROOT_ID`).
const VOLUME: u64 = 0;

/// A live root: a directory with a connection minted at it and not yet disconnected, or the
/// volume's root, which is always live.
struct Root {
    /// The directory's id.
    id: u64,
    /// Its path from the volume's root. It cannot change while the root is live: a rename or
    /// remove that would move or end a live root is refused.
    path: String,
    /// The bytes under it, less what lies under the live roots below it.
    held: u64,
}

/// A connection minted at a live root.
struct Conn {
    badge: u64,
    root: u64,
    quota: u64,
}

/// Why a mint was not recorded.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refusal<E> {
    /// The root's parent has no room for the quota, or a quota was asked at the granter's own
    /// root, or there was no memory for the record.
    Refused,
    /// Counting the new root failed.
    Count(E),
}

/// Whether `path` is the directory `dir` or lies under it.
pub(crate) fn under(path: &str, dir: &str) -> bool {
    dir.is_empty() || path.strip_prefix(dir).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// `n` copies of `v`, or `None` with no memory for them.
fn filled<T: Clone>(n: usize, v: T) -> Option<Vec<T>> {
    let mut out = Vec::new();
    out.try_reserve_exact(n).ok()?;
    out.resize(n, v);
    Some(out)
}

/// The live roots. The first is the volume's.
pub(crate) struct Ledger {
    /// The volume root's quota.
    room: u64,
    roots: Vec<Root>,
    conns: Vec<Conn>,
}

impl Ledger {
    /// The volume root alone, with `room` bytes and holding `held`.
    pub fn new(room: u64, held: u64) -> Ledger {
        let volume = Root { id: VOLUME, path: String::new(), held };
        Ledger { room, roots: alloc::vec![volume], conns: Vec::new() }
    }

    /// Root `i`'s quota: the sum of its connections' quotas; the volume root's is the volume's
    /// room.
    fn quota(&self, i: usize) -> u64 {
        let at = self.conns.iter().filter(|c| c.root == self.roots[i].id);
        if i == 0 { self.room } else { at.fold(0, |n, c| n.saturating_add(c.quota)) }
    }

    /// What root `i` keeps in reserve: the charges of the live roots nearest below it; `None`
    /// with no memory to count them.
    fn reserve(&self, i: usize) -> Option<u64> { self.totals().map(|t| t[i].0) }

    /// Every live root's reserve and charge. A root's charge is what it holds of its parent's
    /// room: the larger of its quota and what it holds and keeps in reserve, so minting a root
    /// never frees room in its parent. Computed afresh each time, it follows every change below,
    /// and shrinks with a root over its quota as it removes. One pass: the roots deepest path
    /// first, as a root's path is longer than its parent's, each adding its charge to its
    /// parent's reserve. `None` with no memory for the pass.
    fn totals(&self) -> Option<Vec<(u64, u64)>> {
        let n = self.roots.len();
        let (mut parent, mut order, mut totals) = (filled(n, 0)?, filled(n, 0)?, filled(n, (0u64, 0u64))?);
        for (j, r) in self.roots.iter().enumerate() {
            (parent[j], order[j]) = (self.above(&r.path), j);
        }
        order.sort_unstable_by_key(|&j| core::cmp::Reverse(self.roots[j].path.len()));
        for j in order {
            let charge = self.quota(j).max(self.roots[j].held.saturating_add(totals[j].0));
            totals[j].1 = charge;
            if j != 0 {
                totals[parent[j]].0 = totals[parent[j]].0.saturating_add(charge);
            }
        }
        Some(totals)
    }

    /// The live root nearest at or above the directory `dir`: who holds what lies in it.
    pub fn holder(&self, dir: &str) -> usize { self.nearest(|p| under(dir, p)) }

    /// The live root nearest strictly above the directory `dir`.
    fn above(&self, dir: &str) -> usize { self.nearest(|p| p != dir && under(dir, p)) }

    fn nearest(&self, f: impl Fn(&str) -> bool) -> usize {
        let found = self.roots.iter().enumerate().filter(|(_, r)| f(&r.path));
        found.max_by_key(|(_, r)| r.path.len()).map_or(0, |(i, _)| i)
    }

    /// Whether root `i` may grow by `need`: held and reserve stay within its quota. Without
    /// memory to count the reserve it may not.
    pub fn fits(&self, i: usize, need: u64) -> bool {
        let Some(reserve) = self.reserve(i) else { return false };
        let held = self.roots[i].held.checked_add(reserve);
        held.and_then(|n| n.checked_add(need)).is_some_and(|n| n <= self.quota(i))
    }

    /// What root `i` may still grow by: nothing without memory to count its reserve.
    pub fn spare(&self, i: usize) -> u64 {
        let spare = |reserve| self.quota(i).saturating_sub(self.roots[i].held).saturating_sub(reserve);
        self.reserve(i).map_or(0, spare)
    }

    /// Root `i` now holds `more` bytes more and `less` fewer.
    pub fn change(&mut self, i: usize, more: u64, less: u64) {
        let r = &mut self.roots[i];
        r.held = r.held.saturating_add(more).saturating_sub(less);
    }

    /// Whether a live root is the directory at `path` or lies under it.
    pub fn holds_live(&self, path: &str) -> bool { self.roots.iter().any(|r| under(&r.path, path)) }

    /// The live roots, `(id, charge)`, for a count that must skip them; `None` with no memory
    /// for the list.
    pub fn charges(&self) -> Option<Vec<(u64, u64)>> {
        let mut totals = self.totals()?;
        for (t, r) in totals.iter_mut().zip(&self.roots) {
            *t = (r.id, t.1);
        }
        Some(totals)
    }

    /// Every live root: id, path, quota, held and reserve.
    #[cfg(test)]
    pub fn roots(&self) -> Vec<(u64, String, u64, u64, u64)> {
        let totals = self.totals().unwrap();
        let root = |(i, r): (usize, &Root)| (r.id, r.path.clone(), self.quota(i), r.held, totals[i].0);
        self.roots.iter().enumerate().map(root).collect()
    }

    /// The root of the connection with `badge`: a minted one's, or the volume's for one that
    /// attached.
    fn root_of(&self, badge: u64) -> u64 {
        self.conns.iter().find(|c| c.badge == badge).map_or(VOLUME, |c| c.root)
    }

    /// Records the connection `badge`, minted through `granter`'s at the directory `id` at
    /// `path` with `quota` bytes (servers/littlefsd.md, "Quotas"). A directory not yet live is
    /// counted with `count`: what it holds and its reserve, the live roots below it skipped.
    /// The parent must have room for what the root's charge adds to it.
    pub fn mint<E>(
        &mut self,
        granter: u64,
        badge: u64,
        id: u64,
        path: &str,
        quota: u64,
        count: impl FnOnce() -> Result<(u64, u64), E>,
    ) -> Result<(), Refusal<E>> {
        self.conns.try_reserve(1).map_err(|_| Refusal::Refused)?;
        if id == self.root_of(granter) {
            // The granter's own root: the connection is that root and carves nothing.
            if quota != 0 {
                return Err(Refusal::Refused);
            }
        } else if let Some(i) = self.roots.iter().position(|r| r.id == id) {
            let sum = self.quota(i).checked_add(quota).ok_or(Refusal::Refused)?;
            let (reserve, was) = self.totals().ok_or(Refusal::Refused)?[i];
            let charge = sum.max(self.roots[i].held.saturating_add(reserve));
            if !self.fits(self.above(path), charge - was) {
                return Err(Refusal::Refused);
            }
        } else {
            let (held, reserve) = count().map_err(Refusal::Count)?;
            let parent = self.above(path);
            let left = self.roots[parent].held.saturating_sub(held);
            let kept = self.reserve(parent).ok_or(Refusal::Refused)?.saturating_sub(reserve);
            let charge = quota.max(held.saturating_add(reserve));
            let room = left.checked_add(kept).and_then(|n| n.checked_add(charge));
            if room.is_none_or(|n| n > self.quota(parent)) {
                return Err(Refusal::Refused);
            }
            let mut owned = String::new();
            owned.try_reserve(path.len()).map_err(|_| Refusal::Refused)?;
            owned.push_str(path);
            self.roots.try_reserve(1).map_err(|_| Refusal::Refused)?;
            self.roots[parent].held = left;
            self.roots.push(Root { id, path: owned, held });
        }
        self.conns.push(Conn { badge, root: id, quota });
        Ok(())
    }

    /// Undoes the carve of the connection `badge`. When the last connection at a directory
    /// goes, so does its record: what it held and its reserve return to the root above it,
    /// which may then be over its quota and grows no further until it is under.
    pub fn disconnect(&mut self, badge: u64) {
        let Some(c) = self.conns.iter().position(|c| c.badge == badge) else { return };
        let conn = self.conns.swap_remove(c);
        let Some(i) = self.roots.iter().position(|r| r.id == conn.root).filter(|i| *i != 0) else { return };
        if self.conns.iter().all(|c| c.root != conn.root) {
            let gone = self.roots.swap_remove(i);
            let parent = self.holder(&gone.path);
            self.roots[parent].held = self.roots[parent].held.saturating_add(gone.held);
        }
    }
}

#[cfg(test)]
#[path = "quota_tests.rs"]
mod tests;
