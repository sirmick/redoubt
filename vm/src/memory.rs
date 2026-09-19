//! Memory accounting: how much a process (or an ETS table) holds, in BEAM's units.
//!
//! Terms are reference counted, not kept on per-process heaps, so memory is measured rather than
//! read off an allocator: walk everything reachable from the process's roots and add up what
//! BEAM would need for it (64-bit words, as `erts_debug:flat_size/1` counts them). A node that
//! is referenced more than once is counted once, so a term built with sharing (a DAG) costs what
//! it really costs, not the size of its flattened copy. Binaries above 64 bytes live outside the
//! process heap, as BEAM's reference-counted binaries do; their bytes are reported separately,
//! each buffer once.
//!
//! The walk stops as soon as the total passes a budget, so measuring a process that is over its
//! limit costs no more than the limit.

use alloc::collections::BTreeSet;
use alloc::rc::Rc;
use alloc::vec::Vec;

use crate::process::Process;
use crate::term::{Fun, Term};

/// Binaries larger than this many bytes are kept off-heap (BEAM's `ERL_ONHEAP_BIN_LIMIT`).
const ONHEAP_BINARY_BYTES: usize = 64;

/// What a measurement found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Heap words, counting each shared node once.
    pub words: u64,
    /// Bytes of off-heap binaries, counting each buffer once.
    pub binary_bytes: u64,
}

impl Usage {
    /// Everything, in words: what resource limits compare against.
    pub fn total_words(&self) -> u64 {
        self.words.saturating_add(self.binary_bytes.div_ceil(8))
    }
}

/// A walk over terms that remembers the shared nodes it has already counted.
pub struct Meter<'a> {
    usage: Usage,
    budget: u64,
    work: Vec<&'a Term>,
    /// Addresses of nodes with more than one reference that have been counted. Nodes with a
    /// single reference can only be reached once (through their one parent, itself counted
    /// once), so they need no entry: the set stays small for tree-shaped data.
    seen: BTreeSet<usize>,
    buffers: BTreeSet<usize>,
}

/// `true` the first time a shared node is met; always `true` for unshared ones.
fn first_visit<T: ?Sized>(seen: &mut BTreeSet<usize>, rc: &Rc<T>) -> bool {
    Rc::strong_count(rc) == 1 || seen.insert(Rc::as_ptr(rc) as *const () as usize)
}

impl<'a> Meter<'a> {
    /// A meter that stops counting once the total passes `budget` words.
    pub fn new(budget: u64) -> Meter<'a> {
        Meter { usage: Usage::default(), budget, work: Vec::new(), seen: BTreeSet::new(), buffers: BTreeSet::new() }
    }

    /// Whether the budget has been passed; once it has, further terms are not walked.
    pub fn over(&self) -> bool {
        self.usage.total_words() > self.budget
    }

    pub fn usage(&self) -> Usage {
        self.usage
    }

    /// Count `t` and everything reachable from it that has not been counted yet.
    pub fn add(&mut self, t: &'a Term) {
        self.work.push(t);
        while let Some(t) = self.work.pop() {
            if self.over() {
                self.work.clear();
                return;
            }
            self.usage.words += self.node(t);
        }
    }

    /// The words `t` itself takes (not its children, which are queued).
    fn node(&mut self, t: &'a Term) -> u64 {
        let seen = &mut self.seen;
        match t {
            Term::Int(_) | Term::Atom(_) | Term::Nil | Term::Pid(_) => 0,
            Term::Float(_) => 2,
            Term::Big(b) if first_visit(seen, b) => 1 + b.bits().div_ceil(64),
            Term::Cons(c) if first_visit(seen, c) => {
                self.work.push(&c.head);
                self.work.push(&c.tail);
                2
            }
            Term::Tuple(e) if first_visit(seen, e) => {
                self.work.extend(e.iter());
                if e.is_empty() { 0 } else { 1 + e.len() as u64 }
            }
            Term::Map(m) if first_visit(seen, m) => {
                for (k, v) in m.iter() {
                    self.work.push(&k.0);
                    self.work.push(v);
                }
                let n = m.len() as u64;
                3 + n + if n > 0 { 1 + n } else { 0 }
            }
            Term::Bits(b) if first_visit(seen, b) => {
                if !offheap(b) {
                    2 + (b.len.div_ceil(8) as u64).div_ceil(8)
                } else {
                    if first_visit(&mut self.buffers, &b.data) {
                        self.usage.binary_bytes += b.data.len() as u64;
                    }
                    8
                }
            }
            Term::Match(m) if first_visit(seen, m) => {
                if offheap(&m.bits) && first_visit(&mut self.buffers, &m.bits.data) {
                    self.usage.binary_bytes += m.bits.data.len() as u64;
                }
                5
            }
            Term::Fun(f) if first_visit(seen, f) => match &**f {
                Fun::Export { .. } => 2,
                Fun::Local { env, .. } => {
                    self.work.extend(env.iter());
                    2 + env.len() as u64
                }
            },
            Term::Ref(_) => 3,
            Term::Resource(r) if first_visit(seen, r) => 3,
            // A shared node met again.
            _ => 0,
        }
    }
}

fn offheap(b: &crate::term::Bits) -> bool {
    b.len.div_ceil(8) > ONHEAP_BINARY_BYTES
}

/// Everything process `p` holds: registers, stack, mailbox, dictionary, and a pending exit
/// reason. Stops early once past `budget` words.
pub fn process(p: &Process, budget: u64) -> Usage {
    let mut m = Meter::new(budget);
    // The fixed cost of a process, as BEAM's `process_info(P, memory)` includes it.
    m.usage.words = PROCESS_WORDS + p.x.len() as u64 + p.stack.len() as u64 + p.frames.len() as u64;
    for t in p.x.iter().chain(&p.stack).chain(&p.mailbox).chain(p.pending_exit.iter()) {
        m.add(t);
    }
    for (k, v) in &p.dictionary {
        m.usage.words += 3; // the `{K, V}` tuple BEAM stores each entry as
        m.add(&k.0);
        m.add(v);
    }
    m.usage()
}

/// A process's own structures (BEAM's process struct and minimum heap), in words.
pub const PROCESS_WORDS: u64 = 338;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn measure(t: &Term) -> Usage {
        let mut m = Meter::new(u64::MAX);
        m.add(t);
        m.usage()
    }

    #[test]
    fn trees_cost_their_flat_size() {
        // [1, 2.0, {a}] = 3 cons cells, a float and a 1-tuple.
        let t = Term::list(vec![Term::Int(1), Term::Float(2.0), Term::tuple(vec![Term::Nil])]);
        assert_eq!(measure(&t).words, 3 * 2 + 2 + 2);
    }

    #[test]
    fn shared_nodes_count_once() {
        // Each level is a pair of the level below: flat size 2^61, real size 3 words a level.
        let mut t = Term::list(vec![Term::Int(0)]);
        for _ in 0..60 {
            t = Term::tuple(vec![t.clone(), t]);
        }
        assert_eq!(measure(&t).words, 60 * 3 + 2);
    }

    #[test]
    fn large_binaries_are_off_heap_and_counted_once() {
        let b = Term::binary(&[7u8; 1000]);
        let t = Term::tuple(vec![b.clone(), b.clone(), b]);
        let u = measure(&t);
        assert_eq!(u, Usage { words: 4 + 8, binary_bytes: 1000 });
    }

    #[test]
    fn the_budget_stops_the_walk() {
        let t = Term::list((0..10_000).map(Term::Int).collect::<Vec<_>>());
        let mut m = Meter::new(100);
        m.add(&t);
        assert!(m.over());
        assert!(m.usage().words < 110);
    }
}
