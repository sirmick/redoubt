//! A forger's tools: making a changed block agree with its hashes, so the change reaches the
//! parser. The slot arithmetic is the page's ("The hash region"): every block but the log's has a
//! slot, slot `i` is block `i`'s for the superblock and block `i + 33`'s for the rest, 127 to a
//! hash block, whose last 32 bytes are its own hash.

use sha2::{Digest, Sha256};
use walfs::BLOCK;

/// The log's blocks, which have no slots.
const LOG: u32 = 33;
const HASH_SLOTS: u32 = 127;

fn hash_start(img: &[u8]) -> u32 { u32::from_le_bytes(img[40..44].try_into().unwrap()) }

/// Makes a self-hashed block's own hash (the superblock's, the log header's, a hash block's) agree
/// with it.
pub fn seal(img: &mut [u8], b: u32) {
    let blk = &mut img[b as usize * BLOCK..(b as usize + 1) * BLOCK];
    let h: [u8; 32] = Sha256::digest(&blk[..BLOCK - 32]).into();
    blk[BLOCK - 32..].copy_from_slice(&h);
}

/// Makes block `b`'s slot agree with it, and the slot's hash block's own hash, with the hash region
/// at `hash` (for a superblock that says otherwise).
pub fn rehash_at(img: &mut [u8], b: u32, hash: u32) {
    let at = b as usize * BLOCK;
    let h: [u8; 32] = Sha256::digest(&img[at..at + BLOCK]).into();
    let i = if b == 0 { 0 } else { b - LOG };
    let slot = (hash + i / HASH_SLOTS) as usize * BLOCK + (i % HASH_SLOTS) as usize * 32;
    if let Some(s) = img.get_mut(slot..slot + 32) {
        s.copy_from_slice(&h);
        seal(img, hash + i / HASH_SLOTS);
    }
}

/// Makes block `b`'s slot agree with it, with the hash region where the superblock says.
pub fn rehash(img: &mut [u8], b: u32) {
    let hash = hash_start(img);
    rehash_at(img, b, hash);
}

/// Makes block `b` agree with every hash that covers it: its own if it keeps one, and its slot if
/// it has one.
pub fn agree(img: &mut [u8], b: u32) {
    let bitmap = u32::from_le_bytes(img[44..48].try_into().unwrap());
    if b <= 1 || (hash_start(img)..bitmap).contains(&b) {
        seal(img, b);
    }
    if !(1..1 + LOG).contains(&b) {
        rehash(img, b);
    }
}
