//! Fuzz target: a valid, populated volume with a few bytes changed. Random images never get past
//! the superblock's hash; edits to a real volume reach every block, and an edit can forge the
//! hashes that cover it, so the change reaches the parser proper.
//!
//! Input: records of (offset: u32 little-endian, taken modulo the volume; value: u8; mode: u8).
//! Mode bit 0: XOR instead of set. Mode bit 1: after the edits, make the hash region's slot of
//! every edited block agree with it, and a self-hashed block's own hash (the superblock, the log's
//! header).
//!
//! Unforged, the property is the hash region's: the volume mounts and reads as it was, or what
//! reads a changed block is refused as corrupt; nothing else may change. Forged, only a panic or a
//! hang fails. Either way the volume is then walked, checked and written.
#![no_main]

#[path = "../../tests/common/mod.rs"]
#[allow(dead_code)]
mod common;

use std::sync::OnceLock;

use common::exercise::{READ_CAP, exercise};
use common::forge::agree;
use common::{Ram, Tree, populated, walk};
use libfuzzer_sys::fuzz_target;
use walfs::{BLOCK, Error, Filesystem};

/// A populated volume of 96 blocks (files of one block and of several, a sparse one past the
/// direct blocks), and what the walk reads of it.
fn volume() -> &'static (Vec<u8>, Tree) {
    static IMAGE: OnceLock<(Vec<u8>, Tree)> = OnceLock::new();
    IMAGE.get_or_init(|| {
        let image = populated(96, 8, 20_000, 60_000);
        let mut ram = Ram::from_image(image.clone());
        let tree = walk(&mut Filesystem::mount(&mut ram).unwrap(), READ_CAP).unwrap();
        (image, tree)
    })
}

fuzz_target!(|data: &[u8]| {
    let (good, tree) = volume();
    let mut image = good.clone();
    let mut edited = Vec::new();
    let mut forging = false;
    for rec in data.chunks_exact(6) {
        let at = u32::from_le_bytes([rec[0], rec[1], rec[2], rec[3]]) as usize % image.len();
        let (value, mode) = (rec[4], rec[5]);
        image[at] = if mode & 1 != 0 { image[at] ^ value } else { value };
        edited.push((at / BLOCK) as u32);
        forging |= mode & 2 != 0;
    }
    if forging {
        for b in edited {
            agree(&mut image, b);
        }
    }
    let mut ram = Ram::from_image(image);
    let Ok(mut fs) = Filesystem::mount(&mut ram) else { return };
    let seen = walk(&mut fs, READ_CAP);
    if !forging {
        assert!(matches!(&seen, Ok(t) if t == tree) || seen == Err(Error::Corrupt), "{seen:?}");
    }
    exercise(&mut fs);
});
