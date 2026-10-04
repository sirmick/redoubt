//! One input through [`read`](crate::read) and [`check`](crate::check) against a fixed machine:
//! the body of the fuzz target (`fuzz/fuzz_targets/check.rs`) and of the host test that replays
//! its kept corpus (`fuzz/seeds/check`), so a campaign's findings rerun in every
//! `cargo test -p redoubt-init` (TENETS.md 6: "fuzz what parses"). Built only for tests and
//! fuzzing, never for the machine.

use alloc::vec::Vec;

use redoubt_rt::abi::{Handle, Usage};
use redoubt_sys::DeviceInfo;

use crate::check::Machine;
use crate::sshkey::KEY_LEN;

/// QEMU `virt`'s devices as `device_info` names them, in the kernel's handle order
/// (kernel/boot.md, "Devices handed to the first program"): the Reset right in slot 4, the
/// console's register region and interrupt in 5 and 6, the eight virtio-mmio slots (bus
/// masters), then their interrupts ascending.
pub fn virt_devices() -> Vec<(Handle, DeviceInfo)> {
    let mut answers = alloc::vec![
        DeviceInfo::Reset,
        DeviceInfo::Mmio { base: 0x1000_0000, size: 0x1000, dma: false },
        DeviceInfo::Irq(10),
    ];
    answers.extend((0..8u64).map(|n| DeviceInfo::Mmio {
        base: 0x1000_1000 + n * 0x1000,
        size: 0x1000,
        dma: true,
    }));
    answers.extend((1..=8).map(DeviceInfo::Irq));
    answers.into_iter().enumerate().filter_map(|(i, info)| Some((Handle::new(i as u32 + 4)?, info))).collect()
}

/// A budget's usage record with `free` pages, 15 processes and 250,000 weight free: `system`'s
/// at boot (kernel/budgets.md, "Root, system and users"), or `root`'s with `free` pages.
pub fn usage(free: u64) -> Usage {
    Usage {
        pages_limit: free + 100,
        pages_usage: 100,
        processes_limit: 15,
        processes_usage: 0,
        weight_limit: 250_000,
        weight_carved: 0,
    }
}

/// The bundle's entries the fuzz machine has: every program `image/boot.toml` packs, and one
/// data entry.
pub const ENTRIES: [(&str, usize); 11] = [
    ("manifest", 4096),
    ("keyd", 200_000),
    ("consoled", 150_000),
    ("bootfsd", 150_000),
    ("blkd", 150_000),
    ("netd", 150_000),
    ("ipd", 600_000),
    ("fsd", 300_000),
    ("beamlet", 4_000_000),
    ("trace", 100),
    ("system.index", 50_000),
];

/// The machine `init` would see on QEMU `virt` with the 1 GiB the image needs, with `devices`
/// and `entries`: `system` is a quarter of it (docs/kernel/budgets.md).
pub fn machine<'a>(devices: &'a [(Handle, DeviceInfo)], entries: &'a [(&'a str, usize)]) -> Machine<'a> {
    Machine {
        devices,
        system: usage(64_000),
        root: usage(1000),
        entries,
        stub_bytes: 16 * 1024,
        stack_pages: 16,
        arena_pages: crate::ARENA_PAGES,
        handles_at_start: 3 + devices.len(),
    }
}

/// A bundle key no manifest names.
pub const BUNDLE_KEY: [u8; KEY_LEN] = [0x42; KEY_LEN];

/// `data` as a manifest: never a panic, and a plan only within what the machine holds.
pub fn check_one(data: &[u8]) {
    let devices = virt_devices();
    let machine = machine(&devices, &ENTRIES);
    let Ok(manifest) = crate::read(data, machine.arena_pages) else { return };
    let Ok(plan) = crate::check(&manifest, &machine, BUNDLE_KEY) else { return };
    assert_eq!(plan.placements.len(), manifest.servers.len());
    // Every placed handle is one the kernel gave, and none is placed twice.
    let placed: Vec<Handle> = plan.placements.iter().flatten().map(|(_, h)| *h).collect();
    for (n, h) in placed.iter().enumerate() {
        assert!(devices.iter().any(|(d, info)| d == h && *info != DeviceInfo::Reset));
        assert!(!placed[..n].contains(h));
    }
    assert_eq!(plan.keys.last().map(|(_, key)| key), Some(&BUNDLE_KEY));
    let free = machine.root.pages_limit - machine.root.pages_usage;
    assert!(plan.bound <= free);
}
