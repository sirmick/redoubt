//! Maps: persistent AVL trees whose nodes are heap objects, ordered by the exact term order.
//!
//! A map is `[len, root]`; a node is `[key, value, left, right, height]`, its children
//! [`Term::Node`]s or `[]`. Nodes never change: an update builds new nodes along the path to the
//! change and shares the rest, so `maps:put` on a map that is still in use costs O(log n), and
//! old versions stay valid, as Erlang semantics require. Iteration is in key order.
//!
//! Recursion is bounded by the tree height, which AVL balancing keeps below 1.45·log2(n).

use alloc::vec::Vec;
use core::cmp::Ordering;

use super::{compare, Heap, Kind, Term};

/// A node's cells.
#[derive(Clone, Copy)]
struct Node {
    key: Term,
    value: Term,
    left: Term,
    right: Term,
    height: u8,
}

impl Heap {
    fn node(&self, n: Term) -> Option<Node> {
        let Term::Node(p) = n else { return None };
        let c = self.object(p).1;
        let Term::Int(height) = c[4] else { unreachable!("a height") };
        Some(Node { key: c[0], value: c[1], left: c[2], right: c[3], height: height as u8 })
    }

    fn height(&self, n: Term) -> u8 {
        self.node(n).map_or(0, |n| n.height)
    }

    fn new_node(&mut self, key: Term, value: Term, left: Term, right: Term) -> Term {
        let height = 1 + self.height(left).max(self.height(right));
        Term::Node(self.push_object(Kind::Node, &[key, value, left, right, Term::Int(height as i64)]))
    }

    /// A node for `key`, `value` over subtrees whose heights differ by at most two, balanced.
    fn balanced(&mut self, key: Term, value: Term, left: Term, right: Term) -> Term {
        let (hl, hr) = (self.height(left), self.height(right));
        if hl > hr + 1 {
            let l = self.node(left).expect("the taller side is a node");
            if self.height(l.left) >= self.height(l.right) {
                let r = self.new_node(key, value, l.right, right);
                return self.new_node(l.key, l.value, l.left, r);
            }
            let lr = self.node(l.right).expect("the taller side is a node");
            let a = self.new_node(l.key, l.value, l.left, lr.left);
            let b = self.new_node(key, value, lr.right, right);
            return self.new_node(lr.key, lr.value, a, b);
        }
        if hr > hl + 1 {
            let r = self.node(right).expect("the taller side is a node");
            if self.height(r.right) >= self.height(r.left) {
                let l = self.new_node(key, value, left, r.left);
                return self.new_node(r.key, r.value, l, r.right);
            }
            let rl = self.node(r.left).expect("the taller side is a node");
            let a = self.new_node(key, value, left, rl.left);
            let b = self.new_node(r.key, r.value, rl.right, r.right);
            return self.new_node(rl.key, rl.value, a, b);
        }
        self.new_node(key, value, left, right)
    }

    /// The tree with `key` set to `value`, and whether the key is new.
    fn insert(&mut self, n: Term, key: Term, value: Term) -> (Term, bool) {
        let Some(node) = self.node(n) else { return (self.new_node(key, value, Term::Nil, Term::Nil), true) };
        match compare(self, key, self, node.key, true) {
            Ordering::Less => {
                let (left, added) = self.insert(node.left, key, value);
                (self.balanced(node.key, node.value, left, node.right), added)
            }
            Ordering::Greater => {
                let (right, added) = self.insert(node.right, key, value);
                (self.balanced(node.key, node.value, node.left, right), added)
            }
            Ordering::Equal => (self.new_node(node.key, value, node.left, node.right), false),
        }
    }

    /// The smallest entry of a non-empty tree, and the tree without it.
    fn take_min(&mut self, n: Node) -> (Term, Term, Term) {
        match self.node(n.left) {
            None => (n.key, n.value, n.right),
            Some(left) => {
                let (k, v, rest) = self.take_min(left);
                (k, v, self.balanced(n.key, n.value, rest, n.right))
            }
        }
    }

    /// The tree without `key`, or `None` if it has no such key.
    fn delete(&mut self, n: Term, key: Term) -> Option<Term> {
        let node = self.node(n)?;
        Some(match compare(self, key, self, node.key, true) {
            Ordering::Less => {
                let left = self.delete(node.left, key)?;
                self.balanced(node.key, node.value, left, node.right)
            }
            Ordering::Greater => {
                let right = self.delete(node.right, key)?;
                self.balanced(node.key, node.value, node.left, right)
            }
            Ordering::Equal => match (self.node(node.left), self.node(node.right)) {
                (None, _) => node.right,
                (_, None) => node.left,
                (Some(_), Some(right)) => {
                    // Replace with the successor: the smallest entry on the right.
                    let (k, v, rest) = self.take_min(right);
                    self.balanced(k, v, node.left, rest)
                }
            },
        })
    }

    fn map_parts(&self, m: Term) -> Option<(usize, Term)> {
        let Term::Map(p) = m else { return None };
        let c = self.object(p).1;
        let Term::Int(len) = c[0] else { unreachable!("a map size") };
        Some((len as usize, c[1]))
    }

    fn new_map(&mut self, len: usize, root: Term) -> Term {
        Term::Map(self.push_object(Kind::Map, &[Term::Int(len as i64), root]))
    }

    /// `#{}`.
    pub fn empty_map(&mut self) -> Term {
        self.new_map(0, Term::Nil)
    }

    /// The number of keys, if `m` is a map.
    pub fn map_len(&self, m: Term) -> Option<usize> {
        self.map_parts(m).map(|(len, _)| len)
    }

    /// The value of `key` (compared exactly), if `m` is a map that has it.
    pub fn map_get(&self, m: Term, key: Term) -> Option<Term> {
        let (_, mut cur) = self.map_parts(m)?;
        while let Some(n) = self.node(cur) {
            cur = match compare(self, key, self, n.key, true) {
                Ordering::Less => n.left,
                Ordering::Greater => n.right,
                Ordering::Equal => return Some(n.value),
            };
        }
        None
    }

    /// `m` with `key` set to `value`. `m` must be a map.
    pub fn map_put(&mut self, m: Term, key: Term, value: Term) -> Term {
        let (len, root) = self.map_parts(m).expect("a map");
        let (root, added) = self.insert(root, key, value);
        self.new_map(len + added as usize, root)
    }

    /// `m` without `key` (`m` itself if it has no such key). `m` must be a map.
    pub fn map_remove(&mut self, m: Term, key: Term) -> Term {
        let (len, root) = self.map_parts(m).expect("a map");
        match self.delete(root, key) {
            Some(root) => self.new_map(len - 1, root),
            None => m,
        }
    }

    /// A map of `pairs`; a later pair wins over an earlier one with an equal key.
    pub fn map_from(&mut self, pairs: impl IntoIterator<Item = (Term, Term)>) -> Term {
        let mut m = self.empty_map();
        let (mut len, mut root) = (0, Term::Nil);
        for (k, v) in pairs {
            let (r, added) = self.insert(root, k, v);
            root = r;
            len += added as usize;
        }
        if len > 0 {
            m = self.new_map(len, root);
        }
        m
    }

    /// The entries of a map in key order (`None` if `m` is not a map).
    pub fn map_entries(&self, m: Term) -> Option<Vec<(Term, Term)>> {
        let (len, root) = self.map_parts(m)?;
        let mut out = Vec::with_capacity(len);
        let mut stack = Vec::new();
        let mut cur = root;
        loop {
            while let Some(n) = self.node(cur) {
                stack.push(n);
                cur = n.left;
            }
            let Some(n) = stack.pop() else { break };
            out.push((n.key, n.value));
            cur = n.right;
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::Literals;
    use alloc::collections::BTreeMap;

    /// Check order, balance and cached heights of every node; return the height.
    fn check(h: &Heap, n: Term, lo: Option<i64>, hi: Option<i64>) -> u8 {
        let Some(node) = h.node(n) else { return 0 };
        let Term::Int(k) = node.key else { panic!("int keys") };
        assert!(lo.is_none_or(|lo| lo < k) && hi.is_none_or(|hi| k < hi), "order");
        let (hl, hr) = (check(h, node.left, lo, Some(k)), check(h, node.right, Some(k), hi));
        assert!((hl as i16 - hr as i16).abs() <= 1, "balance");
        assert_eq!(node.height, 1 + hl.max(hr), "height");
        node.height
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

    fn entries(h: &Heap, m: Term) -> Vec<(i64, i64)> {
        h.map_entries(m).unwrap().into_iter().map(|(k, v)| (k.as_i64().unwrap(), v.as_i64().unwrap())).collect()
    }

    /// Random puts and removes, compared step by step with `BTreeMap`, keeping old versions to
    /// check that changing one version never changes another.
    #[test]
    fn behaves_like_btreemap_and_versions_are_independent() {
        let mut h = Heap::new(&Literals::default());
        let mut rng = Rng(0x1234_5678_9abc_def1);
        let mut model = BTreeMap::new();
        let mut m = h.empty_map();
        let mut snapshots = Vec::new();
        for step in 0..20_000i64 {
            let k = (rng.next() % 500) as i64;
            if rng.next().is_multiple_of(3) {
                assert_eq!(h.map_get(m, Term::Int(k)).map(|v| v.as_i64().unwrap()), model.remove(&k));
                m = h.map_remove(m, Term::Int(k));
            } else {
                assert_eq!(h.map_get(m, Term::Int(k)).map(|v| v.as_i64().unwrap()), model.insert(k, step));
                m = h.map_put(m, Term::Int(k), Term::Int(step));
            }
            assert_eq!(h.map_len(m), Some(model.len()));
            if step % 1000 == 0 {
                check(&h, h.map_parts(m).unwrap().1, None, None);
                snapshots.push((m, model.clone()));
            }
        }
        assert_eq!(entries(&h, m), model.into_iter().collect::<Vec<_>>());
        for (snap, model) in snapshots {
            assert_eq!(entries(&h, snap), model.into_iter().collect::<Vec<_>>());
        }
    }

    #[test]
    fn sequential_keys_stay_balanced() {
        let mut h = Heap::new(&Literals::default());
        let m = h.map_from((0..100_000).map(|k| (Term::Int(k), Term::Nil)));
        let height = check(&h, h.map_parts(m).unwrap().1, None, None);
        assert!(height <= 25, "height {height} for 100000 keys");
        assert_eq!(h.map_len(m), Some(100_000));
    }

    #[test]
    fn exact_keys() {
        let mut h = Heap::new(&Literals::default());
        let m = h.map_from([(Term::Int(1), Term::Int(10)), (Term::Float(1.0), Term::Int(20))]);
        assert_eq!(h.map_len(m), Some(2));
        assert_eq!(h.map_get(m, Term::Int(1)).and_then(|v| v.as_i64()), Some(10));
        assert_eq!(h.map_get(m, Term::Float(1.0)).and_then(|v| v.as_i64()), Some(20));
    }
}
