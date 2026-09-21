//! Memory accounting: how much a process (or an ETS object) holds, in BEAM's units (64-bit
//! words).
//!
//! Terms live on per-process heaps, so memory is read off them, not measured: a heap cell is
//! two words, and the bytes a heap holds off-heap (binaries, bignums) are counted separately,
//! as BEAM counts its reference-counted binaries.

use crate::process::Process;

/// What a process holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Heap words (including garbage not yet collected), stack and registers.
    pub words: u64,
    /// Bytes held off the heap.
    pub binary_bytes: u64,
}

impl Usage {
    /// Everything, in words: what resource limits compare against.
    pub fn total_words(&self) -> u64 {
        self.words.saturating_add(self.binary_bytes.div_ceil(8))
    }
}

/// Everything process `p` holds.
pub fn process(p: &Process) -> Usage {
    let fixed = PROCESS_WORDS + p.x.len() as u64 + 2 * p.stack.len() as u64 + p.frames.len() as u64;
    Usage {
        words: fixed + 2 * p.heap.len() as u64,
        binary_bytes: p.heap.offheap_bytes() as u64,
    }
}

/// A process's own structures (BEAM's process struct and minimum heap), in words.
pub const PROCESS_WORDS: u64 = 338;
