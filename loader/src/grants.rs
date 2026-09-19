//! Parses the boot bundle's `grants` manifest (see `planning/xous64/DEVICE-GRANTS.md`)
//! and emits a `Grnt` argument tag per granted process.
//!
//! The manifest is plain text, one rule per line:
//!   `<process-name> mmio <hex-base> <hex-len>`  or  `<process-name> irq <decimal>`
//! Lines are matched by process name, so no allocation or name table is needed.

use crate::args::ArgsBuilder;

const MAX_PER_PROCESS: usize = 16;

struct Grants {
    mmio: [(u64, u64); MAX_PER_PROCESS],
    n_mmio: usize,
    irq: [u32; MAX_PER_PROCESS],
    n_irq: usize,
}

fn parse_int(token: &str) -> Option<u64> {
    match token.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => token.parse().ok(),
    }
}

/// Collect the grants for `name` from the manifest text.
fn parse_for(manifest: &str, name: &str) -> Grants {
    let mut g = Grants { mmio: [(0, 0); MAX_PER_PROCESS], n_mmio: 0, irq: [0; MAX_PER_PROCESS], n_irq: 0 };
    for line in manifest.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut fields = line.split_whitespace();
        if fields.next() != Some(name) {
            continue;
        }
        match fields.next() {
            Some("mmio") => {
                if let (Some(base), Some(len)) =
                    (fields.next().and_then(parse_int), fields.next().and_then(parse_int))
                {
                    if g.n_mmio < MAX_PER_PROCESS {
                        g.mmio[g.n_mmio] = (base, len);
                        g.n_mmio += 1;
                    }
                }
            }
            Some("irq") => {
                if let Some(irq) = fields.next().and_then(parse_int) {
                    if g.n_irq < MAX_PER_PROCESS {
                        g.irq[g.n_irq] = irq as u32;
                        g.n_irq += 1;
                    }
                }
            }
            _ => {}
        }
    }
    g
}

/// If `name` has any grants in `manifest`, emit a `Grnt` tag for `pid`.
pub fn emit(args: &mut ArgsBuilder, manifest: &str, name: &str, pid: u8) {
    let g = parse_for(manifest, name);
    if g.n_mmio == 0 && g.n_irq == 0 {
        return;
    }
    args.begin(b"Grnt");
    args.word(pid as u32);
    args.word(g.n_mmio as u32);
    args.word(g.n_irq as u32);
    for &(base, len) in &g.mmio[..g.n_mmio] {
        args.word64(base);
        args.word64(len);
    }
    for &irq in &g.irq[..g.n_irq] {
        args.word(irq);
    }
    args.end();
}
