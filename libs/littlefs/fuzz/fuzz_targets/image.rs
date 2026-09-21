//! Fuzz target: an arbitrary image. The first byte picks a geometry, the rest is the disk.
//! Mounting it and running every operation must never panic or hang.
#![no_main]

#[path = "../../tests/common/mod.rs"]
#[allow(dead_code)]
mod common;
#[path = "../../tests/common/exercise.rs"]
mod exercise;

use common::Ram;
use libfuzzer_sys::fuzz_target;
use littlefs::{Config, Filesystem};

fuzz_target!(|data: &[u8]| {
    let Some((&g, disk)) = data.split_first() else { return };
    let (block_size, prog_size) = [(128, 1), (256, 16), (512, 8), (128, 16)][(g & 3) as usize];
    let block_count = 16 + (g >> 2) as u32 % 16;
    let cfg = Config { block_size, block_count, prog_size };
    let mut image = vec![0xff; (block_size * block_count) as usize];
    let n = disk.len().min(image.len());
    image[..n].copy_from_slice(&disk[..n]);
    let mut ram = Ram::from_image(cfg, image);
    ram.strict = false;
    if let Ok(mut fs) = Filesystem::mount(&mut ram, cfg) {
        exercise::exercise(&mut fs);
    }
});
