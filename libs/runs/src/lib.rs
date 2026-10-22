//! A placement area's free runs (kernel/memory.md, R22): where `map_anon`'s search looks.
//!
//! An area is a bit a page, set while the page's entry is empty (free), and a record a group of
//! [`GROUP_PAGES`] pages: the free run at the group's start, the one at its end, and its longest.
//! The kernel keeps both in step with the page tables: each entry that becomes empty or stops being
//! empty is one [`Area::set`]. That flips the page's bit and leaves its group's record pending; a
//! record is rebuilt from its group's words when a change moves to another group, or a search
//! settles it first. A range call's pages are contiguous, so they fall in at most two groups, and
//! its changes rebuild at most three records, not one a page.
//!
//! [`Area::place`] finds what the kernel's search always found: the first free run of the length
//! asked, from the start it placed last up to the last start that fits, then from the area's start.
//! It reads the records, and a group's words only where the run begins: the group it starts in,
//! and the one group whose record says the run fits inside it. So a search reads at most
//! [`Area::most_reads`] words and records, the settle's included, a constant of the area's size
//! however its pages are taken.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
mod tests;

/// The pages a group record covers: a leaf page table's span on Sv39.
pub const GROUP_PAGES: usize = 512;

/// The pages a word of bits covers, page 0 in bit 0.
const WORD: usize = 64;

/// The words of a group.
const GROUP_WORDS: usize = GROUP_PAGES / WORD;

/// The bits where a run of `len` set bits starts inside `x`, `len` at most a word: each step
/// doubles the run every set bit is known to start, at most seven steps.
fn starts(x: u64, len: usize) -> u64 {
    let (mut y, mut have) = (x, 1);
    while have < len {
        let by = have.min(len - have);
        y &= y >> by;
        have += by;
    }
    y
}

/// The longest run of set bits in `x`, not all set: doubled while one is that long, then halved
/// back by the steps between, a few word operations either way.
fn longest(x: u64) -> usize {
    if x == 0 {
        return 0;
    }
    let (mut y, mut have) = (x, 1);
    loop {
        let z = y & (y >> have);
        if z == 0 {
            break;
        }
        y = z;
        have *= 2;
    }
    let mut step = have / 2;
    while step > 0 {
        let z = y & (y >> step);
        if z != 0 {
            y = z;
            have += step;
        }
        step /= 2;
    }
    have
}

/// A group's free runs: from its start, at its end, and the longest anywhere in it, in pages.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Group {
    pub head: u16,
    pub tail: u16,
    pub longest: u16,
}

impl Group {
    /// The record as one word, as a store keeps it.
    pub fn encode(self) -> u64 {
        u64::from(self.head) | u64::from(self.tail) << 16 | u64::from(self.longest) << 32
    }

    pub fn decode(word: u64) -> Group {
        Group { head: word as u16, tail: (word >> 16) as u16, longest: (word >> 32) as u16 }
    }
}

/// A free run being followed across words and groups: where it began and its pages so far.
struct Run {
    start: usize,
    pages: usize,
}

/// Where an area's bits and records are kept: the kernel's are in frames it reaches through the
/// physmap, the model's and the tests' in vectors.
pub trait Store {
    /// The area's words, `GROUP_PAGES / 64` a group.
    fn words(&self) -> usize;
    fn word(&self, index: usize) -> u64;
    fn set_word(&mut self, index: usize, word: u64);
    fn group(&self, index: usize) -> Group;
    fn set_group(&mut self, index: usize, group: Group);
    /// The group whose record waits to be rebuilt, if any.
    fn pending(&self) -> Option<usize>;
    fn set_pending(&mut self, group: Option<usize>);
}

/// One placement area over its store.
pub struct Area<S> {
    store: S,
}

impl<S: Store> Area<S> {
    /// The area over `store`, as it holds.
    pub fn new(store: S) -> Self {
        assert!(store.words() % GROUP_WORDS == 0, "an area is whole groups");
        Area { store }
    }

    /// The area's pages.
    pub fn pages(&self) -> usize { self.store.words() * WORD }

    fn groups(&self) -> usize { self.store.words() / GROUP_WORDS }

    /// The most words and records one [`Area::place`] reads: the pending group's words it settles,
    /// then two passes, each over every record, the words of the group it starts in and those of
    /// the group it finds the run in.
    pub fn most_reads(&self) -> usize { GROUP_WORDS + 2 * (self.groups() + 2 * GROUP_WORDS) }

    /// Every page free: a new space's area.
    pub fn clear(&mut self) {
        for index in 0..self.store.words() {
            self.store.set_word(index, u64::MAX);
        }
        let whole = GROUP_PAGES as u16;
        for index in 0..self.groups() {
            self.store.set_group(index, Group { head: whole, tail: whole, longest: whole });
        }
        self.store.set_pending(None);
    }

    /// How many of the area's pages are free: a count of its words' bits.
    pub fn free_pages(&self) -> usize {
        (0..self.store.words()).map(|index| self.store.word(index).count_ones() as usize).sum()
    }

    /// Whether `page` is free.
    pub fn is_free(&self, page: usize) -> bool { self.store.word(page / WORD) >> (page % WORD) & 1 == 1 }

    /// Mark `page` free or taken. Its group's record is left pending; the one pending before, if
    /// another group's, is rebuilt now: a group's words, a few operations each.
    pub fn set(&mut self, page: usize, free: bool) {
        let index = page / WORD;
        let word = self.store.word(index);
        let bit = 1 << (page % WORD);
        let new = if free { word | bit } else { word & !bit };
        if new != word {
            self.store.set_word(index, new);
            let group = page / GROUP_PAGES;
            if self.store.pending() != Some(group) {
                self.settle();
                self.store.set_pending(Some(group));
            }
        }
    }

    /// Rebuild the pending record, if any: every record is then what its words say. The words it
    /// read, for [`Area::place`]'s count.
    pub fn settle(&mut self) -> usize {
        let Some(group) = self.store.pending() else { return 0 };
        self.store.set_group(group, self.record(group));
        self.store.set_pending(None);
        GROUP_WORDS
    }

    /// What group `group`'s record must hold, from its words alone.
    pub fn record(&self, group: usize) -> Group {
        let (mut head, mut run, mut most) = (None, 0, 0);
        for index in group * GROUP_WORDS..(group + 1) * GROUP_WORDS {
            let x = self.store.word(index);
            if x == u64::MAX {
                run += WORD;
                continue;
            }
            run += x.trailing_ones() as usize;
            head.get_or_insert(run);
            most = most.max(run).max(longest(x));
            run = x.leading_ones() as usize;
        }
        let page = |n: usize| n as u16;
        Group { head: page(head.unwrap_or(GROUP_PAGES)), tail: page(run), longest: page(most.max(run)) }
    }

    /// Whether every record but the pending one is what its words say: the checked build's audit,
    /// and the tests'.
    pub fn records_hold(&self) -> bool {
        (0..self.groups()).all(|g| self.store.pending() == Some(g) || self.store.group(g) == self.record(g))
    }

    /// The first run of `len` free pages, where the kernel's search finds it: from `last`, the
    /// start placed last (clamped to the last start that fits), up to the area's end, then from
    /// the area's start. `None` if no run fits, or `len` is zero or longer than the area. The
    /// pending record is settled first. `reads` counts the words and records read, never more than
    /// [`Area::most_reads`].
    pub fn place(&mut self, last: usize, len: usize, reads: &mut usize) -> Option<usize> {
        *reads += self.settle();
        let last_start = self.pages().checked_sub(len).filter(|_| len > 0)?;
        let first = last.min(last_start);
        // A run found from the start lies below `first`: one at or past it the first pass found.
        self.first_fit(first, len, reads).or_else(|| self.first_fit(0, len, reads))
    }

    /// The first run of `len` free pages starting at or after `from`.
    fn first_fit(&self, from: usize, len: usize, reads: &mut usize) -> Option<usize> {
        let mut run = Run { start: from, pages: 0 };
        let mut group = from / GROUP_PAGES;
        if from % GROUP_PAGES != 0 {
            // The group it starts in, from `from`, by its words.
            if let Some(at) = self.scan(from, len, &mut run, reads) {
                return Some(at);
            }
            group += 1;
        }
        while group < self.groups() {
            let record = self.store.group(group);
            *reads += 1;
            let at = group * GROUP_PAGES;
            if run.pages == 0 {
                run.start = at;
            }
            let (head, tail) = (usize::from(record.head), usize::from(record.tail));
            if run.pages + head >= len {
                return Some(run.start);
            }
            if head == GROUP_PAGES {
                run.pages += GROUP_PAGES;
            } else if usize::from(record.longest) >= len {
                // Not with the run before it, so inside: the record says one fits.
                let mut inside = Run { start: at, pages: 0 };
                return self.scan(at, len, &mut inside, reads);
            } else {
                run = Run { start: at + GROUP_PAGES - tail, pages: tail };
            }
            group += 1;
        }
        None
    }

    /// Follow `run` through the words from page `from` to its group's end: the start of the first
    /// run of `len` found, or `None` with `run` the free run reaching the group's end.
    fn scan(&self, from: usize, len: usize, run: &mut Run, reads: &mut usize) -> Option<usize> {
        let end = (from / GROUP_PAGES + 1) * GROUP_PAGES;
        let mut word = from / WORD;
        // The pages below `from` in its word count as taken.
        let mut x = self.store.word(word) & (u64::MAX << (from % WORD));
        loop {
            *reads += 1;
            let at = word * WORD;
            if run.pages == 0 {
                run.start = at;
            }
            if run.pages + x.trailing_ones() as usize >= len {
                return Some(run.start);
            }
            if x == u64::MAX {
                run.pages += WORD;
            } else {
                if len <= WORD {
                    let inside = starts(x, len);
                    if inside != 0 {
                        return Some(at + inside.trailing_zeros() as usize);
                    }
                }
                let tail = x.leading_ones() as usize;
                *run = Run { start: at + WORD - tail, pages: tail };
            }
            word += 1;
            if word * WORD >= end {
                return None;
            }
            x = self.store.word(word);
        }
    }
}
