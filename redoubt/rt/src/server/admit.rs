//! Admission (CONTAINMENT.md, the shared server library: `admit(account, labels)`): limits on
//! what one (account, label set) may hold in a shared server at once, so that one client cannot
//! use up a server that serves others. Accounts, not badges or budgets, because both of those are
//! cheap to create; with the label set, so that a vault session filling a server's slots is not
//! visible to its owner's unlabelled session, which shares the account.

use alloc::vec::Vec;

use redoubt_sys::MAX_LABELS;

use crate::ipc::Caller;

/// What admission is keyed by: the caller's account and label set (CONTAINMENT.md), and for
/// account 0 ("none": system-class callers) the badge as well, so one daemon cannot use up what
/// the steward needs (answer 53). [`AdmitKey::of`] is the one place the key is made.
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
}

/// The kinds of thing admission counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    /// Calls a server has taken and not yet replied to (it holds them open, like a blocked read).
    InFlight = 0,
    /// Open files, or 9P fids.
    Files = 1,
    /// Any other per-client state the server keeps.
    State = 2,
}

/// The most of each [`Resource`] one key may hold. The 9P skeleton consumes only `files`;
/// `in_flight` and `state` are for servers that hold calls open or keep other per-client state,
/// and count only what those servers admit themselves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub in_flight: u32,
    pub files: u32,
    pub state: u32,
}

impl Limits {
    fn of(&self, resource: Resource) -> u32 {
        match resource {
            Resource::InFlight => self.in_flight,
            Resource::Files => self.files,
            Resource::State => self.state,
        }
    }
}

/// Admission was refused: the key is at its limit for that resource, or the server has no
/// memory to track one more key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused;

/// Counts what each key holds. A key holding nothing has no entry, so the table only ever holds
/// keys that currently hold something. A plain vector searched linearly: growing it can fail
/// cleanly (`try_reserve`), and a server has few distinct clients.
#[derive(Debug)]
pub struct Admission {
    limits: Limits,
    held: Vec<(AdmitKey, [u32; 3])>,
}

impl Admission {
    pub fn new(limits: Limits) -> Admission { Admission { limits, held: Vec::new() } }

    fn find(&self, key: AdmitKey) -> Option<usize> { self.held.iter().position(|(k, _)| *k == key) }

    /// Takes one `resource` for `key`, or refuses if it already holds its limit.
    pub fn admit(&mut self, key: AdmitKey, resource: Resource) -> Result<(), Refused> {
        let limit = self.limits.of(resource);
        let index = match self.find(key) {
            Some(index) => index,
            None if limit == 0 => return Err(Refused),
            None => {
                self.held.try_reserve(1).map_err(|_| Refused)?;
                self.held.push((key, [0; 3]));
                self.held.len() - 1
            }
        };
        let count = &mut self.held[index].1[resource as usize];
        if *count >= limit {
            return Err(Refused);
        }
        *count += 1;
        Ok(())
    }

    /// Gives back one `resource` taken by [`Admission::admit`]. Releasing what was never taken
    /// is a server bug; it changes nothing rather than underflowing.
    pub fn release(&mut self, key: AdmitKey, resource: Resource) {
        if let Some(index) = self.find(key) {
            let counts = &mut self.held[index].1;
            counts[resource as usize] = counts[resource as usize].saturating_sub(1);
            if counts.iter().all(|c| *c == 0) {
                self.held.swap_remove(index);
            }
        }
    }

    /// How much of `resource` `key` holds.
    pub fn held(&self, key: AdmitKey, resource: Resource) -> u32 {
        self.find(key).map_or(0, |index| self.held[index].1[resource as usize])
    }

    /// How many keys hold anything.
    pub fn keys(&self) -> usize { self.held.len() }
}

#[cfg(test)]
mod tests {
    use redoubt_sys::Labels;

    use super::*;

    fn caller(account: u64, labels: &[u64]) -> Caller {
        Caller { badge: 7, account, labels: Labels::from_slice(labels).unwrap() }
    }

    fn key(account: u64) -> AdmitKey { AdmitKey::of(&caller(account, &[])) }

    #[test]
    fn limits_are_per_key_and_per_resource() {
        let mut a = Admission::new(Limits { in_flight: 2, files: 1, state: 0 });
        assert_eq!(a.admit(key(1), Resource::InFlight), Ok(()));
        assert_eq!(a.admit(key(1), Resource::InFlight), Ok(()));
        assert_eq!(a.admit(key(1), Resource::InFlight), Err(Refused));
        // Another account is unaffected by the first one's use.
        assert_eq!(a.admit(key(2), Resource::InFlight), Ok(()));
        assert_eq!(a.admit(key(1), Resource::Files), Ok(()));
        assert_eq!(a.admit(key(1), Resource::Files), Err(Refused));
        assert_eq!(a.admit(key(3), Resource::State), Err(Refused));
        assert_eq!(a.keys(), 2, "a refused key with nothing held leaves no entry");
        a.release(key(1), Resource::InFlight);
        assert_eq!(a.held(key(1), Resource::InFlight), 1);
        assert_eq!(a.admit(key(1), Resource::InFlight), Ok(()));
    }

    #[test]
    fn released_keys_leave_the_table() {
        let mut a = Admission::new(Limits { in_flight: 1, files: 1, state: 1 });
        for account in 0..100 {
            a.admit(key(account), Resource::Files).unwrap();
        }
        assert_eq!(a.keys(), 100);
        for account in 0..100 {
            a.release(key(account), Resource::Files);
            a.release(key(account), Resource::Files); // never taken twice: no underflow
        }
        assert_eq!(a.keys(), 0);
        assert_eq!(a.held(key(5), Resource::Files), 0);
    }

    #[test]
    fn the_key_is_the_account_and_the_label_set() {
        // CONTAINMENT.md: per (account, label set). A vault session filling its slots leaves
        // its owner's unlabelled session (same account) untouched.
        let owner = caller(9, &[]);
        let vault = caller(9, &[5]);
        assert_ne!(AdmitKey::of(&owner), AdmitKey::of(&vault));
        let mut a = Admission::new(Limits { in_flight: 1, files: 1, state: 1 });
        a.admit(AdmitKey::of(&vault), Resource::Files).unwrap();
        assert_eq!(a.admit(AdmitKey::of(&vault), Resource::Files), Err(Refused));
        assert_eq!(a.admit(AdmitKey::of(&owner), Resource::Files), Ok(()));
        // The badge does not matter, and the label set is a set: order and repeats do not.
        let other_badge = Caller { badge: 99, ..vault };
        assert_eq!(AdmitKey::of(&other_badge), AdmitKey::of(&vault));
        // Except for account 0, admitted per badge (answer 53): a daemon filling its slots
        // leaves the steward's alone.
        let (daemon, steward) = (caller(0, &[]), Caller { badge: 8, ..caller(0, &[]) });
        assert_ne!(AdmitKey::of(&daemon), AdmitKey::of(&steward));
        assert_eq!(AdmitKey::of(&caller(9, &[3, 1, 3])), AdmitKey::of(&caller(9, &[1, 3])));
    }
}
