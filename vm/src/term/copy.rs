//! Copying terms between heaps, and terms that own their heap.

use alloc::collections::BTreeMap;
use core::cmp::Ordering;
use core::fmt;

use super::{compare, Heap, Literals, Ptr, Term};

/// Copy `t`, a term of `src`, onto `dst`, and return it as a term of `dst`. Objects in `src`'s
/// own space are copied (each once, so sharing within `t` is kept); immediates and literals are
/// not. Cheney's algorithm: the copies are scanned in order, so depth costs no Rust stack.
pub fn copy(src: &Heap, t: Term, dst: &mut Heap) -> Term {
    dst.refresh(&src.lits);
    if t.ptr().is_none_or(|p| p.space != 0) {
        return t;
    }
    let start = dst.terms.len();
    let mut c = Copier {
        src,
        moved: BTreeMap::new(),
        offheap: BTreeMap::new(),
    };
    let root = c.evacuate(dst, t);
    let mut i = start;
    while i < dst.terms.len() {
        let cell = dst.terms[i];
        match cell {
            Term::Header(_) => {}
            Term::OffHeap(j) => {
                let k = *c.offheap.entry(j).or_insert_with(|| {
                    match dst.push_offheap(src.offheap[j as usize].clone()) {
                        Term::OffHeap(k) => k,
                        _ => unreachable!(),
                    }
                });
                dst.terms[i] = Term::OffHeap(k);
            }
            _ => dst.terms[i] = c.evacuate(dst, cell),
        }
        i += 1;
    }
    root
}

struct Copier<'s> {
    src: &'s Heap,
    /// Objects of `src` already copied: their index there and in the destination.
    moved: BTreeMap<u32, u32>,
    /// Off-heap entries already copied, likewise.
    offheap: BTreeMap<u32, u32>,
}

impl Copier<'_> {
    /// Copy the object `t` points to, if it is in `src`'s own space and not yet copied; its
    /// cells still refer to `src` until the scan reaches them.
    fn evacuate(&mut self, dst: &mut Heap, mut t: Term) -> Term {
        let Some(p) = t.ptr_mut() else { return t };
        if p.space != 0 {
            return t;
        }
        let new = match self.moved.get(&p.index) {
            Some(&new) => new,
            None => {
                let at = dst.terms.len();
                let from = p.at();
                let len = match self.src.terms[from] {
                    Term::Header(h) => 1 + h.len as usize,
                    _ => 2, // a list cell
                };
                dst.terms
                    .extend_from_slice(&self.src.terms[from..from + len]);
                let new = Ptr::own(at).index;
                self.moved.insert(p.index, new);
                new
            }
        };
        p.index = new;
        t
    }
}

/// A term with a heap of its own: what is kept outside any process (ETS objects, exit reasons,
/// monitor names, message timers, results).
#[derive(Clone)]
pub struct OwnedTerm {
    heap: Heap,
    root: Term,
}

impl OwnedTerm {
    /// A copy of `t`, a term of `src`.
    pub fn new(src: &Heap, t: Term) -> OwnedTerm {
        let mut heap = Heap::new(&src.lits);
        let root = copy(src, t, &mut heap);
        heap.terms.shrink_to_fit();
        OwnedTerm { heap, root }
    }

    /// A term built by `f` on a heap of its own.
    pub fn build(lits: &Literals, f: impl FnOnce(&mut Heap) -> Term) -> OwnedTerm {
        let mut heap = Heap::new(lits);
        let root = f(&mut heap);
        OwnedTerm { heap, root }
    }

    /// A term that is an immediate or a literal (so needs no heap).
    pub fn immediate(t: Term) -> OwnedTerm {
        debug_assert!(
            t.ptr().is_none_or(|p| p.space != 0),
            "a term with objects of its own"
        );
        OwnedTerm {
            heap: Heap::new(&Literals::default()),
            root: t,
        }
    }

    pub fn term(&self) -> Term {
        self.root
    }

    pub fn heap(&self) -> &Heap {
        &self.heap
    }

    /// A copy of this term on `dst`.
    pub fn copy_into(&self, dst: &mut Heap) -> Term {
        copy(&self.heap, self.root, dst)
    }

    /// Memory in 8-byte words.
    pub fn words(&self) -> u64 {
        self.heap.words()
    }
}

impl PartialEq for OwnedTerm {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for OwnedTerm {}
impl PartialOrd for OwnedTerm {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
/// Exact term order.
impl Ord for OwnedTerm {
    fn cmp(&self, other: &Self) -> Ordering {
        compare(&self.heap, self.root, &other.heap, other.root, true)
    }
}

impl fmt::Display for OwnedTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.heap.show(self.root), f)
    }
}

impl fmt::Debug for OwnedTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Clone for Heap {
    fn clone(&self) -> Heap {
        Heap {
            terms: self.terms.clone(),
            offheap: self.offheap.clone(),
            offheap_index: self.offheap_index.clone(),
            offheap_bytes: self.offheap_bytes,
            lits: self.lits.clone(),
        }
    }
}
