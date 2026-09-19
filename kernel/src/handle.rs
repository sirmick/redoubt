// SPDX-License-Identifier: MIT OR Apache-2.0

//! Handle tables (KERNEL-SPEC.md, Handle; I1): per process, an index names a (object, badge,
//! stamp) triple that only the kernel writes, so a process can use only what its own table
//! holds.
//!
//! # Layout, and the cost table's 128
//! A handle is four 64-bit words (32 bytes): the object (its kind and frame index, then its id),
//! the badge, and the stamp (the stamping budget's frame). So a 4 KiB page holds exactly
//! [`HANDLES_PER_PAGE`] = 128 handles, the cost table's figure. Index `i` (from 1; 0 is never
//! allocated) is slot `(i - 1) % 128` of table page `(i - 1) / 128`, so the first 128 handles
//! fill the first page.
//!
//! A table page is a RAM frame of its own, allocated when its first handle is installed and
//! freed when its last is removed, each charged one page to the process's budget. A new handle
//! takes the lowest free index (as the model does), so a table filled without closing any
//! handle costs exactly ceil(n / 128) pages; one with holes costs a page per page in use.
//!
//! The stamp names a budget by frame, not by id: R10's sweep closes every handle stamped with a
//! budget before that budget's frame is freed (I2), so a live handle's stamp always names the
//! live budget that stamped it. Objects carry their id as well, and every lookup checks it.

use redoubt_sys::Error;
use xous_kernel::PID;

use crate::budget::BudgetFrame;
use crate::kframe;
use crate::mem::MemoryManager;

/// Handles in one table page: `PAGE_SIZE` / 32 bytes.
pub const HANDLES_PER_PAGE: usize = 128;
/// Table pages a process may have. There must be some bound for the array below; this one allows
/// 4096 handles per process.
pub const MAX_HANDLE_PAGES: usize = 32;
/// Words one handle takes.
const HANDLE_WORDS: usize = 4;
const _: () = assert!(HANDLES_PER_PAGE * HANDLE_WORDS * 8 == xous_kernel::arch::PAGE_SIZE);

/// What a handle names. WP-K2 to WP-K4 add endpoints, processes and devices.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Object {
    Budget { frame: BudgetFrame, id: u64 },
}

/// KERNEL-SPEC.md, Handle = (object, badge, stamp).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handle {
    pub object: Object,
    pub badge: u64,
    pub stamp: BudgetFrame,
}

/// Object kinds in a handle's first word; 0 is an empty slot.
const KIND_BUDGET: u64 = 1;

impl Handle {
    fn encode(&self) -> [u64; HANDLE_WORDS] {
        let (kind, index, id) = match self.object {
            Object::Budget { frame, id } => (KIND_BUDGET, frame, id),
        };
        [kind | u64::from(index) << 32, id, self.badge, u64::from(self.stamp)]
    }

    fn decode(words: [u64; HANDLE_WORDS]) -> Option<Handle> {
        let index = (words[0] >> 32) as u32;
        let object = match words[0] as u32 as u64 {
            0 => return None,
            KIND_BUDGET => Object::Budget { frame: index, id: words[1] },
            // Only the kernel writes table pages.
            _ => panic!("I1: corrupt handle table"),
        };
        Some(Handle { object, badge: words[2], stamp: words[3] as u32 })
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
        Handle::decode(core::array::from_fn(|w| kframe::read(phys, slot_offset(slot, w))))
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
        // Every live handle names a live object (I1): check the object is the one it named.
        match handle.object {
            Object::Budget { frame, id } => assert!(self.budget(frame).id == id, "I1: stale budget handle"),
        }
        Ok(handle)
    }

    /// The budget `pid`'s handle `index` names: `BadHandle`, then `WrongObject`.
    pub fn budget_handle(&self, pid: PID, index: u32) -> Result<BudgetFrame, Error> {
        match self.handle(pid, index)?.object {
            Object::Budget { frame, .. } => Ok(frame),
        }
    }

    /// Put `handle` at the lowest free index of `pid`'s table. A new table page is charged to
    /// the process's budget: `OutOfMemory` if it cannot pay, or if the table is at
    /// `MAX_HANDLE_PAGES`.
    pub fn install_handle(&mut self, pid: PID, handle: Handle) -> Result<u32, Error> {
        let budget = self.budget_of(pid).ok_or(Error::NotPermitted)?;
        let table = *self.table(pid).expect("account");
        for page in 0..MAX_HANDLE_PAGES {
            let frame = match table.pages[page] {
                Some(_) if usize::from(table.live[page]) == HANDLES_PER_PAGE => continue,
                Some(frame) => frame,
                None => {
                    self.charge(budget, 1)?;
                    let frame = self.alloc_object_frame().inspect_err(|_| self.uncharge(budget, 1))?;
                    self.account_mut(pid).expect("account").handles.pages[page] = Some(frame);
                    frame
                }
            };
            let slot = (0..HANDLES_PER_PAGE)
                .find(|slot| self.read_slot(frame, *slot).is_none())
                .expect("a table page with a free slot");
            self.write_slot(frame, slot, handle.encode());
            self.account_mut(pid).expect("account").handles.live[page] += 1;
            return Ok((page * HANDLES_PER_PAGE + slot + 1) as u32);
        }
        Err(Error::OutOfMemory)
    }

    /// Remove `pid`'s handle `index`, freeing its table page if it was the page's last.
    fn remove_handle(&mut self, pid: PID, index: u32) {
        let Some((page, slot)) = position(index) else { return };
        let Some(frame) = self.table(pid).and_then(|t| t.pages[page]) else { return };
        if self.read_slot(frame, slot).is_none() {
            return;
        }
        // WP-K2: closing the last handle with a badge sends its endpoint's owner a notice
        // (QUESTIONS.md 53); that hook goes here and in the sweep, which both come through here.
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
