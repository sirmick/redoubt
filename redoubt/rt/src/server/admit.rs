//! Admission (CONTAINMENT.md, the shared server library: `admit(badge, account, labels)`): limits
//! on what one client may hold in a shared server at once, so that one client cannot use up a
//! server that serves others.
//!
//! - **Buckets.** Limits are per (account, label set): accounts, not badges or budgets, because both of those
//!   are cheap to create; with the label set, so that a vault session filling a server's slots is not visible
//!   to its owner's unlabelled session, which shares the account. Account 0 (every system-class caller) is
//!   admitted per badge, so one daemon cannot fill a bucket the steward needs.
//! - **A fair share per badge within a bucket** (answer 90), with the bucket as the ceiling, so an agent
//!   cannot lock out its sponsor, who shares its bucket. A share may take one more of a resource while it
//!   holds less than `limit / (n + 1)`, where n is the shares in the bucket holding that resource, itself
//!   included: whatever the others hold, a share always leaves room for one more share's worth. So an agent
//!   flooding its sponsor's bucket alone gets half of it, and its sponsor can still take a third. The unit of
//!   a share is the caller's badge; the 9P skeleton counts a connection a client minted for itself in the
//!   share of the connection it minted it from, so minting more connections gains nothing
//!   ([`crate::server::ninep`], Connections).
//! - **Caps sized to fit** (answers 81, 85): at most [`Limits::buckets`] buckets hold anything at once, so
//!   that every bucket at its cap fits the server's budget ([`Limits::fits`]) and the calls they may hold
//!   open sum to less than `MAX_OPEN_CALLS` with headroom (checked by [`Admission::new`]). A bucket beyond
//!   that is refused. Stated residual: a server sized for fewer buckets than it serves refuses the
//!   latecomers, which tells them that others hold state; size a server for every (account, label set) it
//!   serves.

use alloc::vec::Vec;

use redoubt_sys::{MAX_LABELS, MAX_OPEN_CALLS};

use crate::ipc::Caller;

/// Open calls a server keeps free beyond what admission lets its buckets hold: the calls it is
/// answering right now, requests answered ahead of admission (a sponsor ending a lease), and
/// abandoned calls not yet replied to.
pub const OPEN_CALL_HEADROOM: usize = MAX_OPEN_CALLS / 4;

/// What admission is keyed by: the caller's account and label set (CONTAINMENT.md), and for
/// account 0 ("none": system-class callers) the badge as well. [`AdmitKey::of`] is the one place
/// the key is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AdmitKey {
    account: u64,
    /// The caller's badge when its account is 0; otherwise 0.
    badge: u64,
    /// The labels, sorted and deduplicated, in the first `count` slots; the rest are 0.
    labels: [u64; MAX_LABELS],
    count: usize,
}

impl AdmitKey {
    /// The key for a message's sender. The kernel already sorts and deduplicates the labels it
    /// attaches; doing it again here makes the key a set whatever the source.
    pub fn of(caller: &Caller) -> AdmitKey {
        let mut labels = [0; MAX_LABELS];
        let mut count = 0;
        for label in caller.labels.as_slice() {
            if !labels[..count].contains(label) && count < MAX_LABELS {
                labels[count] = *label;
                count += 1;
            }
        }
        labels[..count].sort_unstable();
        let badge = if caller.account == 0 { caller.badge } else { 0 };
        AdmitKey { account: caller.account, badge, labels, count }
    }

    /// Whether the bucket is one badge's alone (account 0), so it has no shares to divide.
    fn per_badge(&self) -> bool { self.account == 0 }
}

/// The kinds of thing admission counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    /// Calls a server has taken and holds open (parked, [`crate::server::parked`]).
    InFlight = 0,
    /// Open files, or 9P fids.
    Files = 1,
    /// Any other per-client state the server keeps: 9P connections minted by `new_connection`.
    State = 2,
}

const RESOURCES: usize = 3;

/// The most of each [`Resource`] one bucket may hold, and how many buckets may hold anything at
/// once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub buckets: u32,
    pub in_flight: u32,
    pub files: u32,
    pub state: u32,
}

/// What one of each [`Resource`] costs the server, in bytes of its budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cost {
    pub in_flight: u64,
    pub file: u64,
    pub state: u64,
}

impl Limits {
    fn of(&self, resource: Resource) -> u32 {
        match resource {
            Resource::InFlight => self.in_flight,
            Resource::Files => self.files,
            Resource::State => self.state,
        }
    }

    /// The calls every bucket at its cap holds open together.
    pub fn open_calls(&self) -> u64 { u64::from(self.buckets) * u64::from(self.in_flight) }

    /// Whether every bucket at its cap fits `budget` bytes (answer 85): the server's own use
    /// comes on top, and a server's budget must also cover lends of abandoned calls (R3).
    pub fn fits(&self, cost: &Cost, budget: u64) -> bool {
        let one = [
            u64::from(self.in_flight).checked_mul(cost.in_flight),
            u64::from(self.files).checked_mul(cost.file),
            u64::from(self.state).checked_mul(cost.state),
        ];
        let bucket = one.iter().try_fold(0u64, |sum, part| sum.checked_add((*part)?));
        bucket.and_then(|b| b.checked_mul(u64::from(self.buckets))).is_some_and(|all| all <= budget)
    }
}

/// Admission was refused: the bucket or the share is at its limit for that resource, too many
/// buckets hold something, or the server has no memory to track one more.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused;

/// The limits break the sizing rule: the calls the buckets may hold open would not leave
/// [`OPEN_CALL_HEADROOM`] of `MAX_OPEN_CALLS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unsized;

/// Counts what each bucket, and each share within it, holds. Nothing holding nothing has an
/// entry, so the tables only ever hold what currently holds something. Plain vectors searched
/// linearly: growing them can fail cleanly (`try_reserve`), and a server has few clients.
#[derive(Debug)]
pub struct Admission {
    limits: Limits,
    buckets: Vec<(AdmitKey, [u32; RESOURCES])>,
    shares: Vec<(AdmitKey, u64, [u32; RESOURCES])>,
}

impl Admission {
    /// Admission under `limits`, if they leave the open-call headroom.
    pub fn new(limits: Limits) -> Result<Admission, Unsized> {
        if limits.open_calls() > (MAX_OPEN_CALLS - OPEN_CALL_HEADROOM) as u64 {
            return Err(Unsized);
        }
        Ok(Admission { limits, buckets: Vec::new(), shares: Vec::new() })
    }

    pub fn limits(&self) -> &Limits { &self.limits }

    fn bucket(&self, key: AdmitKey) -> Option<usize> { self.buckets.iter().position(|(k, _)| *k == key) }

    fn share(&self, key: AdmitKey, share: u64) -> Option<usize> {
        self.shares.iter().position(|(k, s, _)| *k == key && *s == share)
    }

    /// Takes one `resource` for `share` (a badge) in `key`'s bucket, or refuses.
    pub fn admit(&mut self, key: AdmitKey, share: u64, resource: Resource) -> Result<(), Refused> {
        let r = resource as usize;
        let limit = self.limits.of(resource);
        let bucket = self.bucket(key);
        let total = bucket.map_or(0, |i| self.buckets[i].1[r]);
        if total >= limit || (bucket.is_none() && self.buckets.len() >= self.limits.buckets as usize) {
            return Err(Refused);
        }
        let entry = self.share(key, share);
        let held = entry.map_or(0, |i| self.shares[i].2[r]);
        if !key.per_badge() {
            // The shares holding this resource, this one included, plus room for one more.
            let others = self.shares.iter().filter(|(k, s, c)| *k == key && *s != share && c[r] > 0).count();
            let divisor = u32::try_from(others).unwrap_or(u32::MAX).saturating_add(2);
            if held >= (limit / divisor).max(1) {
                return Err(Refused);
            }
        }
        // Make room for both entries before changing either, so a failure changes nothing.
        let (need_bucket, need_share) = (usize::from(bucket.is_none()), usize::from(entry.is_none()));
        self.buckets.try_reserve(need_bucket).map_err(|_| Refused)?;
        self.shares.try_reserve(need_share).map_err(|_| Refused)?;
        let bucket = bucket.unwrap_or_else(|| {
            self.buckets.push((key, [0; RESOURCES]));
            self.buckets.len() - 1
        });
        let entry = entry.unwrap_or_else(|| {
            self.shares.push((key, share, [0; RESOURCES]));
            self.shares.len() - 1
        });
        self.buckets[bucket].1[r] += 1;
        self.shares[entry].2[r] += 1;
        Ok(())
    }

    /// Gives back one `resource` taken by [`Admission::admit`] with the same key and share.
    /// Releasing what was never taken is a server bug; it changes nothing rather than
    /// underflowing.
    pub fn release(&mut self, key: AdmitKey, share: u64, resource: Resource) {
        let r = resource as usize;
        let (Some(bucket), Some(entry)) = (self.bucket(key), self.share(key, share)) else { return };
        if self.shares[entry].2[r] == 0 {
            return;
        }
        self.shares[entry].2[r] -= 1;
        self.buckets[bucket].1[r] = self.buckets[bucket].1[r].saturating_sub(1);
        if self.shares[entry].2.iter().all(|c| *c == 0) {
            self.shares.swap_remove(entry);
        }
        if self.buckets[bucket].1.iter().all(|c| *c == 0) {
            self.buckets.swap_remove(bucket);
        }
    }

    /// How much of `resource` `key`'s bucket holds.
    pub fn held(&self, key: AdmitKey, resource: Resource) -> u32 {
        self.bucket(key).map_or(0, |i| self.buckets[i].1[resource as usize])
    }

    /// How much of `resource` one share of `key`'s bucket holds.
    pub fn held_by(&self, key: AdmitKey, share: u64, resource: Resource) -> u32 {
        self.share(key, share).map_or(0, |i| self.shares[i].2[resource as usize])
    }

    /// How many buckets hold anything.
    pub fn keys(&self) -> usize { self.buckets.len() }
}

#[cfg(test)]
mod tests {
    use redoubt_sys::Labels;

    use super::*;

    fn caller(account: u64, labels: &[u64]) -> Caller {
        Caller { badge: 7, account, labels: Labels::from_slice(labels).unwrap() }
    }

    fn key(account: u64) -> AdmitKey { AdmitKey::of(&caller(account, &[])) }

    fn limits(in_flight: u32, files: u32, state: u32) -> Limits {
        Limits { buckets: 8, in_flight, files, state }
    }

    #[test]
    fn limits_are_per_key_and_per_resource() {
        // One share per bucket, so the share's cap (half, for the one share that may come) binds.
        let mut a = Admission::new(limits(4, 2, 0)).unwrap();
        assert_eq!(a.admit(key(1), 1, Resource::InFlight), Ok(()));
        assert_eq!(a.admit(key(1), 1, Resource::InFlight), Ok(()));
        assert_eq!(a.admit(key(1), 1, Resource::InFlight), Err(Refused));
        // Another account is unaffected by the first one's use.
        assert_eq!(a.admit(key(2), 1, Resource::InFlight), Ok(()));
        assert_eq!(a.admit(key(1), 1, Resource::Files), Ok(()));
        assert_eq!(a.admit(key(1), 1, Resource::Files), Err(Refused));
        assert_eq!(a.admit(key(3), 1, Resource::State), Err(Refused));
        assert_eq!(a.keys(), 2, "a refused key with nothing held leaves no entry");
        a.release(key(1), 1, Resource::InFlight);
        assert_eq!(a.held(key(1), Resource::InFlight), 1);
        assert_eq!(a.admit(key(1), 1, Resource::InFlight), Ok(()));
    }

    #[test]
    fn released_keys_leave_the_table() {
        let mut a = Admission::new(Limits { buckets: 100, in_flight: 0, files: 1, state: 1 }).unwrap();
        for account in 0..100 {
            a.admit(key(account), 1, Resource::Files).unwrap();
        }
        assert_eq!(a.keys(), 100);
        for account in 0..100 {
            a.release(key(account), 1, Resource::Files);
            a.release(key(account), 1, Resource::Files); // never taken twice: no underflow
        }
        assert_eq!(a.keys(), 0);
        assert!(a.shares.is_empty());
        assert_eq!(a.held(key(5), Resource::Files), 0);
    }

    #[test]
    fn the_key_is_the_account_and_the_label_set() {
        // CONTAINMENT.md: per (account, label set). A vault session filling its slots leaves
        // its owner's unlabelled session (same account) untouched.
        let owner = caller(9, &[]);
        let vault = caller(9, &[5]);
        assert_ne!(AdmitKey::of(&owner), AdmitKey::of(&vault));
        let mut a = Admission::new(limits(1, 1, 1)).unwrap();
        a.admit(AdmitKey::of(&vault), 1, Resource::Files).unwrap();
        assert_eq!(a.admit(AdmitKey::of(&vault), 2, Resource::Files), Err(Refused));
        assert_eq!(a.admit(AdmitKey::of(&owner), 1, Resource::Files), Ok(()));
        // The badge does not make the key, and the label set is a set: order and repeats do not.
        let other_badge = Caller { badge: 99, ..vault };
        assert_eq!(AdmitKey::of(&other_badge), AdmitKey::of(&vault));
        // Except for account 0, admitted per badge: a daemon filling its slots leaves the
        // steward's alone.
        let (daemon, steward) = (caller(0, &[]), Caller { badge: 8, ..caller(0, &[]) });
        assert_ne!(AdmitKey::of(&daemon), AdmitKey::of(&steward));
        assert_eq!(AdmitKey::of(&caller(9, &[3, 1, 3])), AdmitKey::of(&caller(9, &[1, 3])));
    }

    /// Answer 90's attack: an agent sharing its sponsor's bucket floods it, alone at first, then
    /// with a second badge; its sponsor still gets a share.
    #[test]
    fn an_agent_flooding_a_bucket_leaves_its_sponsor_a_share() {
        let (agent, agent2, sponsor) = (1, 2, 3);
        let mut a = Admission::new(Limits { buckets: 4, in_flight: 0, files: 12, state: 0 }).unwrap();
        let alice = key(1001);
        while a.admit(alice, agent, Resource::Files).is_ok() {}
        assert_eq!(a.held_by(alice, agent, Resource::Files), 6, "alone, half the bucket");
        assert_eq!(a.admit(alice, sponsor, Resource::Files), Ok(()));
        while a.admit(alice, agent2, Resource::Files).is_ok() {}
        // The agent's two badges hold 6 + 3; the sponsor can still reach its share of 12 / 4.
        let mut sponsor_holds = 1;
        while a.admit(alice, sponsor, Resource::Files).is_ok() {
            sponsor_holds += 1;
        }
        assert_eq!(sponsor_holds, 3);
        assert_eq!(a.held(alice, Resource::Files), 12, "the bucket is the ceiling");
        // A share over its fair share keeps what it holds but takes no more.
        assert_eq!(a.admit(alice, agent, Resource::Files), Err(Refused));
        // Account 0 is admitted per badge, so a lone badge has its whole bucket.
        let daemon = key(0);
        while a.admit(daemon, 0, Resource::Files).is_ok() {}
        assert_eq!(a.held(daemon, Resource::Files), 12);
    }

    #[test]
    fn a_share_is_never_refused_for_want_of_a_fair_share_of_one() {
        let mut a = Admission::new(Limits { buckets: 1, in_flight: 0, files: 1, state: 0 }).unwrap();
        assert_eq!(a.admit(key(1), 1, Resource::Files), Ok(()));
        assert_eq!(a.admit(key(1), 2, Resource::Files), Err(Refused), "the bucket is full");
    }

    #[test]
    fn buckets_are_bounded_so_caps_fit() {
        let mut a = Admission::new(Limits { buckets: 2, in_flight: 1, files: 1, state: 1 }).unwrap();
        a.admit(key(1), 1, Resource::Files).unwrap();
        a.admit(key(2), 1, Resource::Files).unwrap();
        assert_eq!(a.admit(key(3), 1, Resource::Files), Err(Refused));
        a.release(key(1), 1, Resource::Files);
        assert_eq!(a.admit(key(3), 1, Resource::Files), Ok(()));
    }

    #[test]
    fn open_calls_leave_headroom() {
        let most = (MAX_OPEN_CALLS - OPEN_CALL_HEADROOM) as u32;
        assert!(Admission::new(Limits { buckets: most, in_flight: 1, files: 1, state: 1 }).is_ok());
        assert!(Admission::new(Limits { buckets: most + 1, in_flight: 1, files: 1, state: 1 }).is_err());
        assert!(Admission::new(Limits { buckets: 4, in_flight: most / 4 + 1, files: 0, state: 0 }).is_err());
        assert!(
            Admission::new(Limits { buckets: u32::MAX, in_flight: u32::MAX, files: 0, state: 0 }).is_err()
        );
    }

    #[test]
    fn caps_fit_the_budget() {
        let cost = Cost { in_flight: 4096, file: 512, state: 256 };
        let l = Limits { buckets: 4, in_flight: 2, files: 10, state: 2 };
        let need = 4 * (2 * 4096 + 10 * 512 + 2 * 256);
        assert!(l.fits(&cost, need));
        assert!(!l.fits(&cost, need - 1));
        assert!(!Limits { buckets: u32::MAX, ..l }.fits(&Cost { file: u64::MAX, ..cost }, u64::MAX));
    }
}
