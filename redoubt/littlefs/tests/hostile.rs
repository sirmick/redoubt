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

#[test]
fn geometry_mismatch_is_refused() {
    let cfg = Config { block_size: 256, block_count: 96, prog_size: 16 };
    let good = populated(cfg);
    let mut ram = Ram::from_image(Config { block_count: 32, ..cfg }, good[..256 * 32].to_vec());
    assert!(matches!(Filesystem::mount(&mut ram, Config { block_count: 32, ..cfg }), Err(Error::Invalid)));
    let mut ram = Ram::from_image(cfg, vec![0xff; 256 * 96]);
    assert!(matches!(Filesystem::mount(&mut ram, cfg), Err(Error::Corrupt)));
}
