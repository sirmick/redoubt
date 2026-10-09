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
use redoubt_layout::MAX_HARTS;

/// MMIO regions the loader reports, the interrupt controller's included. A tree with more stops
/// the boot (kernel/boot.md, "Hardware bounds"): dropping one would boot without a device.
pub const MAX_MMIO: usize = 32;
/// Distinct interrupt numbers the loader reports. The PLIC's source space is 10 bits, but a
/// machine with more wired sources than this would need a bigger argument block anyway, so a tree
/// with more stops the boot, as one with more MMIO regions does.
pub const MAX_IRQ: usize = 32;
const MAX_SEED: usize = 64;

/// Where a PLIC's per-context registers (threshold and claim) begin, and each context's stride:
/// the RISC-V PLIC specification's layout, which QEMU's `virt` follows.
const PLIC_CONTEXT_BASE: usize = 0x20_0000;
const PLIC_CONTEXT_STRIDE: usize = 0x1000;

pub struct MmioRegion {
    pub range: Range<usize>,
    pub name: [u8; 4],
    /// The device is a bus master: a driver holding it may call `dma_alloc`
    /// (kernel/devices.md, "Device objects"). Nothing in a device tree states this in general,
    /// so the platform's rule stands here: on the targets Redoubt supports the only bus masters
    /// are virtio devices, so a node whose `compatible` names virtio carries the flag and
    /// nothing else does. A platform that confines DMA in hardware (beyond/fpga-platform.md)
    /// will state its own rule in the same place.
    pub dma: bool,
    /// Whether this is the console the device tree's `/chosen/stdout-path` names.
    pub console: bool,
    /// An interrupt controller: reported in `MREx` (the ownership table covers it) but never
    /// made into a device object, because it is the kernel's.
    pub kernel_only: bool,
}

pub struct Plic {
    pub range: Range<usize>,
    /// Index of the (boot hart, S-mode) context in the PLIC's `interrupts-extended`.
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
    /// The harts the kernel starts, by boot index: the boot hart first, then the others in tree
    /// order, at most `MAX_HARTS`. A hart past it, or whose PLIC S-mode context is not inside
    /// the PLIC's window, is left out and stays parked (kernel/boot.md, `Hart`).
    pub harts: [usize; MAX_HARTS],
    /// Each listed hart's PLIC S-mode context, by boot index; 0 for all without a PLIC.
    pub contexts: [usize; MAX_HARTS],
    pub harts_len: usize,
    pub plic: Option<Plic>,
    /// The hart's local interrupt controller (timer and software interrupts), found on its
    /// own rather than through the device list: the kernel is told this range so that it, not
    /// the exclusion below, decides that no device object may name it (kernel/boot.md).
    pub clint: Option<Range<usize>>,
    pub mmio: [MmioRegion; MAX_MMIO],
    pub mmio_len: usize,
    /// Every interrupt number wired to a device the loader reports, in ascending order and
    /// without repeats. The hart timer is not here: it is a CPU resource, not a device
    /// (kernel/boot.md, "Hardware abstraction").
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
    /// Read the tree at `dtb` for a boot on hart `hart`.
    ///
    /// # Safety
    /// `dtb` must point at a device-tree blob (the SBI boot protocol's `a1`).
    pub unsafe fn read(dtb: usize, hart: usize) -> Platform {
        // SAFETY: forwarded from the caller.
        let dt = unsafe { DevTree::from_raw_pointer(dtb as *const u8) }.expect("invalid device tree");
        // The index needs a scratch buffer. One static buffer, sized for large trees.
        static mut INDEX_BUF: [u8; 512 * 1024] = [0; 512 * 1024];
        // SAFETY: the loader is single-threaded and reads the tree once at boot, so this
        // buffer has no other user.
        let buf = unsafe { &mut *core::ptr::addr_of_mut!(INDEX_BUF) };
        Platform::parse(dt, dtb, hart, buf)
    }

    /// Everything the loader needs from `dt`, found at `dtb`, for a boot on hart `hart`, with
    /// `buf` as the index's scratch space.
    pub fn parse(dt: DevTree, dtb: usize, hart: usize, buf: &mut [u8]) -> Platform {
        let total_size = dt.totalsize();
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
            harts: [0; MAX_HARTS],
            contexts: [0; MAX_HARTS],
            harts_len: 0,
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
            platform.cpu_count =
                cpus.children().filter(|n| n.name().unwrap_or("").starts_with("cpu@")).count();
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
            // object; it stays in `MREx` (for the kernel's ownership table) and the kernel maps the PLIC for
            // itself.
            let kernel_only = is_interrupt_controller(&node);
            let is_console = console == Some(name);
            // The interrupts a device raises. `#interrupt-cells` is 1 for the PLIC, which is
            // the only controller these platforms wire devices to; a wider one would need its
            // own decoding, so the loader takes the first cell of each entry and no more.
            if !kernel_only {
                for entry in prop(&node, "interrupts").into_iter().flat_map(|b| b.chunks_exact(4)) {
                    let irq = u32::from_be_bytes([entry[0], entry[1], entry[2], entry[3]]);
                    // Source 0 does not exist on a PLIC, and the kernel keeps number 0 for the
                    // hart timer (kernel/boot.md), so a node that asks for it is asking for
                    // something else: a controller with wider cells, or a tree we do not
                    // understand.
                    if irq == 0 {
                        crate::println!("  {} asks for interrupt 0; skipped", name);
                        continue;
                    }
                    if is_console {
                        platform.console_irq.get_or_insert(irq);
                    }
                    if !platform.irq[..platform.irq_len].contains(&irq) {
                        assert!(platform.irq_len < MAX_IRQ, "interrupt {irq} is past the loader's {MAX_IRQ}");
                        platform.irq[platform.irq_len] = irq;
                        platform.irq_len += 1;
                    }
                }
            }
            assert!(platform.mmio_len < MAX_MMIO, "MMIO region {name} is past the loader's {MAX_MMIO}");
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
        platform.irq[..platform.irq_len].sort_unstable();

        platform.plic = read_plic(&idx, &root, ac, sc, hart);
        platform.harts[0] = hart;
        platform.contexts[0] = platform.plic.as_ref().map_or(0, |plic| plic.context);
        platform.harts_len = 1;
        let plic_node = plic_node(&idx);
        let cpus = root.children().find(|n| n.name() == Ok("cpus"));
        for cpu in
            cpus.iter().flat_map(|c| c.children()).filter(|n| n.name().unwrap_or("").starts_with("cpu@"))
        {
            let Some(id) = cell(prop(&cpu, "reg")) else { continue };
            if id == hart as u64 || platform.harts_len == MAX_HARTS {
                continue;
            }
            // Every hart started has an S-mode context the kernel's PLIC window reaches.
            let mut context = 0;
            if let (Some(plic), Some(node)) = (&platform.plic, &plic_node) {
                match s_context(node, &root, id) {
                    Some(c) if context_in_window(c, plic.range.len()) => context = c,
                    _ => continue,
                }
            }
            platform.harts[platform.harts_len] = id as usize;
            platform.contexts[platform.harts_len] = context;
            platform.harts_len += 1;
        }
        platform.clint = idx.nodes().find(|n| compatible_has(n, b"clint")).and_then(|n| {
            let reg = prop(&n, "reg")?;
            let base = read_cells(reg, 0, ac) as usize;
            Some(base..base + read_cells(reg, ac, sc) as usize)
        });
        platform
    }

    pub fn mmio(&self) -> &[MmioRegion] { &self.mmio[..self.mmio_len] }

    pub fn rng_seed(&self) -> &[u8] { &self.rng_seed[..self.rng_seed_len] }

    pub fn harts(&self) -> &[usize] { &self.harts[..self.harts_len] }

    /// The listed harts' PLIC S-mode contexts, by boot index ([`Self::harts`]).
    pub fn contexts(&self) -> &[usize] { &self.contexts[..self.harts_len] }
}

/// Whether PLIC context `context`'s registers lie inside a PLIC window of `size` bytes.
fn context_in_window(context: usize, size: usize) -> bool {
    context
        .checked_add(1)
        .and_then(|n| n.checked_mul(PLIC_CONTEXT_STRIDE))
        .and_then(|n| n.checked_add(PLIC_CONTEXT_BASE))
        .is_some_and(|end| end <= size)
}

fn plic_node<'a, 'i, 'dt>(idx: &'a DevTreeIndex<'i, 'dt>) -> Option<Node<'a, 'i, 'dt>> {
    idx.nodes().find(|n| prop(n, "compatible").map_or(false, |b| b.windows(4).any(|w| w == b"plic")))
}

/// The index of hart `hart`'s S-mode context in `plic`'s `interrupts-extended`: the pair naming
/// the interrupt controller of the cpu whose `reg` is `hart`, with interrupt 9.
fn s_context(plic: &Node, root: &Node, hart: u64) -> Option<usize> {
    const SUPERVISOR_EXTERNAL: u32 = 9;
    let cpus = root.children().find(|n| n.name() == Ok("cpus"))?;
    let phandle = cpus.children().find_map(|cpu| {
        if cell(prop(&cpu, "reg"))? != hart {
            return None;
        }
        let intc = cpu.children().find(|c| c.name().unwrap_or("").starts_with("interrupt-controller"))?;
        Some(cell(prop(&intc, "phandle"))? as u32)
    })?;
    let extended = prop(plic, "interrupts-extended").unwrap_or(&[]);
    extended.chunks_exact(8).position(|pair| {
        let p = u32::from_be_bytes([pair[0], pair[1], pair[2], pair[3]]);
        let irq = u32::from_be_bytes([pair[4], pair[5], pair[6], pair[7]]);
        p == phandle && irq == SUPERVISOR_EXTERNAL
    })
}

/// Locate the PLIC and the S-mode context wired to the boot hart, the cpu whose `reg` is
/// `hart` (the hart ID the firmware passed in `a0`). `None` if the tree has no PLIC.
///
/// `interrupts-extended` is a list of (hart-interrupt-controller phandle, hart interrupt
/// number) pairs, one per context in order. Supervisor external interrupt is number 9. A PLIC
/// with no such context for the boot hart stops the boot (kernel/boot.md, R17): booting on
/// would leave every driver deaf, or enable interrupts on a hart that never takes them.
fn read_plic(idx: &DevTreeIndex, root: &Node, ac: usize, sc: usize, hart: usize) -> Option<Plic> {
    let plic = plic_node(idx)?;
    let reg = prop(&plic, "reg").expect("the PLIC has no reg");
    let base = read_cells(reg, 0, ac) as usize;
    let range = base..base + read_cells(reg, ac, sc) as usize;
    let context = s_context(&plic, root, hart as u64)
        .unwrap_or_else(|| panic!("the PLIC has no S-mode context for boot hart {}", hart));
    assert!(context_in_window(context, range.len()), "boot hart {}'s PLIC context lies past the PLIC", hart);
    Some(Plic { range, context })
}

#[cfg(test)]
mod tests {
    use std::vec::Vec;

    use super::*;

    /// A flattened device tree, written by hand (the bench does not depend on `dtc`): the
    /// header, an empty reservation map, the structure block and the strings.
    #[derive(Default)]
    struct Fdt {
        structure: Vec<u8>,
        strings: Vec<u8>,
    }

    impl Fdt {
        fn word(&mut self, w: u32) { self.structure.extend(w.to_be_bytes()); }

        fn pad(&mut self) { self.structure.resize(self.structure.len().next_multiple_of(4), 0); }

        fn begin(&mut self, name: &str) -> &mut Self {
            self.word(1);
            self.structure.extend(name.as_bytes());
            self.structure.push(0);
            self.pad();
            self
        }

        fn end(&mut self) -> &mut Self {
            self.word(2);
            self
        }

        fn prop(&mut self, name: &str, value: &[u8]) -> &mut Self {
            let offset = self.strings.len() as u32;
            self.strings.extend(name.as_bytes());
            self.strings.push(0);
            self.word(3);
            self.word(value.len() as u32);
            self.word(offset);
            self.structure.extend(value);
            self.pad();
            self
        }

        fn cells(&mut self, name: &str, cells: &[u32]) -> &mut Self {
            let value: Vec<u8> = cells.iter().flat_map(|c| c.to_be_bytes()).collect();
            self.prop(name, &value)
        }

        /// The blob, copied to a 4-byte boundary inside `storage` as the parser requires.
        fn finish<'a>(&mut self, storage: &'a mut Vec<u8>) -> &'a [u8] {
            self.word(9);
            const HEADER: usize = 40;
            const RESERVE_MAP: usize = 16;
            let strings_at = HEADER + RESERVE_MAP + self.structure.len();
            let total = strings_at + self.strings.len();
            let header = [
                0xd00d_feed,
                total as u32,
                (HEADER + RESERVE_MAP) as u32,
                strings_at as u32,
                HEADER as u32,
                17,
                16,
                0,
                self.strings.len() as u32,
                self.structure.len() as u32,
            ];
            let mut blob: Vec<u8> = header.iter().flat_map(|w: &u32| w.to_be_bytes()).collect();
            blob.resize(HEADER + RESERVE_MAP, 0);
            blob.extend(&self.structure);
            blob.extend(&self.strings);
            storage.resize(total + 3, 0);
            let at = storage.as_ptr().align_offset(4);
            storage[at..at + total].copy_from_slice(&blob);
            &storage[at..at + total]
        }
    }

    /// A QEMU `virt`-like machine with two harts, whose PLIC wires the contexts in `extended`
    /// ((hart intc phandle, hart interrupt) pairs; hart 0's phandle is 1, hart 1's is 2).
    fn machine(extended: &[u32], hart: usize) -> Platform { machine_with(extended, hart, 0, 0) }

    /// [`machine`], booted on hart 0 with every context wired, and `devices` virtio devices
    /// beside the PLIC, a page of MMIO each, every one raising interrupts `1..=irqs`.
    fn devices(devices: usize, irqs: u32) -> Platform {
        machine_with(&[1, M, 1, S, 2, M, 2, S], 0, devices, irqs)
    }

    fn machine_with(extended: &[u32], hart: usize, devices: usize, irqs: u32) -> Platform {
        tree(&[0, 1], extended, hart, 0x60_0000, devices, irqs)
    }

    /// A machine with a cpu for each `reg` in `cpus` (the n-th's interrupt controller has phandle
    /// n + 1), a PLIC `plic_size` bytes long wiring the contexts in `extended`, and `devices`.
    fn tree(
        cpus: &[u32],
        extended: &[u32],
        hart: usize,
        plic_size: u32,
        devices: usize,
        irqs: u32,
    ) -> Platform {
        let mut t = Fdt::default();
        t.begin("").cells("#address-cells", &[2]).cells("#size-cells", &[2]);
        t.begin("memory@80000000").prop("device_type", b"memory\0");
        t.cells("reg", &[0, 0x8000_0000, 0, 0x800_0000]).end();
        t.begin("chosen")
            .cells("linux,initrd-start", &[0x8800_0000])
            .cells("linux,initrd-end", &[0x8810_0000]);
        t.prop("rng-seed", &[7; 32]).end();
        t.begin("cpus").cells("#address-cells", &[1]).cells("#size-cells", &[0]);
        t.cells("timebase-frequency", &[10_000_000]);
        for (i, reg) in cpus.iter().enumerate() {
            t.begin(&std::format!("cpu@{:x}", reg)).cells("reg", &[*reg]);
            t.begin("interrupt-controller").cells("phandle", &[i as u32 + 1]).end();
            t.end();
        }
        t.end();
        t.begin("soc").begin("plic@c000000").prop("compatible", b"riscv,plic0\0");
        t.prop("interrupt-controller", &[]).cells("reg", &[0, 0xc00_0000, 0, plic_size]);
        t.cells("interrupts-extended", extended).end();
        for i in 0..devices {
            let base = 0x1000_1000 + i as u32 * 0x1000;
            t.begin(&std::format!("virtio_mmio@{:x}", base)).prop("compatible", b"virtio,mmio\0");
            t.cells("reg", &[0, base, 0, 0x1000]);
            t.cells("interrupts", &(1..=irqs).collect::<Vec<u32>>()).end();
        }
        t.end().end();
        let mut storage = Vec::new();
        let blob = t.finish(&mut storage);
        // SAFETY: `finish` put the blob on a 4-byte boundary, and the slice is exactly its
        // `totalsize` long, which is what `DevTree::new` asks of its caller.
        let dt = unsafe { DevTree::new(blob) }.expect("a well-formed fixture");
        let mut index = std::vec![0u8; 64 * 1024];
        Platform::parse(dt, 0, hart, &mut index)
    }

    const M: u32 = 11;
    const S: u32 = 9;

    /// (phandle, M), (phandle, S) for each of `n` cpus: contexts 2i and 2i + 1.
    fn wired(n: u32) -> Vec<u32> { (1..=n).flat_map(|p| [p, M, p, S]).collect() }

    #[test]
    fn harts_are_listed_by_boot_index_never_by_id() {
        // Sparse and wide ids, the boot hart in the middle: it is index 0, the rest in tree order.
        let platform = tree(&[0, 5, 1000], &wired(3), 5, 0x60_0000, 0, 0);
        assert_eq!(platform.harts(), &[5, 0, 1000]);
    }

    #[test]
    fn each_listed_hart_carries_its_s_mode_context_by_boot_index() {
        // Hart 5 boots: its context (3) first, then hart 0's (1) and hart 1000's (5).
        let platform = tree(&[0, 5, 1000], &wired(3), 5, 0x60_0000, 0, 0);
        assert_eq!(platform.contexts(), &[3, 1, 5]);
        // Only S-mode contexts, in any wiring order: hart 1's is 0 here, hart 0's is 2.
        let platform = machine(&[2, S, 1, M, 1, S], 0);
        assert_eq!(platform.contexts(), &[2, 0]);
    }

    #[test]
    fn harts_past_max_harts_stay_parked() {
        let cpus: Vec<u32> = (0..12).collect();
        let platform = tree(&cpus, &wired(12), 3, 0x60_0000, 0, 0);
        assert_eq!(platform.harts(), &[3, 0, 1, 2, 4, 5, 6, 7]);
        assert_eq!(platform.cpu_count, 12);
    }

    #[test]
    fn a_hart_whose_s_mode_context_is_past_the_plic_window_stays_parked() {
        // Contexts 1, 3, 5: a PLIC of 0x20_0000 + 4 contexts reaches the first two harts' only.
        let platform = tree(&[0, 1, 2], &wired(3), 0, 0x20_4000, 0, 0);
        assert_eq!(platform.harts(), &[0, 1]);
        // A hart with no S-mode context at all is parked too.
        let platform = tree(&[0, 1], &[1, M, 1, S, 2, M], 0, 0x60_0000, 0, 0);
        assert_eq!(platform.harts(), &[0]);
    }

    #[test]
    #[should_panic(expected = "PLIC context lies past the PLIC")]
    fn a_boot_hart_whose_context_is_past_the_plic_window_is_refused() {
        tree(&[0, 1], &wired(2), 1, 0x20_2000, 0, 0);
    }

    #[test]
    fn booting_on_hart_1_takes_hart_1s_s_mode_context() {
        let platform = machine(&[1, M, 1, S, 2, M, 2, S], 1);
        assert_eq!(platform.plic.expect("a PLIC").context, 3);
    }

    #[test]
    fn hart_0_without_an_s_mode_context_does_not_stop_a_boot_on_hart_1() {
        let platform = machine(&[1, M, 2, M, 2, S], 1);
        assert_eq!(platform.plic.expect("a PLIC").context, 2);
    }

    #[test]
    #[should_panic(expected = "no S-mode context for boot hart 1")]
    fn a_boot_hart_without_an_s_mode_context_is_refused() { machine(&[1, M, 1, S, 2, M], 1); }

    #[test]
    fn thirty_two_devices_are_kept() {
        // 31 devices and the PLIC: 32 regions.
        let platform = devices(31, 32);
        assert_eq!(platform.mmio().len(), MAX_MMIO);
        assert!(platform.mmio().iter().any(|region| region.kernel_only));
        assert_eq!(platform.irq[..platform.irq_len], *(1..=32).collect::<Vec<u32>>());
    }

    #[test]
    #[should_panic(expected = "MMIO region virtio_mmio@10020000 is past the loader's 32")]
    fn a_33rd_mmio_region_is_refused() { devices(32, 1); }

    #[test]
    #[should_panic(expected = "interrupt 33 is past the loader's 32")]
    fn a_33rd_interrupt_is_refused() { devices(1, 33); }

    #[test]
    fn an_interrupt_two_devices_raise_takes_one_slot() {
        // 64 entries, 32 distinct interrupts: the table counts interrupts, not entries.
        let platform = devices(2, 32);
        assert_eq!(platform.irq[..platform.irq_len], *(1..=32).collect::<Vec<u32>>());
    }
}
