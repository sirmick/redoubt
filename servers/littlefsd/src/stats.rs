//! Test-only, under `boot-stats` and off in every default build: what one `littlefsd` did
//! through a boot, for the bench's `boot-profile` cases (docs/testbench.md, "Checked builds"). One
//! instance serves one volume on one thread, so the counts are statics; they are said in one line
//! at each power of two of 9P requests from 2^12, and once more, exactly, on a walk of
//! [`SENTINEL`].
//!
//! littlefs's reads are told apart by their shape, below the library: a 4-byte read is a word, a
//! block pointer `ctz_find` follows or one of the two revision counts a metadata fetch reads
//! first; a whole block from offset 0 is a metadata fetch's block (or a file's first block read
//! whole); anything else is a piece of a file's data.

use alloc::format;
use alloc::string::String;
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

/// A name no volume holds, whose walk says the counts exactly: beamlet looks it up for a call to
/// `BootStats`, typed at the prompt.
pub const SENTINEL: &str = "Elixir.BootStats.beam";

/// The first power of two of requests that says the counts.
const FIRST: u32 = 1 << 12;

/// The blocks told apart for the distinct count; a read past them is counted, not told apart.
const TRACKED: usize = 1 << 15;

/// The 9P operations counted.
#[derive(Clone, Copy)]
pub enum Op {
    Walk,
    Open,
    Read,
    Clunk,
    Stat,
}

static OPS: [AtomicU32; 5] = [const { AtomicU32::new(0) }; 5];
static READ_BYTES: AtomicU32 = AtomicU32::new(0);
static WORDS: AtomicU32 = AtomicU32::new(0);
static WHOLE: AtomicU32 = AtomicU32::new(0);
static PIECES: AtomicU32 = AtomicU32::new(0);
static BLOCK_BYTES: AtomicU32 = AtomicU32::new(0);
static DISTINCT: AtomicU32 = AtomicU32::new(0);
static SEEN: [AtomicU32; TRACKED / 32] = [const { AtomicU32::new(0) }; TRACKED / 32];

/// Counts one 9P operation; true if the requests so far are a power of two to say the counts at.
pub fn op(op: Op) -> bool {
    OPS[op as usize].fetch_add(1, Relaxed);
    let n: u32 = OPS.iter().map(|c| c.load(Relaxed)).sum();
    n >= FIRST && n.is_power_of_two()
}

/// Counts `n` bytes a 9P read returned.
pub fn read_bytes(n: usize) { READ_BYTES.fetch_add(n as u32, Relaxed); }

/// Counts one littlefs read of `len` bytes at `off` in `block`, of a block of `block_size`.
pub fn block_read(block: u32, off: u32, len: usize, block_size: u32) {
    let kind = match len {
        4 => &WORDS,
        _ if off == 0 && len == block_size as usize => &WHOLE,
        _ => &PIECES,
    };
    kind.fetch_add(1, Relaxed);
    BLOCK_BYTES.fetch_add(len as u32, Relaxed);
    let b = block as usize;
    if b < TRACKED {
        let bit = 1 << (b % 32);
        if SEEN[b / 32].fetch_or(bit, Relaxed) & bit == 0 {
            DISTINCT.fetch_add(1, Relaxed);
        }
    }
}

/// The counts, as one console line.
pub fn line() -> String {
    let [walk, open, read, clunk, stat] = OPS.each_ref().map(|c| c.load(Relaxed));
    let (words, whole, pieces) = (WORDS.load(Relaxed), WHOLE.load(Relaxed), PIECES.load(Relaxed));
    format!(
        "littlefsd: boot-stats: 9P {} (walk {walk}, open {open}, read {read}, clunk {clunk}, stat {stat}), \
         {} bytes read; littlefs reads {} (words {words}, whole blocks {whole}, pieces {pieces}), \
         {} bytes, {} distinct blocks\n",
        walk + open + read + clunk + stat,
        READ_BYTES.load(Relaxed),
        words + whole + pieces,
        BLOCK_BYTES.load(Relaxed),
        DISTINCT.load(Relaxed),
    )
}
