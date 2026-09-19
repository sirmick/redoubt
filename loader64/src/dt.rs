//! Device-tree access for the loader, over the `fdt-rs` parser.
//!
//! We use `fdt-rs` rather than the lighter `fdt` crate because `fdt` 0.1.5 mis-parses
//! valid trees that some firmwares emit (RustSBI re-serializes the tree and `fdt` then
//! cannot find `/chosen` or the memory node). `fdt-rs` handles them; per tenet 5, a
//! parser that fails on a valid tree is a bug in us.
//!
//! Everything the loader needs is extracted in one pass into `Platform`, so the rest of
//! the loader never touches the parser. We target QEMU `virt`, whose cell conventions
//! are fixed, but the cell widths are still read from the tree rather than assumed.

use core::ops::Range;

use fdt_rs::base::DevTree;
use fdt_rs::index::{DevTreeIndex, DevTreeIndexNode};
use fdt_rs::prelude::*;

pub const MAX_MMIO: usize = 32;
const MAX_SEED: usize = 64;

pub struct MmioRegion {
    pub range: Range<usize>,
    pub name: [u8; 4],
}

pub struct Plic {
    pub range: Range<usize>,
    /// Index of the (this hart, S-mode) context in the PLIC's `interrupts-extended`.
    pub context: usize,
}

/// Everything the loader reads from the device tree.
pub struct Platform {
    pub ram: Range<usize>,
    pub initrd: Range<usize>,
    pub rng_seed: [u8; MAX_SEED],
    pub rng_seed_len: usize,
    pub timebase_hz: u64,
    pub cpu_count: usize,
    pub plic: Option<Plic>,
    pub mmio: [MmioRegion; MAX_MMIO],
    pub mmio_len: usize,
    pub total_size: usize,
    pub dtb: usize,
}

type Node<'a, 'i, 'dt> = DevTreeIndexNode<'a, 'i, 'dt>;

fn prop<'dt>(node: &Node<'_, '_, 'dt>, name: &str) -> Option<&'dt [u8]> {
    node.props().find(|p| p.name().ok() == Some(name)).map(|p| p.raw())
}

/// Read `cells` big-endian 32-bit words from `bytes` at word `i` as one integer.
fn read_cells(bytes: &[u8], i: usize, cells: usize) -> u64 {
    (0..cells).fold(0u64, |acc, k| {
        let o = (i + k) * 4;
        (acc << 32) | u32::from_be_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]) as u64
    })
}

fn cell(bytes: Option<&[u8]>) -> Option<u64> {
    let b = bytes?;
    Some(read_cells(b, 0, b.len() / 4))
}

fn is_memory(node: &Node) -> bool {
    prop(node, "device_type").map_or(false, |b| b.strip_suffix(b"\0") == Some(b"memory"))
}

impl Platform {
    /// # Safety
    /// `dtb` must point at a device-tree blob (the SBI boot protocol's `a1`).
    pub unsafe fn read(dtb: usize) -> Platform {
        // SAFETY: forwarded from the caller.
        let dt = unsafe { DevTree::from_raw_pointer(dtb as *const u8) }.expect("invalid device tree");
        let total_size = dt.totalsize();

        // The index needs a scratch buffer. One static buffer, sized for large trees.
        static mut INDEX_BUF: [u8; 512 * 1024] = [0; 512 * 1024];
        // SAFETY: the loader is single-threaded and reads the tree once at boot, so this
        // buffer has no other user.
        let buf = unsafe { &mut *core::ptr::addr_of_mut!(INDEX_BUF) };
        let idx = DevTreeIndex::new(dt, buf).expect("device tree too large for the index buffer");
        let root = idx.root();

        let ac = cell(prop(&root, "#address-cells")).unwrap_or(2) as usize;
        let sc = cell(prop(&root, "#size-cells")).unwrap_or(2) as usize;

        let mut platform = Platform {
            ram: 0..0,
            initrd: 0..0,
            rng_seed: [0; MAX_SEED],
            rng_seed_len: 0,
            timebase_hz: 0,
            cpu_count: 0,
            plic: None,
            mmio: core::array::from_fn(|_| MmioRegion { range: 0..0, name: *b"    " }),
            mmio_len: 0,
            total_size,
            dtb,
        };

        // Main memory.
        let memory = root.children().find(|n| is_memory(n)).expect("no memory node");
        let reg = prop(&memory, "reg").expect("memory node has no reg");
        let base = read_cells(reg, 0, ac) as usize;
        platform.ram = base..base + read_cells(reg, ac, sc) as usize;

        // /chosen: initrd and rng-seed.
        let chosen = root.children().find(|n| n.name() == Ok("chosen")).expect("no /chosen node");
        let start = cell(prop(&chosen, "linux,initrd-start")).expect("no initrd-start") as usize;
        let end = cell(prop(&chosen, "linux,initrd-end")).expect("no initrd-end") as usize;
        platform.initrd = start..end;
        if let Some(seed) = prop(&chosen, "rng-seed") {
            let n = seed.len().min(MAX_SEED);
            platform.rng_seed[..n].copy_from_slice(&seed[..n]);
            platform.rng_seed_len = n;
        }

        // CPUs: timebase and count.
        if let Some(cpus) = root.children().find(|n| n.name() == Ok("cpus")) {
            platform.timebase_hz = cell(prop(&cpus, "timebase-frequency")).unwrap_or(0);
            platform.cpu_count = cpus.children().filter(|n| n.name().unwrap_or("").starts_with("cpu@")).count();
        }

        // MMIO device regions: any node with a reg that lies outside RAM. QEMU virt keeps
        // these directly under the root and under /soc, both with the root's cell widths.
        let soc = root.children().find(|n| n.name() == Ok("soc"));
        let devices = root.children().chain(soc.into_iter().flat_map(|s| s.children()));
        for node in devices {
            if is_memory(&node) {
                continue;
            }
            let Some(reg) = prop(&node, "reg") else { continue };
            if reg.len() < (ac + sc) * 4 {
                continue;
            }
            let base = read_cells(reg, 0, ac) as usize;
            let size = read_cells(reg, ac, sc) as usize;
            if size == 0 || (base >= platform.ram.start && base < platform.ram.end) {
                continue;
            }
            if platform.mmio_len < MAX_MMIO {
                let name = node.name().unwrap_or("");
                let mut tag = *b"    ";
                let len = name.len().min(4);
                tag[..len].copy_from_slice(&name.as_bytes()[..len]);
                platform.mmio[platform.mmio_len] = MmioRegion { range: base..base + size, name: tag };
                platform.mmio_len += 1;
            }
        }

        platform.plic = read_plic(&idx, &root, ac, sc);
        platform
    }

    pub fn mmio(&self) -> &[MmioRegion] { &self.mmio[..self.mmio_len] }

    pub fn rng_seed(&self) -> &[u8] { &self.rng_seed[..self.rng_seed_len] }
}

/// Locate the PLIC and the S-mode context wired to the boot hart (hart 0).
///
/// `interrupts-extended` is a list of (hart-interrupt-controller phandle, hart interrupt
/// number) pairs, one per context in order. Supervisor external interrupt is number 9.
fn read_plic(idx: &DevTreeIndex, root: &Node, _ac: usize, _sc: usize) -> Option<Plic> {
    const SUPERVISOR_EXTERNAL: u32 = 9;
    let plic = idx.nodes().find(|n| prop(n, "compatible").map_or(false, |b| b.windows(4).any(|w| w == b"plic")))?;
    let reg = prop(&plic, "reg")?;
    let base = read_cells(reg, 0, _ac) as usize;
    let range = base..base + read_cells(reg, _ac, _sc) as usize;

    // Boot hart is hart 0. Find its interrupt-controller child's phandle.
    let cpus = root.children().find(|n| n.name() == Ok("cpus"))?;
    let boot_phandle = cpus.children().find_map(|cpu| {
        if cell(prop(&cpu, "reg"))? != 0 {
            return None;
        }
        let intc = cpu.children().find(|c| c.name().unwrap_or("").starts_with("interrupt-controller"))?;
        Some(cell(prop(&intc, "phandle"))? as u32)
    })?;

    let extended = prop(&plic, "interrupts-extended")?;
    let context = extended.chunks_exact(8).position(|pair| {
        let phandle = u32::from_be_bytes(pair[..4].try_into().unwrap());
        let irq = u32::from_be_bytes(pair[4..].try_into().unwrap());
        phandle == boot_phandle && irq == SUPERVISOR_EXTERNAL
    })?;
    Some(Plic { range, context })
}
