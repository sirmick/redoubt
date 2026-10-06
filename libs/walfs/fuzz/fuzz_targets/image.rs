//! Fuzz target: an arbitrary image, zero-filled to whole blocks, at least the smallest volume,
//! with its superblock's own hash and slot made to agree, so the mount reaches the field checks.
//! Mounting it, walking it, checking it and changing it must never panic or hang.
#![no_main]

#[path = "../../tests/common/mod.rs"]
#[allow(dead_code)]
mod common;

use common::Ram;
use common::exercise::exercise;
use common::forge::agree;
use libfuzzer_sys::fuzz_target;
use walfs::{BLOCK, Filesystem};

fuzz_target!(|data: &[u8]| {
    let mut image = data.to_vec();
    image.resize(data.len().next_multiple_of(BLOCK).max(40 * BLOCK), 0);
    agree(&mut image, 0);
    let mut ram = Ram::from_image(image);
    if let Ok(mut fs) = Filesystem::mount(&mut ram) {
        exercise(&mut fs);
    }
});
