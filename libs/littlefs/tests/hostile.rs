//! Hostile images: valid volumes with corrupted bytes, and pure noise. Mounting and using
//! them must fail cleanly, never panic. (The coverage-guided fuzzer in `fuzz/` goes further;
//! this runs on every `cargo test`.)

mod common;
#[path = "common/exercise.rs"]
mod exercise;

use common::*;
use littlefs::{Config, Error, Filesystem};

/// A volume with some of everything in it.
fn populated(cfg: Config) -> Vec<u8> {
    let mut ram = Ram::new(cfg);
    Filesystem::format(&mut ram, cfg).unwrap();
    let mut fs = Filesystem::mount(&mut ram, cfg).unwrap();
    let mut rng = Rng(99);
    fs.mkdir("/d").unwrap();
    fs.mkdir("/d/e").unwrap();
    for i in 0..12 {
        let n = (i * 311) % 1200;
        write_file(&mut fs, &format!("/d/f{i}"), &rng.bytes(n)).unwrap();
    }
    fs.set_attr("/d/f1", 1, b"attr").unwrap();
    fs.rename("/d/f2", "/d/e/g").unwrap();
    fs.remove("/d/f3").unwrap();
    drop(fs);
    ram.data
}

fn try_image(cfg: Config, image: Vec<u8>) {
    let mut ram = Ram::from_image(cfg, image);
    ram.strict = false;
    if let Ok(mut fs) = Filesystem::mount(&mut ram, cfg) {
        exercise::exercise(&mut fs);
    }
}

#[test]
fn corrupted_bytes_never_panic() {
    let cfg = Config { block_size: 256, block_count: 96, prog_size: 16 };
    let good = populated(cfg);
    let mut rng = Rng(5);
    for _ in 0..3000 {
        let mut image = good.clone();
        for _ in 0..1 + rng.below(8) {
            let at = rng.below(image.len() as u64) as usize;
            match rng.below(3) {
                0 => image[at] ^= 1 << rng.below(8),
                1 => image[at] = rng.next() as u8,
                // Plausible small numbers, as block pointers and sizes.
                _ => {
                    let v = (rng.below(80) as u32).to_le_bytes();
                    let end = (at + 4).min(image.len());
                    image[at..end].copy_from_slice(&v[..end - at]);
                }
            }
        }
        try_image(cfg, image);
    }
}

#[test]
fn noise_never_panics() {
    let cfg = Config { block_size: 128, block_count: 16, prog_size: 1 };
    let mut rng = Rng(11);
    for _ in 0..2000 {
        try_image(cfg, rng.bytes(128 * 16));
    }
}

/// Hand-built hostile images (256-byte blocks, 64 of them, program size 16).
const RED_TEAM: Config = Config { block_size: 256, block_count: 64, prog_size: 16 };

/// Two entries named `x` in one pair: which one a lookup finds would depend on the
/// implementation, so the pair is refused.
#[test]
fn duplicate_names_are_corrupt() {
    let mut ram = Ram::from_image(RED_TEAM, include_bytes!("images/dup-names.img").to_vec());
    assert!(matches!(Filesystem::mount(&mut ram, RED_TEAM), Err(Error::Corrupt)));
}

/// A name with a NUL byte (written before names were checked for it): the volume mounts,
/// the check refuses it, and new NUL names are refused.
#[test]
fn nul_names_are_found_and_refused() {
    let mut ram = Ram::from_image(RED_TEAM, include_bytes!("images/nul-names.img").to_vec());
    let mut fs = Filesystem::mount(&mut ram, RED_TEAM).unwrap();
    assert_eq!(fs.check(), Err(Error::Corrupt));
    assert_eq!(fs.mkdir("/c\0d"), Err(Error::Invalid));
}

/// An inline file of 100 bytes, longer than this crate's inline limit (32 here) as another
/// writer may make: a two-byte write at 10 used to cut it to 12 bytes.
#[test]
fn oversized_inline_file_keeps_its_tail() {
    let mut ram = Ram::from_image(RED_TEAM, include_bytes!("images/oversized-inline.img").to_vec());
    let mut fs = Filesystem::mount(&mut ram, RED_TEAM).unwrap();
    let before = read_file(&mut fs, "/f").unwrap();
    assert_eq!(before.len(), 100);
    let h = fs.open("/f", littlefs::OpenOptions { read: true, write: true, ..Default::default() }).unwrap();
    fs.seek(h, 10).unwrap();
    fs.write(h, b"XY").unwrap();
    fs.close(h).unwrap();
    let mut want = before;
    want[10..12].copy_from_slice(b"XY");
    assert_eq!(read_file(&mut fs, "/f").unwrap(), want);
    fs.fsck().unwrap();
}

#[test]
fn geometry_mismatch_is_refused() {
    let cfg = Config { block_size: 256, block_count: 96, prog_size: 16 };
    let good = populated(cfg);
    let mut ram = Ram::from_image(Config { block_count: 32, ..cfg }, good[..256 * 32].to_vec());
    assert!(matches!(Filesystem::mount(&mut ram, Config { block_count: 32, ..cfg }), Err(Error::Invalid)));
    let mut ram = Ram::from_image(cfg, vec![0xff; 256 * 96]);
    assert!(matches!(Filesystem::mount(&mut ram, cfg), Err(Error::Corrupt)));
}
