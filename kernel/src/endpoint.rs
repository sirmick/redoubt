// SPDX-License-Identifier: MIT OR Apache-2.0

//! Endpoints (kernel/ipc.md): the object clients call and servers receive on.
//!
//! # Where an endpoint lives
//! One RAM frame of its own, allocated to `mem::OBJECT_OWNER`, exactly as a budget is
//! (`budget.rs`): that frame *is* the page the cost table charges to the owner. An endpoint is
//! named by its frame index, and every lookup checks the id in the frame against the id in the
//! handle, so a handle that outlived its object stops the kernel instead of naming a reused frame
//! (I1).
//!
//! # What it does *not* hold
//! It holds no message buffer. A queued message is a message whose sender is blocked in `send` or
//! `call`, and a thread blocks at most once, so the queue *is* the set of blocked senders, which
//! `message.rs` keeps in each thread's own page. The endpoint's frame holds only the heads of the
//! lists that link them, R2's groups in turn order, and its receivers, owed notices and open
//! calls ([`LIST_WORD`]); the links are in the threads' and the open calls' pages. So no sender
//! can make the kernel allocate.
//!
//! R2's groups are ordered by when their turns fall due, values of one counter that no two turns
//! share, so the kernel never meets the tie R2 breaks by group key.
//!
//! The **owner** is the budget of the process that created it, which is what R1 compares a sender
//! with (I7) and where the cost table charges the page.

use redoubt_layout::Pid;
use redoubt_sys::{Error, MAX_LABELS};

use crate::budget::{Budget, BudgetFrame};
use crate::handle::{BudgetRef, EndpointRef, Handle, Object};
use crate::kframe;
use crate::mem::MemoryManager;

/// The cost table (kernel/objects.md, "What objects cost"), in pages.
pub const ENDPOINT_PAGES: u64 = 1;

/// First word of every endpoint frame, so that a frame read as an endpoint that is not one is
/// caught. Distinct from `budget.rs`'s magic and from any handle-table word.
const MAGIC: u64 = u64::from_le_bytes(*b"endpoint");

/// R2's group: a sender budget's (account, label set), plus its budget id when the account is 0,
/// so that two system callers in different budgets never share a turn or a cap (kernel/ipc.md R2).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Group {
    pub account: u64,
    pub labels: [u64; MAX_LABELS],
    pub nlabels: usize,
    /// The sender's budget id, but only when `account` is 0; otherwise 0.
    pub budget: u64,
}

impl Group {
    /// The group a message from `sender` belongs to.
    pub fn of(sender: &Budget) -> Group {
        Group {
            account: sender.account,
            labels: sender.labels,
            nlabels: sender.nlabels,
            budget: if sender.account == 0 { sender.id } else { 0 },
        }
    }
}

/// An endpoint as the kernel works with it; it lives in its frame as words (`endpoint`, `store`).
#[derive(Clone, Copy)]
pub struct Endpoint {
    pub id: u64,
    /// The budget of the process that created it: what R1 compares against (I7) and what pays
    /// for the page.
    pub owner: BudgetRef,
}

/// Words an endpoint takes in its frame (a frame has 512). Its link in its owner's list is
/// apart, at `budget::OWNED_WORD`.
const WORDS: usize = 4;
/// Where its lists start (`message.rs`): after its own words, which storing it rewrites, and
/// below every word kept apart.
pub const LIST_WORD: usize = WORDS;
const _: () = assert!(LIST_WORD + redoubt_ipclist::ENDPOINT_WORDS <= crate::budget::DEFER_WORD);

impl MemoryManager {
    pub fn endpoint(&self, frame: u32) -> Endpoint {
        let phys = self.object_phys(frame);
        let w = |i: usize| kframe::read(phys, i * 8);
        // As in `budget.rs`: a frame that does not hold an endpoint means a stale reference
        // survived R10's sweep, a violated invariant (I1), so the kernel stops.
        assert!(w(0) == MAGIC, "I1: frame {} holds no endpoint", frame);
        Endpoint { id: w(1), owner: BudgetRef { frame: (w(2) as u32).wrapping_sub(1), id: w(3) } }
    }

    pub fn store_endpoint(&mut self, frame: u32, e: &Endpoint) {
        let phys = self.object_phys(frame);
        let mut words = [0u64; WORDS];
        words[0] = MAGIC;
        words[1] = e.id;
        words[2] = u64::from(e.owner.frame) + 1;
        words[3] = e.owner.id;
        for (i, word) in words.iter().enumerate() {
            kframe::write(phys, i * 8, *word);
        }
    }

    /// Whether `frame` holds an endpoint. Used by R10's sweep, which scans the object frames.
    pub fn is_endpoint_frame(&self, frame: u32) -> bool {
        self.is_object_frame(frame) && kframe::read(self.object_phys(frame), 0) == MAGIC
    }

    /// Whether `r` still names the endpoint it named. As with `is_live_budget`, a stale
    /// reference is an answer here, not a kernel bug.
    pub fn is_live_endpoint(&self, r: EndpointRef) -> bool {
        self.is_endpoint_frame(r.frame) && self.endpoint(r.frame).id == r.id
    }

    /// The endpoint `r` names, which must still be the one it named (I1).
    pub fn endpoint_at(&self, r: EndpointRef) -> Endpoint {
        let e = self.endpoint(r.frame);
        assert!(e.id == r.id, "I1: a handle names an endpoint that is gone");
        e
    }

    /// A new endpoint owned by `owner`: one page, charged there (the cost table), and linked into
    /// the owner's object list so a destruction ends exactly its own (R10).
    pub fn new_endpoint(&mut self, owner: BudgetFrame) -> Result<EndpointRef, Error> {
        self.charge(owner, ENDPOINT_PAGES)?;
        let frame = self.alloc_object_frame().inspect_err(|_| self.uncharge(owner, ENDPOINT_PAGES))?;
        let id = self.next_object_id();
        let owner_ref = BudgetRef { frame: owner, id: self.budget(owner).id };
        self.store_endpoint(frame, &Endpoint { id, owner: owner_ref });
        self.link_owned(owner, frame);
        Ok(EndpointRef { frame, id })
    }

    /// `endpoint_create() -> h` (badge 0, the receive right; R9 stamps it with the caller's
    /// budget).
    pub fn endpoint_create(&mut self, pid: Pid) -> Result<u32, Error> {
        // As in `budget_create`: only the kernel has no account, and it makes no Redoubt calls.
        let owner = self.budget_of(pid).ok_or(Error::NotPermitted)?;
        let e = self.new_endpoint(owner)?;
        let stamp = self.endpoint(e.frame).owner;
        self.install_handle(pid, Handle { object: Object::Endpoint(e), badge: 0, stamp })
            .inspect_err(|_| self.free_endpoint(e.frame, owner))
    }

    /// The endpoint `pid`'s handle `index` names, with the handle: `BadHandle`, then
    /// `WrongObject`.
    pub fn endpoint_handle(&self, pid: Pid, index: u32) -> Result<(EndpointRef, Handle), Error> {
        let handle = self.handle(pid, index)?;
        match handle.object {
            Object::Endpoint(e) => Ok((e, handle)),
            _ => Err(Error::WrongObject),
        }
    }

    /// Free a new endpoint whose handle could not be installed: nothing names it or waits on it
    /// yet. A destruction frees its endpoints in `destroy_marked` instead.
    pub fn free_endpoint(&mut self, frame: u32, owner: BudgetFrame) {
        self.unlink_owned(owner, frame);
        self.release_object_frame(frame);
        self.uncharge(owner, ENDPOINT_PAGES);
    }
}
