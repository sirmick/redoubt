//! A persistent ordered map: an AVL tree whose nodes are shared between versions.
//!
//! Cloning a map is O(1). Changing a clone copies only the O(log n) nodes on the path to the
//! change (`Rc::make_mut` copies a node only when another version still uses it), so building
//! a map one `maps:put` at a time is O(n log n) rather than O(n²), and old versions stay valid,
//! as Erlang semantics require. Keys are kept in order, so iteration is in key order.
//!
//! Recursion is bounded by the tree height, which AVL balancing keeps below 1.45·log2(n).

use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cmp::Ordering;

#[derive(Clone)]
struct Node<K, V> {
    key: K,
    value: V,
    left: Link<K, V>,
    right: Link<K, V>,
    height: u8,
}

type Link<K, V> = Option<Rc<Node<K, V>>>;

pub struct PMap<K, V> {
    root: Link<K, V>,
    len: usize,
}

impl<K, V> Clone for PMap<K, V> {
    fn clone(&self) -> Self {
        PMap { root: self.root.clone(), len: self.len }
    }
}

impl<K, V> Default for PMap<K, V> {
    fn default() -> Self {
        PMap { root: None, len: 0 }
    }
}

fn height<K, V>(l: &Link<K, V>) -> u8 {
    l.as_ref().map_or(0, |n| n.height)
}

impl<K: Ord + Clone, V: Clone> PMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        let mut cur = &self.root;
        while let Some(n) = cur {
            cur = match key.cmp(&n.key) {
                Ordering::Less => &n.left,
                Ordering::Greater => &n.right,
                Ordering::Equal => return Some(&n.value),
            };
        }
        None
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    /// Insert or replace; returns the old value.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let old = insert(&mut self.root, key, value);
        if old.is_none() {
            self.len += 1;
        }
        old
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let old = remove(&mut self.root, key);
        if old.is_some() {
            self.len -= 1;
        }
        old
    }

    /// The entries in key order.
    pub fn iter(&self) -> alloc::vec::IntoIter<(&K, &V)> {
        let mut out = Vec::with_capacity(self.len);
        let mut stack: Vec<&Node<K, V>> = Vec::new();
        let mut cur = self.root.as_deref();
        loop {
            while let Some(n) = cur {
                stack.push(n);
                cur = n.left.as_deref();
            }
            let Some(n) = stack.pop() else { break };
            out.push((&n.key, &n.value));
            cur = n.right.as_deref();
        }
        out.into_iter()
    }

    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &K> + ExactSizeIterator {
        self.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl DoubleEndedIterator<Item = &V> + ExactSizeIterator {
        self.iter().map(|(_, v)| v)
    }

    /// Take the map apart without recursion: the entries of every node no other version
    /// shares. Shared subtrees are only released. Used to drop deep terms iteratively.
    pub fn into_unique_entries(mut self) -> Vec<(K, V)> {
        let mut out = Vec::new();
        let mut stack = alloc::vec![self.root.take()];
        while let Some(link) = stack.pop() {
            let Some(rc) = link else { continue };
            if let Ok(node) = Rc::try_unwrap(rc) {
                out.push((node.key, node.value));
                stack.push(node.left);
                stack.push(node.right);
            }
        }
        self.len = 0;
        out
    }
}

impl<K: Ord + Clone, V: Clone> FromIterator<(K, V)> for PMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut m = PMap::new();
        for (k, v) in iter {
            m.insert(k, v);
        }
        m
    }
}

// ---- AVL operations on links. `Rc::make_mut` copies a node only if another version holds it. ----

fn update<K, V>(n: &mut Node<K, V>) {
    n.height = 1 + height(&n.left).max(height(&n.right));
}

fn balance_factor<K, V>(n: &Node<K, V>) -> i16 {
    height(&n.left) as i16 - height(&n.right) as i16
}

fn rotate_right<K: Clone, V: Clone>(link: &mut Link<K, V>) {
    let mut top = link.take().expect("rotation of an empty tree");
    let t = Rc::make_mut(&mut top);
    let mut left = t.left.take().expect("rotate_right needs a left child");
    let l = Rc::make_mut(&mut left);
    t.left = l.right.take();
    update(t);
    l.right = Some(top);
    update(l);
    *link = Some(left);
}

fn rotate_left<K: Clone, V: Clone>(link: &mut Link<K, V>) {
    let mut top = link.take().expect("rotation of an empty tree");
    let t = Rc::make_mut(&mut top);
    let mut right = t.right.take().expect("rotate_left needs a right child");
    let r = Rc::make_mut(&mut right);
    t.right = r.left.take();
    update(t);
    r.left = Some(top);
    update(r);
    *link = Some(right);
}

/// Restore the AVL property at `link` after one of its subtrees changed height by one.
fn rebalance<K: Clone, V: Clone>(link: &mut Link<K, V>) {
    let Some(rc) = link.as_mut() else { return };
    let n = Rc::make_mut(rc);
    update(n);
    let bf = balance_factor(n);
    if bf > 1 {
        if n.left.as_ref().is_some_and(|l| balance_factor(l) < 0) {
            rotate_left(&mut n.left);
        }
        rotate_right(link);
    } else if bf < -1 {
        if n.right.as_ref().is_some_and(|r| balance_factor(r) > 0) {
            rotate_right(&mut n.right);
        }
        rotate_left(link);
    }
}

fn insert<K: Ord + Clone, V: Clone>(link: &mut Link<K, V>, key: K, value: V) -> Option<V> {
    let Some(rc) = link.as_mut() else {
        *link = Some(Rc::new(Node { key, value, left: None, right: None, height: 1 }));
        return None;
    };
    let n = Rc::make_mut(rc);
    let old = match key.cmp(&n.key) {
        Ordering::Less => insert(&mut n.left, key, value),
        Ordering::Greater => insert(&mut n.right, key, value),
        Ordering::Equal => return Some(core::mem::replace(&mut n.value, value)),
    };
    rebalance(link);
    old
}

/// Remove and return the smallest entry of a non-empty subtree.
fn take_min<K: Ord + Clone, V: Clone>(link: &mut Link<K, V>) -> (K, V) {
    let n = Rc::make_mut(link.as_mut().expect("take_min of an empty tree"));
    if n.left.is_some() {
        let min = take_min(&mut n.left);
        rebalance(link);
        return min;
    }
    let right = n.right.take();
    let taken = link.take().expect("checked above");
    *link = right;
    match Rc::try_unwrap(taken) {
        Ok(node) => (node.key, node.value),
        Err(shared) => (shared.key.clone(), shared.value.clone()),
    }
}

fn remove<K: Ord + Clone, V: Clone>(link: &mut Link<K, V>, key: &K) -> Option<V> {
    let rc = link.as_mut()?;
    let old = match key.cmp(&rc.key) {
        Ordering::Less => remove(&mut Rc::make_mut(rc).left, key),
        Ordering::Greater => remove(&mut Rc::make_mut(rc).right, key),
        Ordering::Equal => {
            let n = Rc::make_mut(rc);
            let (left, right) = (n.left.take(), n.right.take());
            let removed = link.take().expect("checked above");
            let value = match Rc::try_unwrap(removed) {
                Ok(node) => node.value,
                Err(shared) => shared.value.clone(),
            };
            *link = match (left, right) {
                (None, r) => r,
                (l, None) => l,
                (l, r) => {
                    // Replace with the successor: the smallest entry on the right.
                    let mut r = r;
                    let (k, v) = take_min(&mut r);
                    let mut node = Node { key: k, value: v, left: l, right: r, height: 0 };
                    update(&mut node);
                    Some(Rc::new(node))
                }
            };
            rebalance(link);
            return Some(value);
        }
    };
    old.as_ref()?;
    rebalance(link);
    old
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;

    /// Check order, balance and cached heights of every node; return the height.
    fn check<K: Ord, V>(link: &Link<K, V>, lo: Option<&K>, hi: Option<&K>) -> u8 {
        let Some(n) = link else { return 0 };
        assert!(lo.is_none_or(|lo| *lo < n.key) && hi.is_none_or(|hi| n.key < *hi), "order");
        let (hl, hr) = (check(&n.left, lo, Some(&n.key)), check(&n.right, Some(&n.key), hi));
        assert!((hl as i16 - hr as i16).abs() <= 1, "balance");
        assert_eq!(n.height, 1 + hl.max(hr), "height");
        n.height
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    /// Random inserts and removes, compared step by step with `BTreeMap`, keeping old versions
    /// around to check that changing one version never changes another.
    #[test]
    fn behaves_like_btreemap_and_versions_are_independent() {
        let mut rng = Rng(0x1234_5678_9abc_def1);
        let mut model = BTreeMap::new();
        let mut map = PMap::new();
        let mut snapshots: Vec<(PMap<u32, u32>, BTreeMap<u32, u32>)> = Vec::new();
        for step in 0..20_000u32 {
            let k = (rng.next() % 500) as u32;
            if rng.next().is_multiple_of(3) {
                assert_eq!(map.remove(&k), model.remove(&k));
            } else {
                assert_eq!(map.insert(k, step), model.insert(k, step));
            }
            assert_eq!(map.len(), model.len());
            if step % 1000 == 0 {
                check(&map.root, None, None);
                snapshots.push((map.clone(), model.clone()));
            }
        }
        check(&map.root, None, None);
        assert!(map.iter().map(|(k, v)| (*k, *v)).eq(model.iter().map(|(k, v)| (*k, *v))));
        for (snap, model) in &snapshots {
            assert!(snap.iter().map(|(k, v)| (*k, *v)).eq(model.iter().map(|(k, v)| (*k, *v))));
            check(&snap.root, None, None);
        }
        for k in 0..500 {
            assert_eq!(map.get(&k), model.get(&k));
        }
    }

    #[test]
    fn sequential_keys_stay_balanced() {
        let map: PMap<u32, ()> = (0..100_000).map(|k| (k, ())).collect();
        let h = check(&map.root, None, None);
        assert!(h <= 25, "height {h} for 100000 keys");
        let entries = map.clone().into_unique_entries();
        assert!(entries.is_empty(), "every node is shared with `map`");
        assert_eq!(map.into_unique_entries().len(), 100_000);
    }
}
