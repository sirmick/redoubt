//! Every placement against the old search's answer, page by page: random
//! areas, the dense shapes the old search was measured at (kernel/memory.md, R22), and the reads
//! each search took against the bound.

extern crate std;

use std::vec;
use std::vec::Vec;

use super::*;

/// A small deterministic generator (xorshift), so a failure names its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
}

/// The old search over a plain list of free pages: every start from `first`, then from the
/// area's start, each tried by looking at every page of its run.
fn reference(free: &[bool], last: usize, len: usize) -> Option<usize> {
    let last_start = free.len().checked_sub(len).filter(|_| len > 0)?;
    let first = last.min(last_start);
    let fits = |s: usize| free[s..s + len].iter().all(|f| *f);
    (first..=last_start).chain(0..first).find(|s| fits(*s))
}

/// A store in vectors.
struct Vecs {
    bits: Vec<u64>,
    groups: Vec<Group>,
    pending: Option<usize>,
}

impl Store for Vecs {
    fn words(&self) -> usize { self.bits.len() }

    fn word(&self, index: usize) -> u64 { self.bits[index] }

    fn set_word(&mut self, index: usize, word: u64) { self.bits[index] = word }

    fn group(&self, index: usize) -> Group { self.groups[index] }

    fn set_group(&mut self, index: usize, group: Group) { self.groups[index] = group }

    fn pending(&self) -> Option<usize> { self.pending }

    fn set_pending(&mut self, group: Option<usize>) { self.pending = group }
}

/// An area of `groups` groups, and which of its pages are free, kept apart for the reference.
struct Owned {
    area: Area<Vecs>,
    free: Vec<bool>,
}

impl Owned {
    fn new(groups: usize) -> Self {
        let words = groups * GROUP_WORDS;
        // Not yet what the words say: `clear` makes it so.
        let store = Vecs { bits: vec![0; words], groups: vec![Group::default(); groups], pending: None };
        let mut area = Area::new(store);
        area.clear();
        Owned { area, free: vec![true; groups * GROUP_PAGES] }
    }

    fn set(&mut self, page: usize, free: bool) {
        self.area.set(page, free);
        self.free[page] = free;
        assert_eq!(self.area.is_free(page), free);
        assert_eq!(self.area.free_pages(), self.free.iter().filter(|f| **f).count());
    }

    /// Place, check it against the reference and the bound, and return it.
    fn place(&mut self, last: usize, len: usize) -> Option<usize> {
        assert!(self.area.records_hold(), "a record not pending is stale");
        let mut reads = 0;
        let found = self.area.place(last, len, &mut reads);
        assert!(
            self.area.records_hold() && self.area.store.pending.is_none(),
            "a search left a record stale"
        );
        assert_eq!(found, reference(&self.free, last, len), "placing {len} from {last}");
        let most = self.area.most_reads();
        assert!(reads <= most, "{reads} reads placing {len} from {last}, over {most}");
        found
    }
}

#[test]
fn the_longest_run_in_a_word() {
    for x in [0u64, 1, 0b1011_0111, u64::MAX >> 1, u64::MAX << 1, 0x00ff_00ff_00ff_00ff, 1 << 63] {
        let mut best = 0;
        let mut run = 0;
        for bit in 0..64 {
            run = if x >> bit & 1 == 1 { run + 1 } else { 0 };
            best = best.max(run);
        }
        assert_eq!(longest(x), best, "{x:#x}");
    }
}

/// Random areas, sparse to dense, every length from a page to past the area, from random last
/// placements: the answer is the old search's, and the reads are under the bound.
#[test]
fn placement_is_the_old_searchs() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..400 {
        let mut a = Owned::new(1 + rng.below(4));
        let pages = a.free.len();
        let density = 1 + rng.below(64);
        for _ in 0..pages * density / 64 {
            let page = rng.below(pages);
            a.set(page, false);
        }
        // Free some again, so records also grow.
        for _ in 0..rng.below(pages / 4) {
            let page = rng.below(pages);
            a.set(page, true);
        }
        for _ in 0..40 {
            let len = match rng.below(4) {
                0 => 1 + rng.below(8),
                1 => 1 + rng.below(WORD * 2),
                2 => 1 + rng.below(GROUP_PAGES * 2),
                _ => 1 + rng.below(pages + 2),
            };
            let last = rng.below(pages + 4);
            let _ = a.place(last, len);
        }
    }
}

/// The old search's measured shapes: a page taken at the end of every n-page window of
/// `map_anon`'s 65,536-page area, and a refused request of n pages; then one run near the area's
/// end, found from below it and by the wrap. The reads stay under the bound however many starts
/// the old search tried.
#[test]
fn dense_shapes() {
    for n in [2, 4, 8, 16] {
        let mut a = Owned::new(128);
        let pages = a.free.len();
        for window in (0..pages).step_by(n) {
            a.set(window + n - 1, false);
        }
        assert_eq!(a.place(0, n), None);
        assert_eq!(a.place(pages / 2 + 3, n), None);
        // One run near the area's end: the last window but one, its pin freed.
        a.set(pages - n - 1, true);
        assert_eq!(a.place(0, n), Some(pages - 2 * n));
        // And one below a last placement past it, found by the wrap.
        a.set(pages / 2 - 1, true);
        assert_eq!(a.place(pages / 2 + 100, n), Some(pages - 2 * n));
        a.set(pages - n - 1, false);
        assert_eq!(a.place(pages / 2 + 100, n), Some(pages / 2 - n));
    }
}

/// The edges: a run that is the whole area, one that fits only at its very end, a placement
/// past the area, and a zero length.
#[test]
fn the_area_s_edges() {
    let mut a = Owned::new(2);
    let pages = a.free.len();
    assert_eq!(a.place(0, pages), Some(0));
    assert_eq!(a.place(pages + 9, pages), Some(0));
    assert_eq!(a.place(0, pages + 1), None);
    assert_eq!(a.place(0, 0), None);
    a.set(0, false);
    assert_eq!(a.place(0, pages - 1), Some(1));
    assert_eq!(a.place(0, pages), None);
    a.set(pages - 1, false);
    assert_eq!(a.place(5, pages - 2), Some(1));
    // A record that goes from full to taken and back.
    a.set(700, false);
    a.set(700, true);
    assert_eq!(a.place(0, pages - 2), Some(1));
}
