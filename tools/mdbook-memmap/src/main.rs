//! An mdBook preprocessor: a fenced ```` ```memmap ```` block is an address map written as text,
//! and it is rendered into an inline SVG drawn to scale on a log axis, so a 4 KiB page and a
//! 256 GiB region both have room for their labels while their sizes still read.
//!
//! The block, top of the address space first, one region per line:
//!
//! ```text
//! top 0xffff_ffff_ffff_ffff
//! columns root entries | sharing
//! 0xffff_ffff_c000_0000 | kernel area: image, stacks, PLIC and DMA windows | 511 | shared
//! 0x0000_0040_0000_0000 hole | not canonical: every access faults
//! 0x0 | user space, 256 GiB | 0 - 255 | one per address space
//! ```
//!
//! `top` is the last address of the space. Each region line is its first address, `hole` if it
//! is drawn hatched (unmapped, not canonical), then `|`-separated cells: the region's name, then
//! one cell per note column. A region reaches up to the line above it. Addresses are hex with
//! optional `_` grouping and must descend. A malformed block fails the build, naming the chapter
//! and the line, since a map that silently drops a region would mislead.
//!
//! Only the page source is text; the SVG exists in the rendered book alone (the docs checker
//! allows no image files). The drawing uses `currentColor`, so it follows the theme.
#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::io::{self, Read, Write};
use std::process::ExitCode;

use serde_json::Value;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("supports") {
        return ExitCode::SUCCESS;
    }
    let mut input = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut input) {
        eprintln!("mdbook-memmap: reading stdin: {e}");
        return ExitCode::FAILURE;
    }
    let mut parsed: Value = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("mdbook-memmap: not mdBook's [context, book] JSON: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(book) = parsed.as_array_mut().and_then(|a| a.get_mut(1)) else {
        eprintln!("mdbook-memmap: expected [context, book]");
        return ExitCode::FAILURE;
    };
    let mut errors = Vec::new();
    // mdBook 0.5 calls the list `items`; 0.4 called it `sections`.
    for key in ["items", "sections"] {
        if let Some(items) = book.get_mut(key).and_then(Value::as_array_mut) {
            for item in items {
                render_item(item, &mut errors);
            }
        }
    }
    if !errors.is_empty() {
        for e in &errors {
            eprintln!("mdbook-memmap: {e}");
        }
        return ExitCode::FAILURE;
    }
    match serde_json::to_string(book).map(|s| io::stdout().write_all(s.as_bytes())) {
        Ok(Ok(())) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

fn render_item(item: &mut Value, errors: &mut Vec<String>) {
    let Some(chapter) = item.get_mut("Chapter") else { return };
    let name = chapter.get("name").and_then(Value::as_str).unwrap_or("?").to_string();
    if let Some(content) = chapter.get("content").and_then(Value::as_str) {
        match render_markdown(content) {
            Ok(out) => chapter["content"] = Value::String(out),
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }
    if let Some(subs) = chapter.get_mut("sub_items").and_then(Value::as_array_mut) {
        for sub in subs {
            render_item(sub, errors);
        }
    }
}

/// `markdown` with each ```` ```memmap ```` block replaced by its SVG. Other fences are left
/// alone, and a `memmap` fence inside another fence is text, as Markdown has it.
pub fn render_markdown(markdown: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut lines = markdown.lines().enumerate().peekable();
    let mut n = 0;
    while let Some((i, line)) = lines.next() {
        let fence = line.trim_start();
        if fence.starts_with("```memmap") {
            let mut block = Vec::new();
            let mut closed = false;
            for (_, l) in lines.by_ref() {
                if l.trim_start().starts_with("```") {
                    closed = true;
                    break;
                }
                block.push(l);
            }
            if !closed {
                return Err(format!("line {}: memmap block is not closed", i + 1));
            }
            let map = parse(&block).map_err(|e| format!("line {}: {e}", i + 1))?;
            n += 1;
            out.push('\n');
            out.push_str(&svg(&map, n));
            out.push('\n');
        } else if fence.starts_with("```") || fence.starts_with("~~~") {
            let marker = if fence.starts_with("```") { "```" } else { "~~~" };
            out.push_str(line);
            out.push('\n');
            for (_, l) in lines.by_ref() {
                out.push_str(l);
                out.push('\n');
                if l.trim_start().starts_with(marker) {
                    break;
                }
            }
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Region {
    pub start: u128,
    /// One past the last address.
    pub end: u128,
    pub hole: bool,
    pub name: String,
    pub notes: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Map {
    pub columns: Vec<String>,
    /// Top of the space first.
    pub regions: Vec<Region>,
}

fn parse_address(s: &str) -> Result<u128, String> {
    let digits = s.strip_prefix("0x").ok_or_else(|| format!("`{s}` is not a hex address"))?;
    let digits: String = digits.chars().filter(|&c| c != '_').collect();
    if digits.is_empty() || digits.len() > 32 {
        return Err(format!("`{s}` is not a hex address"));
    }
    u128::from_str_radix(&digits, 16).map_err(|_| format!("`{s}` is not a hex address"))
}

pub fn parse(lines: &[&str]) -> Result<Map, String> {
    let mut top = None;
    let mut columns = Vec::new();
    let mut regions: Vec<Region> = Vec::new();
    for raw in lines {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(t) = line.strip_prefix("top ") {
            if top.is_some() || !regions.is_empty() {
                return Err("`top` must come first, once".into());
            }
            top = Some(parse_address(t.trim())?);
            continue;
        }
        if let Some(c) = line.strip_prefix("columns ") {
            columns = c.split('|').map(|s| s.trim().to_string()).collect();
            continue;
        }
        let Some(top) = top else { return Err("the first line must be `top <address>`".into()) };
        let mut cells = line.split('|').map(str::trim);
        let head = cells.next().unwrap_or("");
        let (addr, hole) = match head.split_once(char::is_whitespace) {
            Some((a, flag)) if flag.trim() == "hole" => (a, true),
            Some((_, flag)) => return Err(format!("`{}` is not a flag; only `hole` is", flag.trim())),
            None => (head, false),
        };
        let start = parse_address(addr)?;
        let end = match regions.last() {
            Some(prev) => prev.start,
            None => top.checked_add(1).ok_or("the top address overflows")?,
        };
        if start >= end {
            return Err(format!("`{addr}` does not descend below the region above it"));
        }
        let name = cells.next().ok_or_else(|| format!("`{addr}` has no name cell"))?.to_string();
        if name.is_empty() {
            return Err(format!("`{addr}` has an empty name"));
        }
        let notes: Vec<String> = cells.map(str::to_string).collect();
        if notes.len() > columns.len() {
            return Err(format!(
                "`{addr}` has {} notes but there are {} columns",
                notes.len(),
                columns.len()
            ));
        }
        regions.push(Region { start, end, hole, name, notes });
    }
    if regions.is_empty() {
        return Err("no regions".into());
    }
    Ok(Map { columns, regions })
}

// ---- drawing ----

const FONT: f64 = 13.0;
const LINE: f64 = 17.0;
const CHAR: f64 = 7.4; // the width of one character at FONT, for wrapping and columns
const ADDR_W: f64 = 180.0; // the address column, right-aligned on the box's left edge
const BOX_W: f64 = 290.0;
const NOTE_W: f64 = 190.0; // a note column at most, before its text wraps
const GAP: f64 = 14.0;
const PAD: f64 = 8.0;
const PX_PER_BIT: f64 = 2.4; // height per doubling of a region's size
const TOP_PAD: f64 = 24.0; // room for the column headers and the top address

fn log2(size: u128) -> f64 { (128 - size.leading_zeros()) as f64 - 1.0 + ((size as f64).log2().fract()) }

fn wrap(text: &str, width_px: f64) -> Vec<String> {
    let max = ((width_px - 2.0 * PAD) / CHAR).floor().max(8.0) as usize;
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        let cur = lines.last_mut().unwrap();
        if !cur.is_empty() && cur.len() + 1 + word.len() > max {
            lines.push(word.to_string());
        } else {
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(word);
        }
    }
    lines
}

fn esc(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;") }

fn hex(a: u128) -> String {
    let digits = format!("{a:x}");
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 4 == 0 {
            grouped.push('_');
        }
        grouped.push(c);
    }
    format!("0x{grouped}")
}

/// The region heights: to scale on a log axis, never below what the text needs.
fn heights(map: &Map) -> Vec<f64> {
    map.regions
        .iter()
        .map(|r| {
            let text = wrap(&r.name, BOX_W)
                .len()
                .max(r.notes.iter().map(|n| wrap(n, NOTE_W).len()).max().unwrap_or(1));
            let need = text as f64 * LINE + PAD;
            (PX_PER_BIT * log2(r.end - r.start)).max(need)
        })
        .collect()
}

pub fn svg(map: &Map, n: usize) -> String {
    let hs = heights(map);
    let col_w: Vec<f64> = (0..map.columns.len())
        .map(|c| {
            let longest = map
                .regions
                .iter()
                .filter_map(|r| r.notes.get(c))
                .chain(std::iter::once(&map.columns[c]))
                .map(|s| s.len())
                .max()
                .unwrap_or(4);
            (longest as f64 * CHAR + PAD).min(NOTE_W + PAD).max(40.0)
        })
        .collect();
    let width = ADDR_W + BOX_W + col_w.iter().map(|w| w + GAP).sum::<f64>() + PAD;
    let height = TOP_PAD + hs.iter().sum::<f64>() + LINE;
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg class="memmap" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:.0} {height:.0}" width="{width:.0}" height="{height:.0}" font-size="{FONT}" fill="currentColor" role="img" aria-label="address map">"#
    );
    let _ = write!(
        s,
        r#"<defs><pattern id="memmap-hatch-{n}" width="8" height="8" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><line x1="0" y1="0" x2="0" y2="8" stroke="currentColor" stroke-opacity="0.35" stroke-width="1"/></pattern></defs>"#
    );
    // Column headers.
    let mut x = ADDR_W + BOX_W + GAP;
    for (c, name) in map.columns.iter().enumerate() {
        let _ = write!(
            s,
            r#"<text x="{x:.1}" y="{:.1}" font-style="italic" fill-opacity="0.7">{}</text>"#,
            FONT + 2.0,
            esc(name)
        );
        x += col_w[c] + GAP;
    }
    let mut y = TOP_PAD;
    for (i, r) in map.regions.iter().enumerate() {
        let h = hs[i];
        // The box, hatched for a hole.
        let fill = if r.hole { format!("url(#memmap-hatch-{n})") } else { "none".into() };
        let _ = write!(
            s,
            r#"<rect x="{ADDR_W}" y="{y:.1}" width="{BOX_W}" height="{h:.1}" fill="{fill}" stroke="currentColor" stroke-width="1"/>"#
        );
        // The upper boundary's address, at the box's top edge; the last region also gets its start.
        let upper = if i == 0 { r.end - 1 } else { r.end };
        let _ = write!(
            s,
            r#"<text x="{:.1}" y="{:.1}" text-anchor="end" font-family="monospace">{}</text>"#,
            ADDR_W - PAD,
            y + 4.0,
            hex(upper)
        );
        // The name, vertically centred.
        let lines = wrap(&r.name, BOX_W);
        let start_y = y + h / 2.0 - (lines.len() as f64 - 1.0) * LINE / 2.0 + FONT / 3.0;
        for (k, l) in lines.iter().enumerate() {
            let _ = write!(
                s,
                r#"<text x="{:.1}" y="{:.1}">{}</text>"#,
                ADDR_W + PAD,
                start_y + k as f64 * LINE,
                esc(l)
            );
        }
        // The notes, one column each.
        let mut x = ADDR_W + BOX_W + GAP;
        for (c, note) in r.notes.iter().enumerate() {
            let lines = wrap(note, col_w[c] + 2.0 * PAD);
            let start_y = y + h / 2.0 - (lines.len() as f64 - 1.0) * LINE / 2.0 + FONT / 3.0;
            for (k, l) in lines.iter().enumerate() {
                let _ =
                    write!(s, r#"<text x="{x:.1}" y="{:.1}">{}</text>"#, start_y + k as f64 * LINE, esc(l));
            }
            x += col_w[c] + GAP;
        }
        y += h;
    }
    let last = map.regions.last().unwrap();
    let _ = write!(
        s,
        r#"<text x="{:.1}" y="{:.1}" text-anchor="end" font-family="monospace">{}</text>"#,
        ADDR_W - PAD,
        y + 4.0,
        hex(last.start)
    );
    s.push_str("</svg>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const SV39: &str = "top 0xffff_ffff_ffff_ffff
columns root entries | sharing
0xffff_ffff_c000_0000 | kernel area: image, kernel and trap stacks, PLIC and DMA register windows | 511 | shared
0xffff_ffff_8000_0000 | per-process kernel data | 510 | one per address space
0xffff_ffe0_0000_0000 | empty | 384 - 509
0xffff_ffc0_0000_0000 | physmap: physical 0 to the end of RAM, 1 GiB leaves | 256 - 383 | shared
0x0000_0040_0000_0000 hole | not canonical: every access faults
0x0 | user space, 256 GiB | 0 - 255 | one per address space";

    fn lines(s: &str) -> Vec<&str> { s.lines().collect() }

    #[test]
    fn a_map_parses_top_down() {
        let m = parse(&lines(SV39)).unwrap();
        assert_eq!(m.columns, ["root entries", "sharing"]);
        assert_eq!(m.regions.len(), 6);
        assert_eq!(m.regions[0].end, 1u128 << 64);
        assert_eq!(m.regions[0].start, 0xffff_ffff_c000_0000);
        assert_eq!(m.regions[1].end, 0xffff_ffff_c000_0000);
        assert!(m.regions[4].hole);
        assert_eq!(m.regions[5].notes, ["0 - 255", "one per address space"]);
        assert_eq!(m.regions[2].notes, ["384 - 509"]);
    }

    #[test]
    fn heights_follow_the_log_of_the_size_and_never_crush_the_text() {
        let m = parse(&lines(SV39)).unwrap();
        let hs = heights(&m);
        // user space (256 GiB) is taller than per-process kernel data (1 GiB) ...
        assert!(hs[5] > hs[1], "{hs:?}");
        // ... but not 256 times taller: the axis is log, not linear.
        assert!(hs[5] < hs[1] * 3.0, "{hs:?}");
        // A tiny region still fits one line of text.
        let tiny = parse(&lines("top 0x1fff\n0x1000 | one page\n0x0 | the rest")).unwrap();
        assert!(heights(&tiny)[0] >= LINE + PAD);
    }

    #[test]
    fn the_drawing_has_every_boundary_and_hatches_the_hole() {
        let m = parse(&lines(SV39)).unwrap();
        let out = svg(&m, 1);
        for a in ["0xffff_ffff_ffff_ffff", "0xffff_ffff_c000_0000", "0x40_0000_0000", "0x0"] {
            assert!(out.contains(&format!(">{a}<")), "{a} missing");
        }
        assert_eq!(out.matches("<rect").count(), 6);
        assert_eq!(out.matches(r##"fill="url(#memmap-hatch-1)""##).count(), 1);
        assert!(out.contains("root entries") && out.contains("one per address space"));
        assert!(out.starts_with("<svg") && out.ends_with("</svg>"));
    }

    #[test]
    fn malformed_maps_are_refused_with_a_reason() {
        let bad = [
            ("0x0 | no top first", "first line must be `top"),
            ("top 0xff\n0x100 | above the top", "does not descend"),
            ("top 0xff\n0x10 | a\n0x20 | b", "does not descend"),
            ("top 0xff\n0x10 x | a", "not a flag"),
            ("top 0xff\n0x10 | a | too | many", "0 columns"),
            ("top 0xff\n0x10", "no name cell"),
            ("top 0xff\n0xzz | a", "not a hex address"),
            ("top 0xff", "no regions"),
        ];
        for (src, why) in bad {
            let err = parse(&lines(src)).unwrap_err();
            assert!(err.contains(why), "{src:?}: {err}");
        }
    }

    #[test]
    fn only_memmap_fences_are_replaced() {
        let md = "before\n\n```memmap\ntop 0xff\n0x0 | all\n```\n\n```rust\nlet x = \"```memmap\";\n```\n\n~~~\n```memmap\n~~~\nafter\n";
        let out = render_markdown(md).unwrap();
        assert!(out.contains("<svg"));
        assert_eq!(out.matches("<svg").count(), 1);
        assert!(out.contains("let x = \"```memmap\";"));
        assert!(out.contains("after"));
        assert!(render_markdown("```memmap\ntop 0xff\n").unwrap_err().contains("not closed"));
        assert!(render_markdown("x\n```memmap\nnope\n```\n").unwrap_err().starts_with("line 2:"));
    }

    #[test]
    fn addresses_are_grouped_by_four() {
        assert_eq!(hex(0), "0x0");
        assert_eq!(hex(0x8000_0000), "0x8000_0000");
        assert_eq!(hex(0xffff_ffff_ffff_ffff), "0xffff_ffff_ffff_ffff");
        assert_eq!(hex(0x40_0000_0000), "0x40_0000_0000");
    }
}
