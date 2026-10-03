// SPDX-License-Identifier: MIT OR Apache-2.0

//! Handle tables (kernel/objects.md, "Handles"; I1): per process, an index names a (object, badge,
//! stamp) triple that only the kernel writes, so a process can use only what its own table
//! holds.
//!
//! # Layout, and the cost table's 64
//! A handle is four 64-bit words: the object's kind with its frame and the stamping budget's
//! frame, packed in one word; the object's id; the badge; the stamping budget's id. A message's
//! copy of a handle is those four words. A table slot adds two chain entries (below), so it is
//! eight words, 64 bytes, and a 4 KiB page holds exactly [`HANDLES_PER_PAGE`] = 64 handles, the
//! cost table's figure. Index `i` (from 1; 0 is never allocated) is slot `(i - 1) % 64` of table
//! page `(i - 1) / 64`, so the first 64 handles fill the first page.
//!
//! A table page is a RAM frame of its own, allocated when its first handle is installed and
//! freed when its last is removed, each charged one page to the process's budget. A new handle
//! takes the lowest free index (as the model does), so a table filled without closing any
//! handle costs exactly ceil(n / 64) pages; one with holes costs a page per page in use, which
//! is what the memory really costs (kernel/objects.md, "Handles").
//!
//! # The chains (kernel/budgets.md, "Residual risks", item 3)
//! A handle dies when the object it names goes or when the budget that stamped it does. One
//! held inside that budget's subtree dies with its holder's table, which goes whole. One held
//! outside is entered, when it arrives in a table, in the chain of the budget it depends on:
//! the object chain of the budget its object is charged to (a budget handle's is the budget
//! itself), and the stamp chain of its stamp. A process object can go while its creator lives
//! (its notice taken), so a handle naming one is entered in that object's own chain wherever it
//! is held. Each chain is doubly linked through the slots' entry words, from a head word in the
//! frame it belongs to; an entry names its neighbour by (holder, index), and the first names the
//! head's frame, so an entry is unlinked without reading any object. A destruction closes the
//! dying budgets' chains, and nothing sweeps a live table.
//!
//! The object and the stamp each name a budget by frame and by id. Every handle naming or
//! stamped with a budget is closed before that budget's frame is freed (I2), so both frames
//! always hold the budgets named; every path that reads either checks the id, and a mismatch (a
//! handle that escaped its chain, naming a reused frame) stops the kernel (I1).

use redoubt_layout::Pid;
use redoubt_sys::Error;

use crate::budget::{Budget, BudgetFrame};
use crate::kframe;
use crate::mem::MemoryManager;

/// Handles in one table page: `PAGE_SIZE` / 64 bytes.
pub const HANDLES_PER_PAGE: usize = 64;
/// Handles one process may hold (kernel/objects.md; the ABI's constant): a table is an array of
/// this many divided by 64 pages. Installing one more is `TooLarge`.
pub use redoubt_sys::MAX_HANDLES;
/// Table pages a process may have.
pub const MAX_HANDLE_PAGES: usize = MAX_HANDLES / HANDLES_PER_PAGE;
/// Words one handle takes, in a slot and in a message.
const HANDLE_WORDS: usize = 4;
/// Words one table slot takes: the handle, then its two chain entries.
const SLOT_WORDS: usize = 8;
const _: () = assert!(HANDLES_PER_PAGE * SLOT_WORDS * 8 == redoubt_sys::PAGE_SIZE);
const _: () = assert!(MAX_HANDLE_PAGES <= u8::MAX as usize && HANDLES_PER_PAGE <= u8::MAX as usize);
/// Bits of a frame index in a handle's first word. Two fit, with the kind above them, because
/// the physmap reaches at most 2^25 frames.
const FRAME_BITS: u32 = 28;
const _: () = assert!(redoubt_layout::PHYSMAP_SIZE / redoubt_sys::PAGE_SIZE <= 1 << FRAME_BITS);

/// A budget, named by frame and by id (the id is what the spec's stamp is).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BudgetRef {
    pub frame: BudgetFrame,
    pub id: u64,
}

/// An endpoint, named by frame and by id, like a budget (`endpoint.rs`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EndpointRef {
    pub frame: u32,
    pub id: u64,
}

/// A device, named by frame and by id, like an endpoint (`device.rs`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeviceRef {
    pub frame: u32,
    pub id: u64,
}

/// A process, named by frame and by id, like an endpoint (`process.rs`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProcessRef {
    pub frame: u32,
    pub id: u64,
}

/// What a handle names (kernel/objects.md, "The four object kinds").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Object {
    Budget(BudgetRef),
    Endpoint(EndpointRef),
    Device(DeviceRef),
    Process(ProcessRef),
}

/// kernel/objects.md, "Handles": a handle = (object, badge, stamp).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handle {
    pub object: Object,
    pub badge: u64,
    pub stamp: BudgetRef,
}

/// Object kinds in a handle's first word; 0 is an empty slot.
const KIND_BUDGET: u64 = 1;
const KIND_ENDPOINT: u64 = 2;
const KIND_DEVICE: u64 = 3;
const KIND_PROCESS: u64 = 4;

impl Handle {
    /// A handle as the four words a table slot holds. `message.rs` keeps copies in the same
    /// form, in a thread's IPC page.
    pub fn to_words(&self) -> [u64; HANDLE_WORDS] {
        let (kind, frame, id) = match self.object {
            Object::Budget(b) => (KIND_BUDGET, b.frame, b.id),
            Object::Endpoint(e) => (KIND_ENDPOINT, e.frame, e.id),
            Object::Device(d) => (KIND_DEVICE, d.frame, d.id),
            Object::Process(p) => (KIND_PROCESS, p.frame, p.id),
        };
        let mask = (1u64 << FRAME_BITS) - 1;
        let (of, sf) = (u64::from(frame), u64::from(self.stamp.frame));
        assert!(of <= mask && sf <= mask, "frame index out of range");
        [kind << (2 * FRAME_BITS) | sf << FRAME_BITS | of, id, self.badge, self.stamp.id]
    }

    /// The handle four words hold; `None` for an empty slot.
    pub fn from_words(words: [u64; HANDLE_WORDS]) -> Option<Handle> {
        let mask = (1u64 << FRAME_BITS) - 1;
        let frame = |shift: u32| ((words[0] >> shift) & mask) as u32;
        let object = match words[0] >> (2 * FRAME_BITS) {
            0 => return None,
            KIND_BUDGET => Object::Budget(BudgetRef { frame: frame(0), id: words[1] }),
            KIND_ENDPOINT => Object::Endpoint(EndpointRef { frame: frame(0), id: words[1] }),
            KIND_DEVICE => Object::Device(DeviceRef { frame: frame(0), id: words[1] }),
            KIND_PROCESS => Object::Process(ProcessRef { frame: frame(0), id: words[1] }),
            // Only the kernel writes table pages.
            _ => panic!("I1: corrupt handle table"),
        };
        Some(Handle { object, badge: words[2], stamp: BudgetRef { frame: frame(FRAME_BITS), id: words[3] } })
    }
}

impl MemoryManager {
    /// The budget `r` names, which must still be the one it named (I1): a handle that escaped
    /// R10's sweep would name a frame freed and perhaps reused, and the kernel stops instead.
    pub fn budget_at(&self, r: BudgetRef) -> Budget {
        let b = self.budget(r.frame);
        assert!(b.id == r.id, "I1: a handle names a budget that is gone");
        b
    }
}

/// One process's table: the frame of each page (in use or not), how many handles each holds, and
/// how many chain entries its slots hold, so that freeing the table whole reads only the pages
/// with one.
#[derive(Clone, Copy)]
pub struct HandleTable {
    pages: [Option<u32>; MAX_HANDLE_PAGES],
    live: [u8; MAX_HANDLE_PAGES],
    chained: [u8; MAX_HANDLE_PAGES],
}

impl HandleTable {
    pub const EMPTY: HandleTable = HandleTable {
        pages: [None; MAX_HANDLE_PAGES],
        live: [0; MAX_HANDLE_PAGES],
        chained: [0; MAX_HANDLE_PAGES],
    };
}

/// The two chains a table slot can be entered in (the module's docs).
#[derive(Clone, Copy)]
enum Chain {
    /// The chain of the budget the handle's object is charged to, or of the process object.
    Object,
    /// The chain of the budget that stamped the handle.
    Stamp,
}

impl Chain {
    /// The slot word holding this chain's previous link; the next link follows it.
    fn prev_word(self) -> usize {
        match self {
            Chain::Object => HANDLE_WORDS,
            Chain::Stamp => HANDLE_WORDS + 2,
        }
    }

    /// The word of a budget or process-object frame holding this chain's first link.
    fn head_word(self) -> usize {
        match self {
            Chain::Object => crate::budget::HELD_WORD,
            Chain::Stamp => crate::budget::STAMPED_WORD,
        }
    }
}

/// One link of a chain: none (not entered, or the last), the head word of the frame the chain
/// belongs to (the first entry's previous link), or a slot named by its holder and index, which
/// the holder's table turns into a frame without a reverse map.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Link {
    None,
    Head(u32),
    Slot(Pid, u32),
}

/// The bit that marks a [`Link::Head`] word; a slot's word is `pid << 32 | index` (an index is
/// never 0).
const HEAD_BIT: u64 = 1 << 63;

impl Link {
    fn word(self) -> u64 {
        match self {
            Link::None => 0,
            Link::Head(frame) => HEAD_BIT | u64::from(frame),
            Link::Slot(pid, index) => u64::from(pid.get()) << 32 | u64::from(index),
        }
    }

    fn from_word(word: u64) -> Link {
        match word {
            0 => Link::None,
            w if w & HEAD_BIT != 0 => Link::Head(w as u32),
            // Only the kernel writes table pages and heads.
            w => Link::Slot(crate::budget::pid_from(w >> 32).expect("I1: corrupt chain link"), w as u32),
        }
    }
}

/// (page, slot) of index `i`, or `None` if `i` cannot be an index (0, or past the last page).
fn position(index: u32) -> Option<(usize, usize)> {
    let i = (index as usize).checked_sub(1)?;
    let page = i / HANDLES_PER_PAGE;
    (page < MAX_HANDLE_PAGES).then_some((page, i % HANDLES_PER_PAGE))
}

fn slot_offset(slot: usize, word: usize) -> usize { (slot * SLOT_WORDS + word) * 8 }

impl MemoryManager {
    fn table(&self, pid: Pid) -> Option<&HandleTable> { self.account(pid).map(|a| &a.handles) }

    fn read_slot(&self, frame: u32, slot: usize) -> Option<Handle> {
        let phys = self.object_phys(frame);
        Handle::from_words(core::array::from_fn(|w| kframe::read(phys, slot_offset(slot, w))))
    }

    fn write_slot(&mut self, frame: u32, slot: usize, words: [u64; HANDLE_WORDS]) {
        let phys = self.object_phys(frame);
        for (w, word) in words.iter().enumerate() {
            kframe::write(phys, slot_offset(slot, w), *word);
        }
    }

    /// The handle at `index` in `pid`'s table; `BadHandle` if there is none.
    pub fn handle(&self, pid: Pid, index: u32) -> Result<Handle, Error> {
        let (page, slot) = position(index).ok_or(Error::BadHandle)?;
        let frame = self.table(pid).and_then(|t| t.pages[page]).ok_or(Error::BadHandle)?;
        let handle = self.read_slot(frame, slot).ok_or(Error::BadHandle)?;
        // Every live handle names a live object, and a live stamp (I1).
        match handle.object {
            Object::Budget(b) => {
                self.budget_at(b);
            }
            Object::Endpoint(e) => {
                self.endpoint_at(e);
            }
            Object::Device(d) => {
                self.device_at(d);
            }
            Object::Process(p) => {
                self.process_at(p);
            }
        }
        self.budget_at(handle.stamp);
        Ok(handle)
    }

    /// The budget `pid`'s handle `index` names: `BadHandle`, then `WrongObject`.
    pub fn budget_handle(&self, pid: Pid, index: u32) -> Result<BudgetFrame, Error> {
        match self.handle(pid, index)?.object {
            Object::Budget(b) => Ok(b.frame),
            _ => Err(Error::WrongObject),
        }
    }

    /// Put `handle` at the lowest free index of `pid`'s table. A new table page is charged to
    /// the process's budget (`OutOfMemory` if it cannot pay); a table already holding
    /// `MAX_HANDLES` gets `TooLarge`.
    pub fn install_handle(&mut self, pid: Pid, handle: Handle) -> Result<u32, Error> {
        // Only the kernel has no account, and it holds no handles (see `budget_create`).
        let budget = self.budget_of(pid).ok_or(Error::NotPermitted)?;
        let table = *self.table(pid).expect("account");
        for page in 0..MAX_HANDLE_PAGES {
            let frame = match table.pages[page] {
                Some(_) if usize::from(table.live[page]) == HANDLES_PER_PAGE => continue,
                Some(frame) => frame,
                None => {
                    // One page per table page in use, charged when the page is first needed and
                    // returned when it empties (kernel/objects.md, "Handles").
                    self.charge(budget, 1)?;
                    let frame = self.alloc_object_frame().inspect_err(|_| self.uncharge(budget, 1))?;
                    self.account_mut(pid).expect("account").handles.pages[page] = Some(frame);
                    frame
                }
            };
            let slot = (0..HANDLES_PER_PAGE)
                .find(|slot| self.read_slot(frame, *slot).is_none())
                .expect("a table page with a free slot");
            self.write_slot(frame, slot, handle.to_words());
            self.account_mut(pid).expect("account").handles.live[page] += 1;
            let index = (page * HANDLES_PER_PAGE + slot + 1) as u32;
            let (object, stamp) = self.chain_heads(budget, &handle);
            // Test builds only: a missing stamp entry, or a missing process object entry, for the
            // audit below to catch.
            #[cfg(feature = "handle-chain-fault")]
            let stamp = stamp.filter(|_| false);
            #[cfg(feature = "process-chain-fault")]
            let object = object.filter(|_| !matches!(handle.object, Object::Process(_)));
            if let Some(head) = object {
                self.enter(Chain::Object, head, pid, index);
            }
            if let Some(head) = stamp {
                self.enter(Chain::Stamp, head, pid, index);
            }
            return Ok(index);
        }
        // Past MAX_HANDLES (kernel/objects.md, "Handles").
        Err(Error::TooLarge)
    }

    /// The table pages `pid` would have to buy to hold `extra` more handles, or `None` if they
    /// would take it past `MAX_HANDLES` (kernel/objects.md). Delivery asks before it charges (R4).
    pub fn table_growth(&self, pid: Pid, extra: usize) -> Option<u64> {
        let table = self.table(pid)?;
        let mut free = 0;
        let mut pages = 0;
        for page in 0..MAX_HANDLE_PAGES {
            match table.pages[page] {
                Some(_) => free += HANDLES_PER_PAGE - usize::from(table.live[page]),
                None => pages += 1,
            }
        }
        let bought = extra.saturating_sub(free).div_ceil(HANDLES_PER_PAGE);
        (bought <= pages).then_some(bought as u64)
    }

    /// Remove `pid`'s handle `index`, freeing its table page if it was the page's last.
    fn remove_handle(&mut self, pid: Pid, index: u32) {
        let Some((page, slot)) = position(index) else { return };
        let Some(frame) = self.table(pid).and_then(|t| t.pages[page]) else { return };
        if self.read_slot(frame, slot).is_none() {
            return;
        }
        self.leave(Chain::Object, pid, index);
        self.leave(Chain::Stamp, pid, index);
        self.write_slot(frame, slot, [0; HANDLE_WORDS]);
        let budget = self.budget_of(pid).expect("a table belongs to an account");
        let table = &mut self.account_mut(pid).expect("account").handles;
        table.live[page] -= 1;
        if table.live[page] == 0 {
            table.pages[page] = None;
            self.free_object_frame(frame);
            self.uncharge(budget, 1);
        }
    }

    /// `handle_close(h)`: `BadHandle` if `h` is not in the caller's table.
    pub fn handle_close(&mut self, pid: Pid, index: u32) -> Result<(), Error> {
        self.handle(pid, index)?;
        self.remove_handle(pid, index);
        Ok(())
    }

    /// Remove every handle for which `doomed` holds, from every process's table (R10).
    pub fn sweep_handles(&mut self, doomed: impl Fn(&Self, &Handle) -> bool) {
        for pid in crate::budget::pids() {
            if self.account(pid).is_some() {
                self.remove_handles_where(pid, &doomed);
            }
        }
    }

    /// [`MemoryManager::sweep_handles`], except during a destruction: then the dying owner's
    /// chain closes them (`destroy_marked`), so a sweep would only repeat work (I1, I2).
    pub fn sweep_handles_now(&mut self, doomed: impl Fn(&Self, &Handle) -> bool) {
        if self.objects.deferring {
            return;
        }
        self.sweep_handles(doomed);
    }

    /// Free the table of a process that is ending, whole: each slot still entered in a chain is
    /// unlinked from it (a word read per entry, on the pages that hold one), then each page goes
    /// back at once. Nothing is cleared slot by slot: a table page is zeroed when it is next
    /// allocated.
    pub fn close_all_handles(&mut self, pid: Pid) {
        let Some(budget) = self.budget_of(pid) else { return };
        let table = *self.table(pid).expect("account");
        let mut freed = 0;
        for (page, frame) in table.pages.iter().enumerate() {
            let Some(frame) = *frame else { continue };
            let phys = self.object_phys(frame);
            for slot in 0..HANDLES_PER_PAGE {
                if self.table(pid).expect("account").chained[page] == 0 {
                    break;
                }
                let index = (page * HANDLES_PER_PAGE + slot + 1) as u32;
                for chain in [Chain::Object, Chain::Stamp] {
                    if kframe::read(phys, slot_offset(slot, chain.prev_word())) != 0 {
                        self.leave(chain, pid, index);
                    }
                }
            }
            self.free_object_frame(frame);
            freed += 1;
        }
        self.account_mut(pid).expect("account").handles = HandleTable::EMPTY;
        self.uncharge(budget, freed);
    }

    /// Close every handle that depends on `budget`, which is dying: those held outside its
    /// subtree, entered in its two chains (R10 step 6). The ones held inside went with their
    /// holders' tables.
    pub fn close_dependents(&mut self, budget: BudgetFrame) {
        self.close_chain(Chain::Object, budget);
        self.close_chain(Chain::Stamp, budget);
    }

    /// Close every handle naming the process object in `frame`, which is being freed: its own
    /// chain holds them all, wherever they are held.
    pub fn close_handles_to(&mut self, frame: u32) { self.close_chain(Chain::Object, frame); }

    fn close_chain(&mut self, chain: Chain, frame: u32) {
        while let Link::Slot(pid, index) = self.head(chain, frame) {
            self.remove_handle(pid, index);
            assert!(self.head(chain, frame) != Link::Slot(pid, index), "I1: a chain names an empty slot");
        }
    }

    /// The checked build's audit, run once after a destruction's walk (`check_object_indexes`):
    /// every live handle is entered in exactly the chains its holder's place calls for, and each
    /// table page counts its entries right. A scan of every table, so never on the walk.
    #[cfg(debug_assertions)]
    pub(crate) fn check_handle_chains(&self) {
        for pid in crate::budget::pids() {
            let (Some(holder), Some(table)) = (self.budget_of(pid), self.table(pid)) else { continue };
            for (page, frame) in table.pages.iter().enumerate() {
                let Some(frame) = *frame else { continue };
                let phys = self.object_phys(frame);
                let mut entries = 0;
                for slot in 0..HANDLES_PER_PAGE {
                    let Some(h) = self.read_slot(frame, slot) else { continue };
                    let (object, stamp) = self.chain_heads(holder, &h);
                    for (chain, head) in [(Chain::Object, object), (Chain::Stamp, stamp)] {
                        let entered = kframe::read(phys, slot_offset(slot, chain.prev_word())) != 0;
                        assert!(
                            entered == head.is_some(),
                            "I2: handle {} of PID {} is not in the chains its holder's place calls for",
                            page * HANDLES_PER_PAGE + slot + 1,
                            pid
                        );
                        entries += usize::from(entered);
                    }
                    if let Object::Process(p) = h.object {
                        assert!(self.is_live_process(p), "I1: a handle names a freed process object");
                    }
                }
                assert!(
                    entries == usize::from(table.chained[page]),
                    "I2: a table page miscounts its chain entries"
                );
            }
        }
    }

    /// The frames whose chains a handle held in `holder` is entered in: its object's and its
    /// stamp's, each only if `holder` is outside that budget's subtree. A process object's
    /// chain is its own and takes every handle naming it, since the object can go while its
    /// creator lives.
    fn chain_heads(&self, holder: BudgetFrame, h: &Handle) -> (Option<u32>, Option<u32>) {
        let outside = |b: BudgetFrame| (!self.within(holder, b)).then_some(b);
        let object = match h.object {
            Object::Budget(b) => outside(b.frame),
            Object::Endpoint(e) => outside(self.endpoint_at(e).owner.frame),
            Object::Device(d) => outside(self.device_at(d).owner.frame),
            Object::Process(p) => Some(p.frame),
        };
        (object, outside(h.stamp.frame))
    }

    /// Whether `holder` is `b` or below it: at most `MAX_DEPTH` steps up the tree, and the answer
    /// holds for a handle's life, since no budget moves.
    fn within(&self, holder: BudgetFrame, b: BudgetFrame) -> bool {
        let mut at = Some(holder);
        while let Some(frame) = at {
            if frame == b {
                return true;
            }
            at = self.budget(frame).parent;
        }
        false
    }

    /// The table page and slot holding `pid`'s handle `index`, which a chain names.
    fn slot_at(&self, pid: Pid, index: u32) -> (usize, usize) {
        let (page, slot) = position(index).expect("I1: a chain names no index");
        let frame =
            self.table(pid).and_then(|t| t.pages[page]).expect("I1: a chain names a slot of no table");
        (self.object_phys(frame), slot)
    }

    fn head(&self, chain: Chain, frame: u32) -> Link {
        Link::from_word(kframe::read(self.object_phys(frame), chain.head_word() * 8))
    }

    fn set_head(&mut self, chain: Chain, frame: u32, first: Link) {
        kframe::write(self.object_phys(frame), chain.head_word() * 8, first.word());
    }

    /// Entry word `word` (0 the previous link, 1 the next) of `pid`'s handle `index` in `chain`.
    fn link(&self, chain: Chain, pid: Pid, index: u32, word: usize) -> Link {
        let (phys, slot) = self.slot_at(pid, index);
        Link::from_word(kframe::read(phys, slot_offset(slot, chain.prev_word() + word)))
    }

    fn set_link(&mut self, chain: Chain, pid: Pid, index: u32, word: usize, to: Link) {
        let (phys, slot) = self.slot_at(pid, index);
        kframe::write(phys, slot_offset(slot, chain.prev_word() + word), to.word());
    }

    /// The count of chain entries on the table page holding `pid`'s handle `index`.
    fn chained(&mut self, pid: Pid, index: u32) -> &mut u8 {
        let (page, _) = position(index).expect("I1: a chain names no index");
        &mut self.account_mut(pid).expect("account").handles.chained[page]
    }

    /// Put `pid`'s handle `index` first in `frame`'s `chain`.
    fn enter(&mut self, chain: Chain, frame: u32, pid: Pid, index: u32) {
        let first = self.head(chain, frame);
        self.set_link(chain, pid, index, 0, Link::Head(frame));
        self.set_link(chain, pid, index, 1, first);
        if let Link::Slot(p, i) = first {
            self.set_link(chain, p, i, 0, Link::Slot(pid, index));
        }
        self.set_head(chain, frame, Link::Slot(pid, index));
        *self.chained(pid, index) += 1;
    }

    /// Take `pid`'s handle `index` out of `chain`, if it is entered there.
    fn leave(&mut self, chain: Chain, pid: Pid, index: u32) {
        let (prev, next) = (self.link(chain, pid, index, 0), self.link(chain, pid, index, 1));
        match prev {
            Link::None => return,
            Link::Head(frame) => self.set_head(chain, frame, next),
            Link::Slot(p, i) => self.set_link(chain, p, i, 1, next),
        }
        if let Link::Slot(p, i) = next {
            self.set_link(chain, p, i, 0, prev);
        }
        self.set_link(chain, pid, index, 0, Link::None);
        self.set_link(chain, pid, index, 1, Link::None);
        *self.chained(pid, index) -= 1;
    }

    fn remove_handles_where(&mut self, pid: Pid, doomed: &impl Fn(&Self, &Handle) -> bool) {
        for page in 0..MAX_HANDLE_PAGES {
            for slot in 0..HANDLES_PER_PAGE {
                // Looked up again for every slot: removing a page's last handle frees the page.
                let Some(frame) = self.table(pid).and_then(|t| t.pages[page]) else { break };
                if self.read_slot(frame, slot).is_some_and(|h| doomed(self, &h)) {
                    self.remove_handle(pid, (page * HANDLES_PER_PAGE + slot + 1) as u32);
                }
            }
        }
    }
}
