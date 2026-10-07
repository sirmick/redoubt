//! Memory accounting: how much a process (or an ETS object) holds, in BEAM's units (64-bit
//! words).
//!
//! Terms live on per-process heaps, so memory is read off them, not measured: a heap cell is
//! two words, and the bytes a heap holds off-heap (binaries, bignums) are counted separately,
//! as BEAM counts its reference-counted binaries.
//!
//! [`footprint`] is the other measure: everything the VM holds, by kind, in the bytes its
//! allocations hold.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::atom::Atom;
use crate::module::{Arg, Instr, Module};
use crate::process::Process;
use crate::term::Term;
use crate::vm::System;

/// What a process holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Heap words (including garbage not yet collected), stack and registers.
    pub words: u64,
    /// Bytes held off the heap.
    pub binary_bytes: u64,
}

impl Usage {
    /// Everything, in words: what resource limits compare against.
    pub fn total_words(&self) -> u64 { self.words.saturating_add(self.binary_bytes.div_ceil(8)) }
}

/// Everything process `p` holds.
pub fn process(p: &Process) -> Usage {
    let fixed = PROCESS_WORDS + p.x.len() as u64 + 2 * p.stack.len() as u64 + p.frames.len() as u64;
    Usage { words: fixed + 2 * p.heap.len() as u64, binary_bytes: p.heap.offheap_bytes() as u64 }
}

/// A process's own structures (BEAM's process struct and minimum heap), in words.
pub const PROCESS_WORDS: u64 = 338;

/// The embedder's heap, in pages: held now (at the report) and the most held at once (its peak so
/// far), if it knows them.
pub type HeapPages = fn() -> Option<(u64, u64)>;

/// The page of Redoubt's runtime heap.
const PAGE: usize = 4096;

/// The bytes Redoubt's runtime heap holds for an allocation of `n` bytes (libs/rt/src/heap.rs):
/// a power-of-two class from 16 to 2,048 bytes, or whole pages. On a host, a model of the machine.
pub fn held(n: usize) -> usize {
    match n {
        0 => 0,
        1..=2048 => n.max(16).next_power_of_two(),
        _ => n.next_multiple_of(PAGE),
    }
}

/// One row of the breakdown: a count, the bytes allocations asked for, and the bytes the
/// runtime heap holds for them.
#[derive(Clone, Copy, Default)]
struct Row {
    count: usize,
    asked: usize,
    held: usize,
}

impl Row {
    /// One allocation of `bytes`, counting `count`.
    fn add(&mut self, count: usize, bytes: usize) {
        self.count += count;
        self.asked += bytes;
        self.held += held(bytes);
    }
}

/// What the VM holds, by kind, as console lines (docs/userland/beamlet.md, "What the VM holds
/// at its prompt"): what each allocation asked for and what Redoubt's heap holds for it. Every
/// process not running is collected first, so its heap's row is before and after. B-tree nodes
/// and the off-heap tables' indexes are not counted.
pub fn footprint(sys: &mut System, heap_pages: HeapPages) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!(
        "footprint sizes Instr={} Arg={} Term={} Module={} Process={}\n",
        size_of::<Instr>(),
        size_of::<Arg>(),
        size_of::<Term>(),
        size_of::<Module>(),
        size_of::<Process>()
    ));

    let [mut instrs, mut args, mut lits, mut strings, mut attrs, mut cinf, mut tables] = [Row::default(); 7];
    let mut largest: Vec<(usize, &str)> = Vec::new();
    for m in sys.modules.values() {
        let before = [instrs, args, lits, strings, attrs, cinf, tables].iter().map(|r| r.held).sum::<usize>();
        instrs.add(m.code.len(), size_of_val(&*m.code));
        // Lists' items included: they are in the same array.
        args.add(m.operands.len(), size_of_val(&*m.operands));
        lits.add(m.literals.len(), m.literals.capacity() * size_of::<Term>());
        strings.add(1, m.strings.capacity());
        attrs.add(1, m.attributes.capacity());
        cinf.add(1, m.compile_info.capacity());
        tables.add(1, size_of::<Module>());
        tables.add(0, m.imports.capacity() * size_of::<crate::module::Import>());
        tables.add(0, m.exports.capacity() * size_of::<crate::module::Export>());
        tables.add(0, m.funs.capacity() * size_of::<crate::module::FunEntry>());
        tables.add(0, m.functions.capacity() * size_of::<crate::module::FunctionInfo>());
        tables.add(0, m.lines.files.capacity() * size_of::<Term>());
        tables.add(0, m.lines.items.capacity() * size_of::<(u32, u32)>());
        tables.add(0, m.lines.marks.capacity() * size_of::<(u32, u32)>());
        tables.add(0, m.body_natives.capacity() * size_of::<(crate::bif::Native, Atom, u32)>());
        let after = [instrs, args, lits, strings, attrs, cinf, tables].iter().map(|r| r.held).sum::<usize>();
        largest.push((after - before, m.name.as_str()));
    }
    largest.sort_unstable_by(|a, b| b.cmp(a));
    let largest: Vec<String> = largest.iter().take(5).map(|(n, name)| format!("{name}={n}")).collect();

    let mut atoms = Row::default();
    for name in sys.atom_table.names() {
        atoms.add(1, size_of::<String>());
        atoms.add(0, name.len());
    }

    let mut chunks = Row { count: sys.literals.cells(), ..Row::default() };
    for (cells, entries, bytes) in sys.literals.each_chunk() {
        // The `Arc`'s two counts and the chunk's two `Vec`s.
        chunks.add(0, size_of::<[usize; 2]>() + 2 * size_of::<Vec<u8>>());
        chunks.add(0, cells * size_of::<Term>());
        chunks.add(0, entries * size_of::<crate::term::OffHeap>());
        chunks.add(0, bytes);
    }

    let [mut heaps, mut live, mut rest, mut binaries] = [Row::default(); 4];
    for p in sys.procs.present_mut() {
        heaps.add(p.heap.len(), p.heap.capacity() * size_of::<Term>());
        p.collect();
        live.add(p.heap.len(), p.heap.capacity() * size_of::<Term>());
        binaries.add(0, p.heap.offheap_bytes());
        rest.add(1, size_of::<Process>());
        rest.add(0, p.x.capacity() * size_of::<Term>());
        rest.add(0, p.f.capacity() * size_of::<f64>());
        rest.add(0, p.stack.capacity() * size_of::<Term>());
        rest.add(0, p.frames.capacity() * size_of::<crate::process::Frame>());
        rest.add(0, p.mailbox.capacity() * size_of::<Term>());
    }
    let mut ets = Row::default();
    ets.add(0, sys.ets.words() as usize * 8);

    let rows = [
        ("code.instrs", instrs),
        ("code.operands", args),
        ("module.literals", lits),
        ("module.strings", strings),
        ("module.attributes", attrs),
        ("module.compile_info", cinf),
        ("module.tables", tables),
        ("atoms", atoms),
        ("literal_table", chunks),
        ("process.heaps", heaps),
        ("process.heaps_collected", live),
        ("process.rest", rest),
        ("binaries", binaries),
        ("ets", ets),
    ];
    for (name, r) in rows {
        lines.push(format!("footprint {name} count={} asked={} held={}\n", r.count, r.asked, r.held));
    }
    // The heaps as collected: what they hold when the runtime's pages are read below.
    let total: usize = rows.iter().filter(|(name, _)| *name != "process.heaps").map(|(_, r)| r.held).sum();
    lines.push(format!(
        "footprint modules count={} chunks={} replaced={} largest {}\n",
        sys.modules.len(),
        sys.literals.chunks(),
        sys.replaced,
        largest.join(" ")
    ));
    let pages = total.div_ceil(PAGE) as u64;
    lines.push(format!("footprint total held={total} pages={pages}\n"));
    // What the runtime holds beyond this count (free small-class pages, B-tree nodes, the
    // embedder's buffers), and what its peak held beyond now (the transient of a load).
    if let Some((now, peak)) = heap_pages() {
        lines.push(format!(
            "footprint runtime pages held_now={now} peak={peak} unaccounted={} transient={}\n",
            now.saturating_sub(pages),
            peak.saturating_sub(now)
        ));
    }
    lines
}
