//! Test-only (feature `test-support`, off in every target build): a range in memory, for the
//! servers' tests and for packing a volume on the host.

use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;

use super::{Fault, Geometry, Range, SECTOR};

const S: usize = SECTOR as usize;

/// A range in memory, sparse: a sector never written reads as zero, so a test can have a range of
/// gigabytes. Clones share the sectors, so a test can look at or break the disk under a running
/// server.
#[derive(Clone)]
pub struct Memory(pub Rc<RefCell<Disk>>);

pub struct Disk {
    /// The range's length in sectors.
    pub len: u64,
    /// The sectors that hold anything but zeros.
    pub sectors: BTreeMap<u64, Vec<u8>>,
    /// Every request fails from now on.
    pub failing: bool,
    /// Writes are refused, as `blkd` refuses them on a read-only range.
    pub read_only: bool,
    /// Writes that reached the disk.
    pub writes: usize,
    /// Power fails at this write: it and every later one are lost.
    pub fail_at: Option<usize>,
    /// Reads that reached the disk: one per `read`, and one per `read_at` whatever its span, as
    /// [`super::Blkd`] makes one call per span.
    pub reads: usize,
    /// Flushes that reached the disk.
    pub flushes: usize,
}

impl Memory {
    pub fn blank(sectors: usize) -> Memory {
        Memory(Rc::new(RefCell::new(Disk {
            len: sectors as u64,
            sectors: BTreeMap::new(),
            failing: false,
            read_only: false,
            writes: 0,
            fail_at: None,
            reads: 0,
            flushes: 0,
        })))
    }

    /// A range holding `bytes`, the last sector padded with zeros.
    pub fn holding(bytes: Vec<u8>) -> Memory {
        let disk = Memory::blank(bytes.len().div_ceil(S));
        disk.put(0, &bytes);
        disk
    }

    pub fn fail(&self) { self.0.borrow_mut().failing = true }

    /// The same bytes, now a read-only range.
    pub fn read_only(self) -> Memory {
        self.0.borrow_mut().read_only = true;
        self
    }

    /// Reads that reached the disk.
    pub fn reads(&self) -> usize { self.0.borrow().reads }

    /// The whole range, for a small one.
    pub fn bytes(&self) -> Vec<u8> { self.get(0, self.0.borrow().len as usize * S) }

    /// A copy of the disk as it is now, sharing nothing.
    pub fn copy(&self) -> Memory { Memory::holding(self.bytes()) }

    pub fn get(&self, sector: u64, len: usize) -> Vec<u8> {
        let disk = self.0.borrow();
        let mut out = vec![0; len];
        for (i, chunk) in out.chunks_mut(S).enumerate() {
            if let Some(data) = disk.sectors.get(&(sector + i as u64)) {
                chunk.copy_from_slice(&data[..chunk.len()]);
            }
        }
        out
    }

    /// Writes `data` from `sector`, whatever the range says of writing; a part of a sector at
    /// the end is padded with zeros.
    pub fn put(&self, sector: u64, data: &[u8]) {
        let mut disk = self.0.borrow_mut();
        for (i, chunk) in data.chunks(S).enumerate() {
            if chunk.iter().all(|b| *b == 0) {
                disk.sectors.remove(&(sector + i as u64));
            } else {
                let mut whole = chunk.to_vec();
                whole.resize(S, 0);
                disk.sectors.insert(sector + i as u64, whole);
            }
        }
    }

    fn check(&self, sector: u64, len: usize) -> Result<(), Fault> {
        let disk = self.0.borrow();
        let end = sector.checked_add((len / S) as u64).ok_or(Fault)?;
        if disk.failing || !len.is_multiple_of(S) || end > disk.len { Err(Fault) } else { Ok(()) }
    }
}

impl Range for Memory {
    fn info(&mut self) -> Result<Geometry, Fault> {
        let disk = self.0.borrow();
        if disk.failing {
            return Err(Fault);
        }
        Ok(Geometry { sectors: disk.len, read_only: disk.read_only })
    }

    fn read(&mut self, sector: u64, out: &mut [u8]) -> Result<(), Fault> {
        self.0.borrow_mut().reads += 1;
        self.check(sector, out.len())?;
        out.copy_from_slice(&self.get(sector, out.len()));
        Ok(())
    }

    fn write(&mut self, sector: u64, data: &[u8]) -> Result<(), Fault> {
        self.check(sector, data.len())?;
        let mut disk = self.0.borrow_mut();
        if disk.read_only || disk.fail_at.is_some_and(|at| disk.writes + 1 >= at) {
            return Err(Fault);
        }
        disk.writes += 1;
        drop(disk);
        self.put(sector, data);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Fault> {
        self.check(0, 0)?;
        self.0.borrow_mut().flushes += 1;
        Ok(())
    }

    /// One read whatever the span, as [`super::Blkd`] makes one call per span.
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> Result<(), Fault> {
        self.0.borrow_mut().reads += 1;
        let (sector, skip) = (at / u64::from(SECTOR), (at % u64::from(SECTOR)) as usize);
        let whole = (skip + out.len()).div_ceil(S) * S;
        self.check(sector, whole)?;
        out.copy_from_slice(&self.get(sector, whole)[skip..skip + out.len()]);
        Ok(())
    }
}
