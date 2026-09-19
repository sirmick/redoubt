//! Collecting a heap: Cheney's copying collection.
//!
//! The owner hands every term it holds to [`Collector::root`], then calls
//! [`Collector::finish`]. What the roots reach is copied to a fresh heap, in order, and scanned
//! there; an object's old header is overwritten with where it went (a list cell's head, which
//! can never be a header, likewise), so shared objects are copied once. Literal chunks are left
//! alone. Off-heap entries still referred to move to a fresh table; the rest are dropped with
//! the old one, which frees binaries that nothing holds any more.

use alloc::vec::Vec;

use super::{Header, Heap, Kind, OffHeap, Ptr, Term};

/// A collection in progress. Every term the heap's owner holds must go through
/// [`Collector::root`] before [`Collector::finish`]; any other term of this heap is invalid
/// after the collection.
pub struct Collector<'h> {
    heap: &'h mut Heap,
    from: Vec<Term>,
    from_offheap: Vec<OffHeap>,
    /// Where each old off-heap entry went (`u32::MAX`: not yet).
    offheap_moved: Vec<u32>,
}

impl Heap {
    /// Start collecting. `capacity` is a guess at the live size, in cells.
    pub fn collect(&mut self, capacity: usize) -> Collector<'_> {
        let from = core::mem::replace(&mut self.terms, Vec::with_capacity(capacity));
        let from_offheap = core::mem::take(&mut self.offheap);
        self.offheap_bytes = 0;
        let offheap_moved = alloc::vec![u32::MAX; from_offheap.len()];
        Collector {
            heap: self,
            from,
            from_offheap,
            offheap_moved,
        }
    }
}

impl Collector<'_> {
    /// A term the heap's owner holds: after this call it is its new self.
    pub fn root(&mut self, t: &mut Term) {
        *t = self.evacuate(*t);
    }

    /// Copy the object `t` points to (once), and return `t` pointing at the copy.
    fn evacuate(&mut self, mut t: Term) -> Term {
        let is_cons = matches!(t, Term::Cons(_));
        let Some(p) = t.ptr_mut() else { return t };
        if p.space != 0 {
            return t;
        }
        let at = p.at();
        if let Term::Header(Header {
            kind: Kind::Forward,
            len,
        }) = self.from[at]
        {
            p.index = len;
            return t;
        }
        let size = if is_cons {
            2
        } else {
            let Term::Header(h) = self.from[at] else {
                unreachable!("an object starts with a header")
            };
            1 + h.len as usize
        };
        let new = Ptr::own(self.heap.terms.len());
        self.heap.terms.extend_from_slice(&self.from[at..at + size]);
        self.from[at] = Term::Header(Header {
            kind: Kind::Forward,
            len: new.index,
        });
        p.index = new.index;
        t
    }

    /// Copy everything the roots reach, and drop the rest.
    pub fn finish(mut self) {
        let mut i = 0;
        while i < self.heap.terms.len() {
            let cell = self.heap.terms[i];
            self.heap.terms[i] = match cell {
                Term::Header(_) => cell,
                Term::OffHeap(j) => {
                    let j = j as usize;
                    if self.offheap_moved[j] == u32::MAX {
                        let Term::OffHeap(k) = self.heap.push_offheap(self.from_offheap[j].clone())
                        else {
                            unreachable!()
                        };
                        self.offheap_moved[j] = k;
                    }
                    Term::OffHeap(self.offheap_moved[j])
                }
                _ => self.evacuate(cell),
            };
            i += 1;
        }
    }
}
