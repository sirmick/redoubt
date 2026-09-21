// SPDX-License-Identifier: MIT OR Apache-2.0

//! Handle tables (KERNEL-SPEC.md, Handle; I1): per process, an index names a (object, badge,
//! stamp) triple that only the kernel writes, so a process can use only what its own table
//! holds.
//!
//! # Layout, and the cost table's 128
//! A handle is four 64-bit words (32 bytes): the object's kind with its frame and the stamping
//! budget's frame, packed in one word; the object's id; the badge; the stamping budget's id. So a
//! 4 KiB page holds exactly [`HANDLES_PER_PAGE`] = 128 handles, the cost table's figure. Index `i`
//! (from 1; 0 is never allocated) is slot `(i - 1) % 128` of table page `(i - 1) / 128`, so the
//! first 128 handles fill the first page.
//!
//! A table page is a RAM frame of its own, allocated when its first handle is installed and
//! freed when its last is removed, each charged one page to the process's budget. A new handle
//! takes the lowest free index (as the model does), so a table filled without closing any
//! handle costs exactly ceil(n / 128) pages; one with holes costs a page per page in use, which
//! is what the memory really costs (answer 111).
//!
//! The object and the stamp each name a budget by frame and by id. R10's sweep closes every
//! handle naming or stamped with a budget before that budget's frame is freed (I2), so both
//! frames always hold the budgets named; every path that reads either checks the id, and a
//! mismatch (a handle that escaped a sweep, naming a reused frame) stops the kernel (I1).

use redoubt_sys::Error;
use redoubt_abi::PID;

use crate::budget::{Budget, BudgetFrame};
use crate::kframe;
use crate::mem::MemoryManager;

/// Handles in one table page: `PAGE_SIZE` / 32 bytes.
pub const HANDLES_PER_PAGE: usize = 128;
/// Handles one process may hold (answer 102; the ABI's constant): a table is an array of this
/// many divided by 128 pages. Installing one more is `TooLarge`.
pub use redoubt_sys::MAX_HANDLES;
/// Table pages a process may have.
pub const MAX_HANDLE_PAGES: usize = MAX_HANDLES / HANDLES_PER_PAGE;
/// Words one handle takes.
const HANDLE_WORDS: usize = 4;
const _: () = assert!(HANDLES_PER_PAGE * HANDLE_WORDS * 8 == redoubt_abi::arch::PAGE_SIZE);
/// Bits of a frame index in a handle's first word. Two fit, with the kind above them, because
/// the physmap reaches at most 2^25 frames.
const FRAME_BITS: u32 = 28;
const _: () = assert!(redoubt_abi::arch::PHYSMAP_SIZE / redoubt_abi::arch::PAGE_SIZE <= 1 << FRAME_BITS);

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

/// What a handle names. WP-K4 adds processes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Object {
    Budget(BudgetRef),
    Endpoint(EndpointRef),
    Device(DeviceRef),
}

/// KERNEL-SPEC.md, Handle = (object, badge, stamp).
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

impl Handle {
    /// A handle as the four words a table slot holds. `message.rs` keeps copies in the same
    /// form, in a thread's IPC page.
    pub fn to_words(&self) -> [u64; HANDLE_WORDS] {
        let (kind, frame, id) = match self.object {
            Object::Budget(b) => (KIND_BUDGET, b.frame, b.id),
            Object::Endpoint(e) => (KIND_ENDPOINT, e.frame, e.id),
            Object::Device(d) => (KIND_DEVICE, d.frame, d.id),
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

/// One process's table: the frame of each page (in use or not), and how many handles each holds.
#[derive(Clone, Copy)]
pub struct HandleTable {
    pages: [Option<u32>; MAX_HANDLE_PAGES],
    live: [u8; MAX_HANDLE_PAGES],
}

impl HandleTable {
    pub const EMPTY: HandleTable = HandleTable { pages: [None; MAX_HANDLE_PAGES], live: [0; MAX_HANDLE_PAGES] };
}

/// (page, slot) of index `i`, or `None` if `i` cannot be an index (0, or past the last page).
fn position(index: u32) -> Option<(usize, usize)> {
    let i = (index as usize).checked_sub(1)?;
    let page = i / HANDLES_PER_PAGE;
    (page < MAX_HANDLE_PAGES).then_some((page, i % HANDLES_PER_PAGE))
}

fn slot_offset(slot: usize, word: usize) -> usize { (slot * HANDLE_WORDS + word) * 8 }

impl MemoryManager {
    fn table(&self, pid: PID) -> Option<&HandleTable> { self.account(pid).map(|a| &a.handles) }

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
    pub fn handle(&self, pid: PID, index: u32) -> Result<Handle, Error> {
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
        }
        self.budget_at(handle.stamp);
        Ok(handle)
    }

    /// The budget `pid`'s handle `index` names: `BadHandle`, then `WrongObject`.
    pub fn budget_handle(&self, pid: PID, index: u32) -> Result<BudgetFrame, Error> {
        match self.handle(pid, index)?.object {
            Object::Budget(b) => Ok(b.frame),
            _ => Err(Error::WrongObject),
        }
    }

    /// Put `handle` at the lowest free index of `pid`'s table. A new table page is charged to
    /// the process's budget (`OutOfMemory` if it cannot pay); a table already holding
    /// `MAX_HANDLES` gets `TooLarge`.
    pub fn install_handle(&mut self, pid: PID, handle: Handle) -> Result<u32, Error> {
        // Only the kernel has no account, and it holds no handles (see `budget_create`).
        let budget = self.budget_of(pid).ok_or(Error::NotPermitted)?;
        let table = *self.table(pid).expect("account");
        for page in 0..MAX_HANDLE_PAGES {
            let frame = match table.pages[page] {
                Some(_) if usize::from(table.live[page]) == HANDLES_PER_PAGE => continue,
                Some(frame) => frame,
                None => {
                    // One page per table page in use, charged when the page is first needed and
                    // returned when it empties (answer 111).
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
            return Ok((page * HANDLES_PER_PAGE + slot + 1) as u32);
        }
        // Past MAX_HANDLES (answer 102).
        Err(Error::TooLarge)
    }

    /// The table pages `pid` would have to buy to hold `extra` more handles, or `None` if they
    /// would take it past `MAX_HANDLES` (answer 102). Delivery asks before it charges (R4).
    pub fn table_growth(&self, pid: PID, extra: usize) -> Option<u64> {
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
    fn remove_handle(&mut self, pid: PID, index: u32) {
        let Some((page, slot)) = position(index) else { return };
        let Some(frame) = self.table(pid).and_then(|t| t.pages[page]) else { return };
        if self.read_slot(frame, slot).is_none() {
            return;
        }
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
    pub fn handle_close(&mut self, pid: PID, index: u32) -> Result<(), Error> {
        self.handle(pid, index)?;
        self.remove_handle(pid, index);
        Ok(())
    }

    /// Remove every handle for which `doomed` holds, from every process's table (R10).
    pub fn sweep_handles(&mut self, doomed: impl Fn(&Self, &Handle) -> bool) {
        for pid in 1..=crate::arch::process::MAX_PROCESS_COUNT {
            if let Some(pid) = PID::new(pid as u8) {
                self.remove_handles_where(pid, &doomed);
            }
        }
    }

    /// Remove every handle of a process that is ending.
    pub fn close_all_handles(&mut self, pid: PID) { self.remove_handles_where(pid, &|_, _| true); }

    fn remove_handles_where(&mut self, pid: PID, doomed: &impl Fn(&Self, &Handle) -> bool) {
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
