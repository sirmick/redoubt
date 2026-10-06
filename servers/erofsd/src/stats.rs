//! Test-only, under `boot-stats` and off in every default build: what one `erofsd` did through a
//! boot, for the bench's `boot-profile` cases (docs/testbench.md, "Checked builds"). One instance
//! serves one volume on one thread, so the counts are statics; they are said in one line at each
//! power of two of 9P requests from 2^12, and once more, exactly, on a walk of [`SENTINEL`].
//!
//! The range's reads are told apart by what they are for: an inode, a directory block, or a
//! file's bytes; and counted again as the calls the range client made, with their bytes.

use alloc::format;
use alloc::string::String;
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

/// A name no volume holds, whose walk says the counts exactly: beamlet looks it up for a call to
/// `BootStats`, typed at the prompt.
pub const SENTINEL: &str = "Elixir.BootStats.beam";

/// The first power of two of requests that says the counts.
const FIRST: u32 = 1 << 12;

/// The 9P operations counted.
#[derive(Clone, Copy)]
pub enum Op {
    Walk,
    Open,
    Read,
    Clunk,
    Stat,
}

/// What a range read was for.
#[derive(Clone, Copy)]
pub enum For {
    Inode,
    Directory,
    Data,
}

static OPS: [AtomicU32; 5] = [const { AtomicU32::new(0) }; 5];
static READ_BYTES: AtomicU32 = AtomicU32::new(0);
static FOR: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];
static CALLS: AtomicU32 = AtomicU32::new(0);
static CALL_BYTES: AtomicU32 = AtomicU32::new(0);

/// Counts one 9P operation; true if the requests so far are a power of two to say the counts at.
pub fn op(op: Op) -> bool {
    OPS[op as usize].fetch_add(1, Relaxed);
    let n: u32 = OPS.iter().map(|c| c.load(Relaxed)).sum();
    n >= FIRST && n.is_power_of_two()
}

/// Counts `n` bytes a 9P read returned.
pub fn read_bytes(n: usize) { READ_BYTES.fetch_add(n as u32, Relaxed); }

/// Counts one read of the volume, for `what`.
pub fn read_for(what: For) { FOR[what as usize].fetch_add(1, Relaxed); }

/// Counts one call to the range, of `bytes`.
pub fn call(bytes: usize) {
    CALLS.fetch_add(1, Relaxed);
    CALL_BYTES.fetch_add(bytes as u32, Relaxed);
}

/// The counts, as one console line.
pub fn line() -> String {
    let [walk, open, read, clunk, stat] = OPS.each_ref().map(|c| c.load(Relaxed));
    let [inodes, dirs, data] = FOR.each_ref().map(|c| c.load(Relaxed));
    format!(
        "erofsd: boot-stats: 9P {} (walk {walk}, open {open}, read {read}, clunk {clunk}, stat {stat}), \
         {} bytes read; volume reads {} (inodes {inodes}, directory blocks {dirs}, data {data}); \
         range calls {}, {} bytes\n",
        walk + open + read + clunk + stat,
        READ_BYTES.load(Relaxed),
        inodes + dirs + data,
        CALLS.load(Relaxed),
        CALL_BYTES.load(Relaxed),
    )
}
