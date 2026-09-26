// SPDX-License-Identifier: MIT OR Apache-2.0

//! DMA device reset and frame quarantine (kernel/devices.md, "Reset before reuse" and
//! "Quarantine"; I16).
//!
//! A DMA device may still hold the physical address of a `dma_alloc` frame after the process that
//! programmed it has died. So no such frame goes back to the pool until every device that could
//! hold its address has been reset: the device each of the dying process's runs came through,
//! plus every DMA device it ever mapped (the reset set S). A device that does not confirm its
//! reset keeps the process's frames for ever (quarantine), and its device object is destroyed as
//! R10 destroys one, so nobody is handed it again until reboot.
//!
//! # Where things are
//! - **The registry**: one slot per DMA device, keyed by MMIO base, so a slot outlives its device object. At
//!   most `MAX_DMA_DEVICES`; a DMA device beyond that gets no device object at all (`device.rs`), failing
//!   closed.
//! - **Runs**: what `dma_alloc` handed out, up to `MAX_RUNS` per device. The frames are owned by
//!   `mem::DMA_OWNER` in the ownership table, not by the process, and the run's budget pays for them
//!   directly, so no generic release, move or lend path can free or move one: they all check that the caller
//!   owns the frame.
//! - **The window**: each device's first register page, mapped for the kernel alone at `KERNEL_DMA_REGS +
//!   slot * PAGE_SIZE`, in tables the loader shared.
//! - **S's mapped half**: `Account::dma_mapped`, a bit per slot, set by `map_device`.

use redoubt_layout::Pid;
use redoubt_layout::{KERNEL_DMA_PAGES, KERNEL_DMA_REGS};
use redoubt_sys::{Error, PAGE_SIZE};

use crate::handle::BudgetRef;
use crate::mem::{DMA_OWNER, MemoryManager};

/// DMA devices the kernel can reset: one window page each.
pub const MAX_DMA_DEVICES: usize = KERNEL_DMA_PAGES;
const _: () = assert!(MAX_DMA_DEVICES <= u16::BITS as usize, "Account::dma_mapped is a u16");
/// Runs per device. A full table is `OutOfMemory` to that device's holders only.
const MAX_RUNS: usize = 32;

/// virtio-mmio registers (virtio 1.2, 4.2.2): the magic "virt", the version (1 legacy, 2
/// modern) and the device status, which a write of 0 resets.
const VIRTIO_MAGIC: usize = 0x000;
const VIRTIO_VERSION: usize = 0x004;
const VIRTIO_STATUS: usize = 0x070;
const MAGIC_VIRT: u32 = 0x7472_6976;

/// How long one reset may take to confirm, on the `time` CSR, with a backstop on the
/// number of status reads should time not advance.
const RESET_US: u64 = 1000;
const RESET_READS: u32 = 100_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    /// Held by a live process: `holder` is always `Some`.
    Live,
    /// Its process died and some device in its S did not confirm: never pooled.
    Quarantined,
}

#[derive(Clone, Copy)]
struct Run {
    phys: usize,
    npages: usize,
    /// The process holding it; `None` once it died.
    holder: Option<Pid>,
    /// The budget paying for its frames: the holder's at `dma_alloc`, then (quarantined) the
    /// destroyed budget's parent (kernel/devices.md, "Quarantine"); `None` once the root's tree
    /// is gone.
    charged: Option<BudgetRef>,
    state: State,
}

#[derive(Clone, Copy)]
struct Slot {
    base: u64,
    /// Whether the device answered as virtio-mmio at boot. Any other device is never reset,
    /// so every death that reaches it quarantines (kernel/devices.md, "Residual risks").
    virtio: bool,
    quarantined: bool,
    runs: [Option<Run>; MAX_RUNS],
}

pub struct Registry {
    slots: [Option<Slot>; MAX_DMA_DEVICES],
    /// A slot was quarantined and its device object not destroyed yet (`message.rs`,
    /// `destroy_quarantined_devices`).
    doomed: bool,
    /// `dma-reset-deaf`: the slots whose first reset has already been made to fail.
    #[cfg(feature = "dma-reset-deaf")]
    deaf_spent: u16,
}

impl Registry {
    pub const fn new() -> Registry {
        Registry {
            slots: [None; MAX_DMA_DEVICES],
            doomed: false,
            #[cfg(feature = "dma-reset-deaf")]
            deaf_spent: 0,
        }
    }
}

/// The slots whose bits are set in `mask`.
fn bits(mask: u16) -> impl Iterator<Item = usize> {
    (0..MAX_DMA_DEVICES).filter(move |i| mask & (1 << i) != 0)
}

/// One 32-bit register of slot `slot`'s device, `offset` bytes into its first page.
fn register(slot: usize, offset: usize) -> *mut u32 {
    (KERNEL_DMA_REGS + slot * PAGE_SIZE + offset) as *mut u32
}

fn read(slot: usize, offset: usize) -> u32 {
    // SAFETY: `dma_register` mapped this slot's window page, read-write and kernel-only, to the
    // device's first register page before the slot existed, and nothing else maps it or unmaps
    // it; `offset` is a 4-aligned register inside that page.
    unsafe { register(slot, offset).read_volatile() }
}

fn write(slot: usize, offset: usize, value: u32) {
    // SAFETY: as `read`.
    unsafe { register(slot, offset).write_volatile(value) }
}

impl MemoryManager {
    /// Boot: give DMA device `base` a slot, map its first register page into the window, and
    /// classify it with one read of its magic and version (the loader flags only virtio nodes
    /// as DMA today, so that read has no side effect). `false` if the registry is full.
    pub fn dma_register(&mut self, base: u64) -> bool {
        let Some(slot) = self.dma.slots.iter().position(Option::is_none) else { return false };
        crate::arch::mem::map_kernel_page(base as usize, KERNEL_DMA_REGS + slot * PAGE_SIZE);
        let virtio = read(slot, VIRTIO_MAGIC) == MAGIC_VIRT && matches!(read(slot, VIRTIO_VERSION), 1 | 2);
        self.dma.slots[slot] = Some(Slot { base, virtio, quarantined: false, runs: [None; MAX_RUNS] });
        true
    }

    /// The slot of DMA device `base`.
    pub fn dma_slot(&self, base: u64) -> Option<usize> {
        self.dma.slots.iter().position(|s| s.as_ref().is_some_and(|s| s.base == base))
    }

    fn slot_mut(&mut self, slot: usize) -> &mut Slot {
        self.dma.slots[slot].as_mut().expect("a registered slot")
    }

    /// Whether DMA device `base` failed a reset (kernel/devices.md, "Quarantine"). Its object is
    /// gone by the time any process runs again, so a live device object never names one.
    pub fn dma_quarantined(&self, base: u64) -> bool {
        self.dma_slot(base).is_some_and(|s| self.dma.slots[s].as_ref().is_some_and(|s| s.quarantined))
    }

    /// `map_device` of DMA device `slot`: it joins `pid`'s reset set.
    pub fn dma_mapped(&mut self, pid: Pid, slot: usize) {
        self.account_mut(pid).expect("a running process has an account").dma_mapped |= 1 << slot;
    }

    /// The process holding the Live run that frame `phys` belongs to, if it is a DMA frame.
    pub fn dma_holder(&self, phys: usize) -> Option<Pid> {
        self.dma.slots.iter().flatten().flat_map(|s| s.runs.iter().flatten()).find_map(|r| {
            let inside = phys >= r.phys && phys < r.phys + r.npages * PAGE_SIZE;
            (inside && r.state == State::Live).then_some(r.holder).flatten()
        })
    }

    /// `dma_alloc`'s memory: `npages` contiguous zeroed frames owned by `DMA_OWNER`, charged to
    /// `pid`'s budget directly (never through its frame ledger, which `uncharge_all_frames`
    /// empties before the reset), recorded as a Live run of `slot` held by `pid`. Nothing changes
    /// on failure.
    pub fn dma_new_run(&mut self, pid: Pid, slot: usize, npages: usize) -> Result<usize, Error> {
        let oom = Error::OutOfMemory;
        let index = self.slot_mut(slot).runs.iter().position(Option::is_none).ok_or(oom)?;
        let budget = self.budget_of(pid).ok_or(oom)?;
        self.charge(budget, npages as u64)?;
        let Ok(phys) = self.alloc_contiguous(DMA_OWNER, npages) else {
            self.uncharge(budget, npages as u64);
            return Err(oom);
        };
        let charged = Some(BudgetRef { frame: budget, id: self.budget_id(budget) });
        let run = Run { phys, npages, holder: Some(pid), charged, state: State::Live };
        self.slot_mut(slot).runs[index] = Some(run);
        Ok(phys)
    }

    /// Undo `dma_new_run` when mapping it failed: the address never reached the process, so the
    /// frames go straight back to the pool.
    pub fn dma_drop_run(&mut self, slot: usize, phys: usize) {
        let runs = &mut self.slot_mut(slot).runs;
        let index = runs.iter().position(|r| r.is_some_and(|r| r.phys == phys)).expect("the run just made");
        let run = runs[index].take().expect("the run just made");
        self.pool(run);
    }

    /// A run's frames back to the pool, and its charge back to its budget.
    fn pool(&mut self, run: Run) {
        self.free_contiguous(DMA_OWNER, run.phys, run.npages);
        if let Some(b) = run.charged.filter(|b| self.is_live_budget(*b)) {
            self.uncharge(b.frame, run.npages as u64);
        }
    }

    /// Process `pid` is ending (`Process::terminate`, after its own frames went and before its
    /// account is closed): reset its S. Only if every slot in S confirmed in this call are its
    /// runs pooled; otherwise all of them are quarantined, and every slot that did not confirm is
    /// quarantined too (an already-quarantined slot never counts as reset).
    pub fn dma_release(&mut self, pid: Pid) {
        let held = |r: &Option<Run>| r.is_some_and(|r| r.state == State::Live && r.holder == Some(pid));
        let mut s = self.account(pid).map_or(0, |a| a.dma_mapped);
        for (i, slot) in self.dma.slots.iter().enumerate() {
            if slot.as_ref().is_some_and(|slot| slot.runs.iter().any(held)) {
                s |= 1 << i;
            }
        }
        if s == 0 {
            return;
        }
        let confirmed = bits(s).filter(|&i| self.dma_reset(i)).fold(0u16, |m, i| m | 1 << i);
        let pooled = confirmed == s;
        for i in bits(s) {
            for index in 0..MAX_RUNS {
                let run = &mut self.slot_mut(i).runs[index];
                if !held(run) {
                    continue;
                }
                if pooled {
                    // Pooled only after every slot of S confirmed in this very call.
                    assert!(
                        confirmed & s == s && confirmed & 1 << i != 0,
                        "P1-1: a run pooled before its reset"
                    );
                    let run = run.take().expect("held");
                    self.pool(run);
                } else {
                    let run = run.as_mut().expect("held");
                    run.state = State::Quarantined;
                    run.holder = None;
                }
            }
        }
        for i in bits(s & !confirmed) {
            let slot = self.slot_mut(i);
            if !slot.quarantined {
                println!("DMA: device {:x} did not confirm its reset; quarantined until reboot", slot.base);
                slot.quarantined = true;
                self.dma.doomed = true;
            }
        }
    }

    /// Whether a slot was quarantined since the last call: its device object must now be
    /// destroyed, as R10 destroys one.
    pub fn dma_take_doomed(&mut self) -> bool { core::mem::take(&mut self.dma.doomed) }

    /// Reset slot `slot`'s device and wait for it to confirm, within `RESET_US`, without
    /// preemption. `false`, touching nothing, for a device that is quarantined or not virtio.
    fn dma_reset(&mut self, slot: usize) -> bool {
        let s = self.dma.slots[slot].as_ref().expect("a registered slot");
        if s.quarantined || !s.virtio {
            return false;
        }
        write(slot, VIRTIO_STATUS, 0);
        let deadline = crate::time::now_us() + RESET_US;
        let mut confirmed = false;
        for _ in 0..RESET_READS {
            if read(slot, VIRTIO_STATUS) == 0 {
                confirmed = true;
                break;
            }
            if crate::time::now_us() >= deadline {
                break;
            }
        }
        // Test builds only: the first reset of each device reports "not confirmed" after
        // the real write, so the quarantine path runs on a device that would reset.
        #[cfg(feature = "dma-reset-deaf")]
        if self.dma.deaf_spent & 1 << slot == 0 {
            self.dma.deaf_spent |= 1 << slot;
            confirmed = false;
        }
        confirmed
    }

    /// R10, after `top`'s carve went back to `parent`: every quarantined run charged to a
    /// dying budget is charged to `parent` instead. Those pages were part of the dying subtree's
    /// usage, at most `top`'s limit, which the parent just got back whole, so the charge cannot
    /// fail (I5). With no parent the charge ends with the tree.
    pub fn dma_migrate_quarantine(&mut self, parent: Option<crate::budget::BudgetFrame>) {
        let parent = parent.map(|p| BudgetRef { frame: p, id: self.budget_id(p) });
        for i in 0..MAX_DMA_DEVICES {
            for index in 0..MAX_RUNS {
                let Some(run) = self.dma.slots[i].as_ref().and_then(|s| s.runs[index]) else { continue };
                if run.state != State::Quarantined {
                    continue;
                }
                if !run.charged.is_some_and(|b| self.is_live_budget(b) && self.budget(b.frame).dying) {
                    continue;
                }
                if let Some(p) = parent {
                    self.charge(p.frame, run.npages as u64).expect("I5: the carve just returned covers it");
                }
                self.slot_mut(i).runs[index].as_mut().expect("read above").charged = parent;
            }
        }
    }

    /// Whether `pid` holds any run (a `process_create` rollback's child never may).
    pub fn dma_holds_any(&self, pid: Pid) -> bool {
        self.dma.slots.iter().flatten().flat_map(|s| s.runs.iter().flatten()).any(|r| r.holder == Some(pid))
    }
}
