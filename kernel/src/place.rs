// SPDX-License-Identifier: MIT OR Apache-2.0

//! The placement areas' free runs (kernel/memory.md, "Where `map_anon` puts pages"; R22): what
//! `find_virtual_address` searches instead of the page tables.
//!
//! Each address space keeps, for each of its two placement areas, a bit a page (set while the
//! page's entry is empty) and a record a group of `GROUP_PAGES` pages (`redoubt-runs`). `map_anon`'s
//! area, 65,536 pages, has its bits in two frames of the process's own, mapped without `U` beside
//! its header in `PROCESS_AREA` so that the space's end frees them with the rest; its records, and
//! the message area's bits and records, are words of the header page. Every leaf write that turns
//! an entry of an area empty or not empty goes through [`changed`] (arch/riscv/mem.rs,
//! `Space::write_leaf`), so the bits are the tables' and a search costs a constant.

use redoubt_runs::{Area, GROUP_PAGES, Group, Store};
use redoubt_sys::PAGE_SIZE;

use crate::kframe;
use crate::mem::{DEFAULT_BASE, DEFAULT_MESSAGE_BASE, MemoryType};

/// `map_anon`'s area: 256 MiB from `DEFAULT_BASE`.
const DEFAULT_PAGES: usize = 0x1000_0000 / PAGE_SIZE;
/// The message area: one superpage from `DEFAULT_MESSAGE_BASE`.
const MESSAGE_PAGES: usize = 0x40_0000 / PAGE_SIZE;

/// How many frames hold `map_anon`'s bits, a bit a page.
pub const FRAMES_TAKEN: usize = DEFAULT_PAGES / 8 / PAGE_SIZE;

/// The header's words (`ProcessImpl::runs`): the frames of `map_anon`'s bits, each area's pending
/// group (one more than its index, 0 for none), `map_anon`'s records, then the message area's bits
/// and records.
const FRAMES: usize = 0;
const DEFAULT_PENDING: usize = FRAMES + FRAMES_TAKEN;
const MESSAGE_PENDING: usize = DEFAULT_PENDING + 1;
const DEFAULT_RECORDS: usize = MESSAGE_PENDING + 1;
const MESSAGE_BITS: usize = DEFAULT_RECORDS + DEFAULT_PAGES / GROUP_PAGES;
const MESSAGE_RECORDS: usize = MESSAGE_BITS + MESSAGE_PAGES / 64;
pub const HEADER_WORDS: usize = MESSAGE_RECORDS + MESSAGE_PAGES / GROUP_PAGES;

/// The placement areas: which, where each starts and how many pages it has.
pub const AREAS: [(MemoryType, usize, usize); 2] = [
    (MemoryType::Default, DEFAULT_BASE, DEFAULT_PAGES),
    (MemoryType::Messages, DEFAULT_MESSAGE_BASE, MESSAGE_PAGES),
];

/// The byte offset of word `index` of the runs in a header page.
fn header_word(index: usize) -> usize { crate::arch::process::RUNS_OFFSET + index * 8 }

/// One area's runs in a space: its bits from byte `bits` on through `frames`, its records and its
/// pending group in the header.
struct Runs {
    header: usize,
    frames: [usize; FRAMES_TAKEN],
    bits: usize,
    words: usize,
    records: usize,
    pending: usize,
}

impl Runs {
    /// The frame and offset of bit word `index`.
    fn at(&self, index: usize) -> (usize, usize) {
        let byte = self.bits + index * 8;
        (self.frames[byte / PAGE_SIZE], byte % PAGE_SIZE)
    }
}

impl Store for Runs {
    fn words(&self) -> usize { self.words }

    fn word(&self, index: usize) -> u64 {
        let (frame, offset) = self.at(index);
        kframe::read(frame, offset)
    }

    fn set_word(&mut self, index: usize, word: u64) {
        let (frame, offset) = self.at(index);
        kframe::write(frame, offset, word)
    }

    fn group(&self, index: usize) -> Group {
        Group::decode(kframe::read(self.header, header_word(self.records + index)))
    }

    fn set_group(&mut self, index: usize, group: Group) {
        kframe::write(self.header, header_word(self.records + index), group.encode())
    }

    fn pending(&self) -> Option<usize> {
        (kframe::read(self.header, header_word(self.pending)) as usize).checked_sub(1)
    }

    fn set_pending(&mut self, group: Option<usize>) {
        kframe::write(self.header, header_word(self.pending), group.map_or(0, |g| g as u64 + 1))
    }
}

/// Area `kind`'s runs in the space whose header is at `header`, once it has them: a space the
/// loader built has none until the boot gives them ([`set_up`]), and nothing writes its areas
/// before then.
fn area(header: usize, kind: MemoryType) -> Option<Area<Runs>> {
    let frames: [usize; FRAMES_TAKEN] =
        core::array::from_fn(|n| kframe::read(header, header_word(FRAMES + n)) as usize);
    if frames[0] == 0 {
        return None;
    }
    let runs = match kind {
        MemoryType::Default => Runs {
            header,
            frames,
            bits: 0,
            words: DEFAULT_PAGES / 64,
            records: DEFAULT_RECORDS,
            pending: DEFAULT_PENDING,
        },
        MemoryType::Messages => Runs {
            header,
            frames: [header; FRAMES_TAKEN],
            bits: header_word(MESSAGE_BITS),
            words: MESSAGE_PAGES / 64,
            records: MESSAGE_RECORDS,
            pending: MESSAGE_PENDING,
        },
    };
    Some(Area::new(runs))
}

/// The area `virt` lies in and its page there, if it lies in one: what a leaf write asks before it
/// looks for the space's header.
pub fn page_of(virt: usize) -> Option<(MemoryType, usize)> {
    AREAS.iter().find_map(|(kind, base, pages)| {
        let page = virt.checked_sub(*base)? / PAGE_SIZE;
        (page < *pages).then_some((*kind, page))
    })
}

/// The entry for page `page` of area `kind`, in the space whose header is at `header`, became
/// empty (`free`) or stopped being empty: its bit, and its group's record.
pub fn changed(header: usize, (kind, page): (MemoryType, usize), free: bool) {
    if let Some(mut area) = area(header, kind) {
        area.set(page, free);
    }
}

/// Whether area `kind`'s runs, in the space whose header is at `header`, take `page`: the checked
/// build's audit (arch/riscv/mem.rs, `check_runs`). `None` before the space has runs.
#[cfg(debug_assertions)]
pub fn taken(header: usize, kind: MemoryType, page: usize) -> Option<bool> {
    Some(!area(header, kind)?.is_free(page))
}

/// How many pages of area `kind` its runs take, and whether every record is what its words say,
/// the pending one settled first: the same audit, at a space's end, when settling changes nothing
/// that is read again.
#[cfg(debug_assertions)]
pub fn audit(header: usize, kind: MemoryType) -> Option<(usize, bool)> {
    let mut area = area(header, kind)?;
    area.settle();
    Some((area.pages() - area.free_pages(), area.records_hold()))
}

/// Give the space whose header is at `header` its runs: `frames` hold `map_anon`'s bits, and
/// every page of both areas is free. The caller then marks what is already mapped.
pub fn set_up(header: usize, frames: [usize; FRAMES_TAKEN]) {
    for (n, frame) in frames.iter().enumerate() {
        kframe::write(header, header_word(FRAMES + n), *frame as u64);
    }
    for (kind, ..) in AREAS {
        area(header, kind).expect("the frames just written").clear();
    }
}

/// The first free run of `size` bytes in area `kind` of the space whose header is at `header`,
/// from `last`, the address placed last, as `find_virtual_address` has always searched: from
/// there up to the last start that fits, then from the area's start. At most `most_reads` words
/// and records whatever the area holds (R22).
pub fn search(header: usize, kind: MemoryType, last: usize, size: usize) -> Option<usize> {
    debug_assert!(size % PAGE_SIZE == 0, "a placement is whole pages");
    let base = AREAS.iter().find(|(k, ..)| *k == kind).expect("an area").1;
    let mut area = area(header, kind).expect("a running space has its runs");
    let mut reads = 0;
    let found = area.place(last.saturating_sub(base) / PAGE_SIZE, size / PAGE_SIZE, &mut reads);
    debug_assert!(reads <= area.most_reads(), "R22: a search read {} words and records", reads);
    found.map(|page| base + page * PAGE_SIZE)
}
