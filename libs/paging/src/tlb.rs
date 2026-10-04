//! What a page-table write leaves to flush, and what each `sfence.vma` flushes: the rules the
//! kernel's checked build audits (kernel/memory-layout.md, "`satp`"). From the privileged spec's
//! `SFENCE.VMA`: a flush with an address covers that page's leaf translations, and one with
//! `rs1 = x0` covers every page and every cached table pointer; a flush with an ASID covers that
//! ASID's translations and never a global one, and one with `rs2 = x0` covers every ASID's and the
//! global ones.

use core::fmt;

use redoubt_sys::PAGE_SIZE;

/// Translations a page-table write may have left stale in this hart's TLB.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Stale {
    /// `Some(asid)`: that address space's own entries. `None`: global entries, the kernel half's
    /// shared leaves, which every address space uses.
    pub asid: Option<usize>,
    /// `Some(page)`: the leaf for the page at that address. `None`: any page, because a table
    /// pointer changed or the ASID was given out.
    pub page: Option<usize>,
}

impl Stale {
    /// The narrowest flush that covers it.
    pub fn flush(self) -> Flush {
        match (self.asid, self.page) {
            (Some(asid), Some(page)) => Flush::Page { page, asid },
            (Some(asid), None) => Flush::Asid(asid),
            (None, Some(page)) => Flush::Global(page),
            (None, None) => Flush::All,
        }
    }
}

impl fmt::Display for Stale {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.asid {
            Some(asid) => write!(f, "ASID {asid}, ")?,
            None => write!(f, "global, ")?,
        }
        match self.page {
            Some(page) => write!(f, "page {page:#x}"),
            None => write!(f, "every page"),
        }
    }
}

/// One `sfence.vma`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Flush {
    /// `sfence.vma x0, x0`: everything, global entries included.
    All,
    /// `sfence.vma x0, asid`: every entry of one ASID, table pointers included; no global one.
    Asid(usize),
    /// `sfence.vma page, asid`: one page's leaf in one ASID; no global one.
    Page { page: usize, asid: usize },
    /// `sfence.vma page, x0`: one page's leaf in every ASID, global ones included.
    Global(usize),
}

fn same_page(a: usize, b: usize) -> bool { a / PAGE_SIZE == b / PAGE_SIZE }

impl Flush {
    /// Whether this flush drops every translation `stale` stands for.
    pub fn covers(self, stale: Stale) -> bool {
        match self {
            Flush::All => true,
            Flush::Asid(asid) => stale.asid == Some(asid),
            Flush::Page { page, asid } => {
                stale.asid == Some(asid) && stale.page.is_some_and(|p| same_page(p, page))
            }
            Flush::Global(page) => stale.page.is_some_and(|p| same_page(p, page)),
        }
    }
}

/// The writes a hart has made and not flushed yet, oldest first, at most `N`: the checked
/// build's log, which must be empty whenever the kernel returns to user mode.
pub struct Unflushed<const N: usize> {
    pending: [Option<Stale>; N],
    len: usize,
}

impl<const N: usize> Default for Unflushed<N> {
    fn default() -> Self { Self::new() }
}

impl<const N: usize> Unflushed<N> {
    pub const fn new() -> Self { Unflushed { pending: [None; N], len: 0 } }

    /// Record a write. One already recorded is not recorded twice. `Err` with the oldest record
    /// if the log is full: more writes are waiting than any flush rule leaves, which is itself
    /// the fault the audit looks for.
    pub fn wrote(&mut self, stale: Stale) -> Result<(), Stale> {
        if self.pending[..self.len].contains(&Some(stale)) {
            return Ok(());
        }
        if self.len == N {
            return Err(self.pending[0].expect("a full log has a first record"));
        }
        self.pending[self.len] = Some(stale);
        self.len += 1;
        Ok(())
    }

    /// Drop every record `flush` covers, keeping the rest in order.
    pub fn flushed(&mut self, flush: Flush) {
        let mut kept = 0;
        for i in 0..self.len {
            let stale = self.pending[i].expect("records below len");
            if !flush.covers(stale) {
                self.pending[kept] = Some(stale);
                kept += 1;
            }
        }
        self.pending[kept..self.len].fill(None);
        self.len = kept;
    }

    /// The oldest write not flushed yet.
    pub fn first(&self) -> Option<Stale> { self.pending[0] }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HERE: usize = 0x4000_1000;
    const THERE: usize = 0x4000_2000;

    fn leaf(asid: usize, page: usize) -> Stale { Stale { asid: Some(asid), page: Some(page) } }
    fn whole(asid: usize) -> Stale { Stale { asid: Some(asid), page: None } }
    fn global(page: usize) -> Stale { Stale { asid: None, page: Some(page) } }
    const GLOBAL_TABLE: Stale = Stale { asid: None, page: None };

    /// Each flush kind against each record kind: (flush, record, covered).
    #[test]
    fn each_flush_covers_what_the_spec_says() {
        let cases = [
            // A whole flush covers everything.
            (Flush::All, leaf(5, HERE), true),
            (Flush::All, whole(5), true),
            (Flush::All, global(HERE), true),
            (Flush::All, GLOBAL_TABLE, true),
            // An ASID flush covers that ASID, its table pointers included, and nothing global.
            (Flush::Asid(5), leaf(5, HERE), true),
            (Flush::Asid(5), whole(5), true),
            (Flush::Asid(5), leaf(6, HERE), false),
            (Flush::Asid(5), whole(6), false),
            (Flush::Asid(5), global(HERE), false),
            (Flush::Asid(5), GLOBAL_TABLE, false),
            // A page in one ASID: that pair alone, at any offset in the page.
            (Flush::Page { page: HERE, asid: 5 }, leaf(5, HERE), true),
            (Flush::Page { page: HERE + 0x80, asid: 5 }, leaf(5, HERE), true),
            (Flush::Page { page: HERE, asid: 5 }, leaf(5, THERE), false),
            (Flush::Page { page: HERE, asid: 5 }, leaf(6, HERE), false),
            (Flush::Page { page: HERE, asid: 5 }, whole(5), false),
            (Flush::Page { page: HERE, asid: 5 }, global(HERE), false),
            (Flush::Page { page: HERE, asid: 5 }, GLOBAL_TABLE, false),
            // A page in every ASID: that page wherever it is, global included; no table pointer.
            (Flush::Global(HERE), leaf(5, HERE), true),
            (Flush::Global(HERE), leaf(6, HERE), true),
            (Flush::Global(HERE), global(HERE), true),
            (Flush::Global(HERE), leaf(5, THERE), false),
            (Flush::Global(HERE), global(THERE), false),
            (Flush::Global(HERE), whole(5), false),
            (Flush::Global(HERE), GLOBAL_TABLE, false),
        ];
        for (flush, stale, covered) in cases {
            assert_eq!(flush.covers(stale), covered, "{flush:?} against {stale}");
        }
    }

    #[test]
    fn each_record_names_the_narrowest_flush_that_covers_it() {
        let cases = [
            (leaf(5, HERE), Flush::Page { page: HERE, asid: 5 }),
            (whole(5), Flush::Asid(5)),
            (global(HERE), Flush::Global(HERE)),
            (GLOBAL_TABLE, Flush::All),
        ];
        for (stale, flush) in cases {
            assert_eq!(stale.flush(), flush, "{stale}");
            assert!(stale.flush().covers(stale), "{stale}");
        }
    }

    #[test]
    fn a_flush_empties_what_it_covers_and_keeps_the_rest_in_order() {
        let mut log = Unflushed::<4>::new();
        log.wrote(leaf(5, HERE)).unwrap();
        log.wrote(whole(6)).unwrap();
        log.wrote(global(THERE)).unwrap();
        log.flushed(Flush::Asid(5));
        assert_eq!(log.first(), Some(whole(6)));
        log.flushed(Flush::Page { page: HERE, asid: 6 });
        assert_eq!(log.first(), Some(whole(6)), "a page flush leaves a whole-ASID record");
        log.flushed(Flush::Asid(6));
        assert_eq!(log.first(), Some(global(THERE)), "an ASID flush leaves a global record");
        log.flushed(Flush::Global(THERE));
        assert_eq!(log.first(), None);
    }

    #[test]
    fn a_full_log_names_its_oldest_record() {
        let mut log = Unflushed::<2>::new();
        log.wrote(leaf(5, HERE)).unwrap();
        log.wrote(leaf(5, HERE)).unwrap();
        log.wrote(leaf(5, THERE)).unwrap();
        assert_eq!(log.wrote(whole(5)), Err(leaf(5, HERE)));
        log.flushed(Flush::All);
        assert_eq!(log.first(), None);
        log.wrote(whole(5)).unwrap();
        assert_eq!(log.first(), Some(whole(5)));
    }

    #[test]
    fn records_print_what_they_name() {
        extern crate std;
        use std::string::ToString;
        assert_eq!(leaf(5, HERE).to_string(), "ASID 5, page 0x40001000");
        assert_eq!(whole(5).to_string(), "ASID 5, every page");
        assert_eq!(global(HERE).to_string(), "global, page 0x40001000");
    }
}
