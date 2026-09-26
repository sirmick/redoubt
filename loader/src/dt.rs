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
/// Distinct interrupt numbers the loader reports. The PLIC's source space is 10 bits, but a
/// machine with more wired sources than this would need a bigger argument block anyway.
pub const MAX_IRQ: usize = 32;
const MAX_SEED: usize = 64;

pub struct MmioRegion {
    pub range: Range<usize>,
    pub name: [u8; 4],
    /// The device is a bus master: a driver holding it may call `dma_alloc`
    /// (KERNEL-SPEC.md, Device; IO-ARCHITECTURE.md, DMA). Nothing in a device tree states
    /// this in general, so the platform's rule stands here: on the targets Redoubt supports
    /// the only bus masters are virtio devices, so a node whose `compatible` names virtio
    /// carries the flag and nothing else does. A platform that confines DMA in hardware
    /// (PLATFORM-FPGA.md) will state its own rule in the same place.
    pub dma: bool,
    /// Whether this is the console the device tree's `/chosen/stdout-path` names.
    pub console: bool,
    /// An interrupt controller: reported in `MREx` (the ownership table covers it) but never
    /// made into a device object, because it is the kernel's.
    pub kernel_only: bool,
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
    /// The hart's local interrupt controller (timer and software interrupts), found on its
    /// own rather than through the device list: the kernel is told this range so that it, not
    /// the exclusion below, decides that no device object may name it (QUESTIONS.md 143).
    pub clint: Option<Range<usize>>,
    pub mmio: [MmioRegion; MAX_MMIO],
    pub mmio_len: usize,
    /// Every interrupt number wired to a device the loader reports, in ascending order and
    /// without repeats. The hart timer is not here: it is a CPU resource, not a device
    /// (BOOT.md).
    pub irq: [u32; MAX_IRQ],
    pub irq_len: usize,
    /// The interrupt of the console named by `/chosen/stdout-path`, if it has one.
    pub console_irq: Option<u32>,
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

/// Whether the node's `compatible` list contains `needle` as a substring of any entry.
fn compatible_has(node: &Node, needle: &[u8]) -> bool {
    prop(node, "compatible").map_or(false, |b| b.windows(needle.len()).any(|w| w == needle))
}

/// An interrupt controller is the kernel's, never a userspace device: it decides which
/// source reaches the hart, so a process that could map it would own every interrupt.
/// The PLIC says so with `interrupt-controller`; the CLINT (the hart's own timer and
/// software interrupts, which the firmware and the kernel drive) does not, so it is named.
fn is_interrupt_controller(node: &Node) -> bool {
    prop(node, "interrupt-controller").is_some() || compatible_has(node, b"clint")
}

/// The node name of the console: the last component of `/chosen/stdout-path`
/// (`/soc/serial@10000000:115200` -> `serial@10000000`), with the `:options` suffix dropped.
/// `None` if the tree does not say, or says it with an `/aliases` name rather than a path:
/// every machine the bench boots writes the path (tenet 6).
fn console_name<'dt>(root: &Node<'_, '_, 'dt>) -> Option<&'dt str> {
    let chosen = root.children().find(|n| n.name() == Ok("chosen"))?;
    let raw = prop(&chosen, "stdout-path")?;
    let path = core::str::from_utf8(raw.strip_suffix(b"\0").unwrap_or(raw)).ok()?;
    let path = path.split(':').next().unwrap_or(path);
    path.rsplit('/').next()
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
            clint: None,
            mmio: core::array::from_fn(|_| MmioRegion {
                range: 0..0,
                name: *b"    ",
                dma: false,
                console: false,
                kernel_only: false,
            }),
            mmio_len: 0,
            irq: [0; MAX_IRQ],
            irq_len: 0,
            console_irq: None,
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
        let console = console_name(&root);
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
            let name = node.name().unwrap_or("");
            // Everything downstream works in whole pages: the page-ownership table indexes a
            // region by dividing, and a device object may name nothing but whole pages. A
            // region that does not start on one is dropped here, loudly, so that the kernel's
            // boot checks stay a check against a hostile argument block rather than a limit on
            // which machines boot.
            if base % redoubt_sys::PAGE_SIZE != 0 {
                crate::println!("  {} at {:#x} does not start on a page; skipped", name, base);
                continue;
            }
            // An interrupt controller belongs to the kernel, so it is not offered as a device
            // object; it stays in `MREx` (for the kernel's ownership table) and the kernel maps the PLIC for itself.
            let kernel_only = is_interrupt_controller(&node);
            let is_console = console == Some(name);
            // The interrupts a device raises. `#interrupt-cells` is 1 for the PLIC, which is
            // the only controller these platforms wire devices to; a wider one would need its
            // own decoding, so the loader takes the first cell of each entry and no more.
            if !kernel_only {
                for entry in prop(&node, "interrupts").into_iter().flat_map(|b| b.chunks_exact(4)) {
                    let irq = u32::from_be_bytes([entry[0], entry[1], entry[2], entry[3]]);
                    // Source 0 does not exist on a PLIC, and the kernel keeps number 0 for the
                    // hart timer (BOOT.md), so a node that asks for it is asking for something
                    // else: a controller with wider cells, or a tree we do not understand.
                    if irq == 0 {
                        crate::println!("  {} asks for interrupt 0; skipped", name);
                        continue;
                    }
                    if is_console {
                        platform.console_irq.get_or_insert(irq);
                    }
                    if !platform.irq[..platform.irq_len].contains(&irq) && platform.irq_len < MAX_IRQ {
                        platform.irq[platform.irq_len] = irq;
                        platform.irq_len += 1;
                    }
                }
            }
            if platform.mmio_len < MAX_MMIO {
                let mut tag = *b"    ";
                let len = name.len().min(4);
                tag[..len].copy_from_slice(&name.as_bytes()[..len]);
                platform.mmio[platform.mmio_len] = MmioRegion {
                    range: base..base + size,
                    name: tag,
                    dma: !kernel_only && compatible_has(&node, b"virtio"),
                    console: is_console && !kernel_only,
                    kernel_only,
                };
                platform.mmio_len += 1;
            }
        }
        platform.irq[..platform.irq_len].sort_unstable();

        platform.plic = read_plic(&idx, &root, ac, sc);
        platform.clint = idx.nodes().find(|n| compatible_has(n, b"clint")).and_then(|n| {
            let reg = prop(&n, "reg")?;
            let base = read_cells(reg, 0, ac) as usize;
            Some(base..base + read_cells(reg, ac, sc) as usize)
        });
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
