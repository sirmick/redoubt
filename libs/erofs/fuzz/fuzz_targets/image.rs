//! Fuzz target: an arbitrary volume, zero-filled to whole blocks. The superblock, every inode and
//! every directory block the walk reaches are parsed, and every file read whole, and the input's
//! first block is parsed as a directory block too: nothing may panic, hang or read out of bounds,
//! and a directory reached twice ends the walk as corrupt.
#![no_main]

use erofs::{BLOCK, Dirents, read_tree};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut image = data.to_vec();
    image.resize(data.len().next_multiple_of(BLOCK).max(BLOCK), 0);
    let _ = read_tree(&image);
    if let Ok(dirents) = Dirents::parse(&data[..data.len().min(BLOCK)]) {
        for entry in dirents.iter() {
            assert_eq!(dirents.lookup(entry.name), Some(entry));
        }
    }
});
