//! The label check (CONTAINMENT.md, the shared server library): no read up, no write down.
//! System servers are exempt from the kernel's check (R1) and apply this one to every request,
//! using the label set the kernel attached to the message.

/// What a request does to an object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Information flows from the object to the caller.
    Read,
    /// Information flows from the caller into the object.
    Write,
}

/// The check failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Denied;

/// **No read up**: read only if the object's labels ⊆ the caller's. **No write down**: write only
/// if the caller's labels ⊆ the object's. Label sets are compared as sets: order and repeats do
/// not matter.
pub fn check(caller_labels: &[u64], object_labels: &[u64], access: Access) -> Result<(), Denied> {
    let allowed = match access {
        Access::Read => subset(object_labels, caller_labels),
        Access::Write => subset(caller_labels, object_labels),
    };
    if allowed { Ok(()) } else { Err(Denied) }
}

/// `a ⊆ b`. Label sets hold at most `MAX_LABELS` (8), so the quadratic scan is the simplest
/// correct choice.
fn subset(a: &[u64], b: &[u64]) -> bool { a.iter().all(|label| b.contains(label)) }

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeSet;
    use alloc::vec::Vec;

    use super::*;

    /// Deterministic xorshift, so a failure reproduces.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        /// Up to 8 labels (with repeats, unsorted) from a small universe, so sets often overlap.
        fn labels(&mut self) -> Vec<u64> {
            let n = self.next() % 9;
            (0..n).map(|_| self.next() % 6).collect()
        }
    }

    fn set(labels: &[u64]) -> BTreeSet<u64> { labels.iter().copied().collect() }

    #[test]
    fn matches_the_set_definition() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for _ in 0..200_000 {
            let (caller, object) = (rng.labels(), rng.labels());
            let (c, o) = (set(&caller), set(&object));
            assert_eq!(
                check(&caller, &object, Access::Read).is_ok(),
                o.is_subset(&c),
                "{caller:?} read {object:?}"
            );
            assert_eq!(
                check(&caller, &object, Access::Write).is_ok(),
                c.is_subset(&o),
                "{caller:?} write {object:?}"
            );
        }
    }

    #[test]
    fn properties() {
        let mut rng = Rng(0x1234_5678_9abc_def1);
        for _ in 0..100_000 {
            let (a, b, x) = (rng.labels(), rng.labels(), rng.labels());
            let read = |c: &[u64], o: &[u64]| check(c, o, Access::Read).is_ok();
            let write = |c: &[u64], o: &[u64]| check(c, o, Access::Write).is_ok();
            // Reading and writing the same object needs exactly its label set.
            assert_eq!(read(&a, &b) && write(&a, &b), set(&a) == set(&b));
            // Everyone may read an unlabelled object and write into their own labels.
            assert!(read(&a, &[]) && read(&a, &a) && write(&a, &a));
            // A labelled caller can never write where an unlabelled one can read: no write down.
            if !set(&a).is_empty() {
                assert!(!write(&a, &[]));
            }
            // Duality: a may write into b exactly when b may read what a holds.
            assert_eq!(write(&a, &b), read(&b, &a));
            // Information cannot flow from a to x through b by a write then a read unless a
            // could read-flow to x directly (transitivity of ⊆).
            if write(&a, &b) && read(&x, &b) {
                assert!(read(&x, &a), "{a:?} -> {b:?} -> {x:?}");
            }
            // Order and repeats do not matter.
            let mut shuffled = a.clone();
            shuffled.reverse();
            shuffled.extend_from_slice(&a);
            assert_eq!(read(&shuffled, &b), read(&a, &b));
            assert_eq!(write(&shuffled, &b), write(&a, &b));
        }
    }
}
