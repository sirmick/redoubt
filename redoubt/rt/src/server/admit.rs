//! Admission (CONTAINMENT.md, the shared server library): per-account limits on what one
//! account may hold in a shared server at once, so one account cannot use up a server that
//! serves others. Accounts, not badges or budgets, because both of those are cheap to create.

use alloc::collections::BTreeMap;

use crate::ipc::Caller;

/// What admission is keyed by: the caller's account, as CONTAINMENT.md says today.
///
/// QUESTIONS.md 38 (open) asks whether this should be (account, label set), like the kernel's
/// `WAIT_CAP`, so that a vault session filling a server's slots cannot signal to its owner's
/// unlabelled session. Changing it is this one type and [`AdmitKey::of`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AdmitKey(u64);

impl AdmitKey {
    /// The key for a message's sender.
    pub fn of(caller: &Caller) -> AdmitKey { AdmitKey(caller.account) }
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

/// The most of each [`Resource`] one key may hold.
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

/// Admission was refused: the key is at its limit for that resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused;

/// Counts what each key holds. A key holding nothing has no entry, so the table only ever holds
/// keys that currently hold something.
#[derive(Debug)]
pub struct Admission {
    limits: Limits,
    held: BTreeMap<AdmitKey, [u32; 3]>,
}

impl Admission {
    pub fn new(limits: Limits) -> Admission { Admission { limits, held: BTreeMap::new() } }

    /// Takes one `resource` for `key`, or refuses if it already holds its limit.
    pub fn admit(&mut self, key: AdmitKey, resource: Resource) -> Result<(), Refused> {
        let limit = self.limits.of(resource);
        let counts = self.held.entry(key).or_default();
        let count = &mut counts[resource as usize];
        if *count >= limit {
            if counts.iter().all(|c| *c == 0) {
                self.held.remove(&key);
            }
            return Err(Refused);
        }
        *count += 1;
        Ok(())
    }

    /// Gives back one `resource` taken by [`Admission::admit`]. Releasing what was never taken
    /// is a server bug; it changes nothing rather than underflowing.
    pub fn release(&mut self, key: AdmitKey, resource: Resource) {
        if let Some(counts) = self.held.get_mut(&key) {
            let count = &mut counts[resource as usize];
            *count = count.saturating_sub(1);
            if counts.iter().all(|c| *c == 0) {
                self.held.remove(&key);
            }
        }
    }

    /// How much of `resource` `key` holds.
    pub fn held(&self, key: AdmitKey, resource: Resource) -> u32 {
        self.held.get(&key).map_or(0, |counts| counts[resource as usize])
    }

    /// How many keys hold anything.
    pub fn keys(&self) -> usize { self.held.len() }
}

#[cfg(test)]
mod tests {
    use redoubt_sys::Labels;

    use super::*;

    fn key(account: u64) -> AdmitKey {
        AdmitKey::of(&Caller { badge: 7, account, labels: Labels::from_slice(&[account]).unwrap() })
    }

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
    fn the_key_is_the_account() {
        // Today's rule (CONTAINMENT.md): labels and badge do not change the key. Question 38
        // may change this; the test then changes with `AdmitKey::of`.
        let a = Caller { badge: 1, account: 9, labels: Labels::from_slice(&[]).unwrap() };
        let b = Caller { badge: 2, account: 9, labels: Labels::from_slice(&[5]).unwrap() };
        assert_eq!(AdmitKey::of(&a), AdmitKey::of(&b));
    }
}
