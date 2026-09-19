//! Fuzz target: a valid, populated volume with a few bytes changed. Random images rarely get
//! past the metadata CRCs; small edits to a real image reach the deeper structures (tags,
//! pointers, skip-lists, global state), and a CRC can be fixed up by the edit list itself.
//!
//! Input: records of (offset: u16 little-endian, value: u8, mode: u8). Mode bit 0: XOR
//! instead of set. Mode bit 1: after the edit, recompute the CRC of the metadata commit
//! containing it, so the change passes the checksum and reaches the parser proper.
#![no_main]

#[path = "../../tests/common/mod.rs"]
#[allow(dead_code)]
mod common;
#[path = "../../tests/common/exercise.rs"]
mod exercise;

use std::sync::OnceLock;

use common::{write_file, Ram, Rng};
use libfuzzer_sys::fuzz_target;
use littlefs::{Config, Filesystem};

const CFG: Config = Config { block_size: 256, block_count: 48, prog_size: 16 };

/// Files inline and in skip-lists, nested and split directories, attributes, a rename.
fn populated() -> &'static [u8] {
    static IMAGE: OnceLock<Vec<u8>> = OnceLock::new();
    IMAGE.get_or_init(|| {
        let mut ram = Ram::new(CFG);
        Filesystem::format(&mut ram, CFG).unwrap();
        let mut fs = Filesystem::mount(&mut ram, CFG).unwrap();
        let mut rng = Rng(3);
        fs.mkdir("/d").unwrap();
        fs.mkdir("/d/e").unwrap();
        for i in 0..10 {
            write_file(&mut fs, &format!("/d/f{i}"), &rng.bytes(i * 170 % 900)).unwrap();
        }
        fs.set_attr("/d/f1", 1, b"attr").unwrap();
        fs.set_attr("/", 2, b"root").unwrap();
        fs.rename("/d/f2", "/d/e/g").unwrap();
        fs.remove("/d/f3").unwrap();
        drop(fs);
        ram.data
    })
}

/// Recomputes the CRC of the commit around `at` in its metadata block, if `at` is inside
/// one: walks the block's tags as the parser does and rewrites the CRC that covers `at`.
fn fix_crc(image: &mut [u8], at: usize) {
    let bs = CFG.block_size as usize;
    let block = &mut image[at / bs * bs..at / bs * bs + bs];
    let at = at % bs;
    let crc = |crc: u32, d: &[u8]| {
        let mut c = crc;
        for &b in d {
            c ^= b as u32;
            for _ in 0..8 {
                c = (c >> 1) ^ if c & 1 != 0 { 0xedb8_8320 } else { 0 };
            }
        }
        c
    };
    let dsize = |t: u32| 4 + if t & 0x3ff == 0x3ff { 0 } else { (t & 0x3ff) as usize };
    let (mut off, mut ptag, mut start) = (0usize, 0xffff_ffffu32, 0usize);
    loop {
        off += dsize(ptag);
        if off + 4 > bs {
            return;
        }
        let t = u32::from_be_bytes(block[off..off + 4].try_into().unwrap()) ^ ptag;
        if t & 0x8000_0000 != 0 || off + dsize(t) > bs {
            return;
        }
        ptag = t;
        if (t >> 20) & 0x780 == 0x500 {
            if dsize(t) < 8 {
                return;
            }
            let c = crc(0xffff_ffff, &block[start..off + 4]);
            if at < off + 8 {
                block[off + 4..off + 8].copy_from_slice(&c.to_le_bytes());
                return;
            }
            ptag ^= (((t >> 20) & 1) as u32) << 31;
            start = off + dsize(t);
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let mut image = populated().to_vec();
    for rec in data.chunks_exact(4).take(16) {
        let at = u16::from_le_bytes([rec[0], rec[1]]) as usize % image.len();
        if rec[3] & 1 != 0 {
            image[at] ^= rec[2];
        } else {
            image[at] = rec[2];
        }
        if rec[3] & 2 != 0 {
            fix_crc(&mut image, at);
        }
    }
    let mut ram = Ram::from_image(CFG, image);
    ram.strict = false;
    if let Ok(mut fs) = Filesystem::mount(&mut ram, CFG) {
        exercise::exercise(&mut fs);
    }
});
