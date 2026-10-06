//! A stopped guest's RAM is the witness for first-thread stack use and heap peaks. The paint and
//! the heap record are public, so any guest, including another server, can forge them; the result
//! measures the paths these cases drove, not an adversarial proof.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use redoubt_client::launch::STACK_PAINT;
use redoubt_rt::heap::{RECORD_MAGIC, Record};
use redoubt_sys::PAGE_SIZE;
use serde_json::Value;
use stub::MAX_STACK_PAGES;

use crate::case::{Boot, Program};

const UNIT: usize = 8;
const CHUNK: usize = 64 * 1024;

/// What a server declares: its first-thread stack, and its heap cap, 0 for none.
#[derive(Clone, Debug)]
pub struct Server {
    pub name: String,
    pub stack_pages: usize,
    pub heap_pages: u64,
}

#[derive(Debug)]
pub struct Measurement {
    pub lines: Vec<String>,
    pub failures: Vec<String>,
}

/// Read exactly the manifest a memory case puts in the bundle, including the case's server
/// overrides. The manifest order is the tag order used by `init`.
pub fn servers(boot: &Boot, workspace: &Path) -> Result<Vec<Server>> {
    let file = boot.file.iter().find(|file| file.name == "manifest").context("memory needs a manifest")?;
    let Program::Path { path } = &file.from else { bail!("memory needs a manifest read from a path") };
    let bytes = std::fs::read(workspace.join(path)).with_context(|| format!("reading {}", path.display()))?;
    let manifest: Value = serde_json::from_slice(&file.merged(&bytes)?)?;
    let servers = manifest["servers"].as_array().context("manifest needs servers")?;
    ensure!(servers.len() <= u16::MAX as usize, "too many servers for stack tags");
    servers
        .iter()
        .enumerate()
        .map(|(i, server)| {
            let name = server["name"].as_str().with_context(|| format!("servers[{i}].name"))?;
            let pages = |key: &str| -> Result<Option<u64>> {
                let Some(value) = server.get(key) else { return Ok(None) };
                let text =
                    value.as_str().with_context(|| format!("servers[{i}].{key} is not a decimal string"))?;
                Ok(Some(text.parse()?))
            };
            let stack_pages =
                pages("stack_pages")?.map_or(redoubt_client::launch::STACK_PAGES, |n| n as usize);
            ensure!(
                (1..=MAX_STACK_PAGES).contains(&stack_pages),
                "{name}: invalid stack_pages {stack_pages}"
            );
            let heap_pages = pages("heap_pages")?;
            ensure!(heap_pages != Some(0), "{name}: invalid heap_pages 0");
            Ok(Server { name: name.to_string(), stack_pages, heap_pages: heap_pages.unwrap_or(0) })
        })
        .collect()
}

/// Scan every aligned RAM unit, recording where each tagged stack unit survived and each tagged
/// heap record. A duplicate unit or record is ambiguous even if the copy came from the guest's
/// ordinary data.
pub fn scan(mut ram: impl Read, bytes: u64, servers: &[Server]) -> Result<Measurement> {
    let mut seen: Vec<Vec<bool>> =
        servers.iter().map(|s| vec![false; s.stack_pages * PAGE_SIZE / UNIT]).collect();
    // Each server's heap record, (cap, peak); and the last four units, a record's length.
    let mut heaps: Vec<Option<(u64, u64)>> = vec![None; servers.len()];
    let mut window = [0u64; 4];
    let mut chunk = [0u8; CHUNK];
    let mut left = bytes;
    while left > 0 {
        let n = left.min(CHUNK as u64) as usize;
        ram.read_exact(&mut chunk[..n]).context("reading the QMP RAM dump")?;
        let first_unit = (bytes - left) / UNIT as u64;
        for (at, unit) in chunk[..n].chunks_exact(UNIT).enumerate() {
            let word = u64::from_le_bytes(unit.try_into().unwrap());
            window.rotate_left(1);
            window[3] = word;
            let tag = window[Record::TAG];
            if window[Record::MAGIC] == RECORD_MAGIC && (1..=servers.len() as u64).contains(&tag) {
                let i = tag as usize - 1;
                ensure!(heaps[i].is_none(), "{}: heap record found twice", servers[i].name);
                heaps[i] = Some((window[Record::CAP], window[Record::PEAK]));
            }
            if (word >> 32) as u32 != STACK_PAINT {
                continue;
            }
            let tag = ((word >> 16) & 0xffff) as usize;
            if tag == 0 || tag > servers.len() {
                continue;
            }
            let i = tag - 1;
            let index = (word & 0xffff) as usize;
            // A live stack can copy a painted word into another stack slot. Only its original
            // position within a physical page is evidence of an untouched unit. Frame order
            // need not match virtual stack order, but the offset inside each frame does.
            if (first_unit + at as u64) % (PAGE_SIZE / UNIT) as u64 != (index % (PAGE_SIZE / UNIT)) as u64 {
                continue;
            }
            ensure!(
                index < seen[i].len(),
                "{}: stack paint index {index} is outside its stack",
                servers[i].name
            );
            ensure!(!seen[i][index], "{}: stack paint unit {index} found twice", servers[i].name);
            seen[i][index] = true;
        }
        left -= n as u64;
    }
    let mut lines = Vec::new();
    let mut failures = Vec::new();
    for (i, server) in servers.iter().enumerate() {
        if !seen[i].contains(&true) {
            failures.push(format!("{}: no stack paint found", server.name));
            continue;
        }
        let untouched = seen[i].iter().position(|found| !found).unwrap_or(seen[i].len());
        let peak = (seen[i].len() - untouched) * UNIT;
        let required = (2 * peak).div_ceil(PAGE_SIZE);
        if server.stack_pages < required {
            failures.push(format!(
                "{}: stack needs {required} pages for twice its {peak}-byte peak, declared {}",
                server.name, server.stack_pages
            ));
        }
        lines.push(format!("stack {} {peak} of {} pages", server.name, server.stack_pages));
    }
    for (server, heap) in servers.iter().zip(&heaps) {
        let name = &server.name;
        let Some((cap, peak)) = *heap else {
            failures.push(format!("{name}: no heap record found"));
            continue;
        };
        if cap != server.heap_pages {
            failures
                .push(format!("{name}: heap record capped at {cap} pages, declared {}", server.heap_pages));
        } else if cap != 0 && cap < peak.saturating_mul(2) {
            failures.push(format!(
                "{name}: heap needs {} pages for twice its {peak}-page peak, capped at {cap}",
                peak.saturating_mul(2)
            ));
        }
        lines.push(match cap {
            0 => format!("heap {name} {peak} pages uncapped"),
            cap => format!("heap {name} {peak} of {cap} pages"),
        });
    }
    Ok(Measurement { lines, failures })
}

#[cfg(test)]
mod tests {
    use redoubt_client::launch::stack_paint;

    use super::*;

    fn painted(pages: usize, tag: u16) -> Vec<u8> {
        (0..pages * PAGE_SIZE / UNIT).flat_map(|i| stack_paint(tag, i as u16).to_le_bytes()).collect()
    }

    /// A heap record as the runtime leaves it: magic, tag, cap and peak.
    fn record(tag: u16, cap: u64, peak: u64) -> Vec<u8> {
        [RECORD_MAGIC, u64::from(tag), cap, peak].iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    /// An uncapped server of `pages` stack pages.
    fn server(name: &str, pages: usize) -> Server {
        Server { name: name.into(), stack_pages: pages, heap_pages: 0 }
    }

    /// RAM holding `servers`' records, uncapped, each with a peak of 3 pages.
    fn records(ram: &mut Vec<u8>, servers: usize) {
        for tag in 1..=servers as u16 {
            ram.extend(record(tag, 0, 3));
        }
    }

    #[test]
    fn scanner_measures_the_lowest_missing_unit() {
        let mut ram = painted(2, 1);
        let used = 200 * UNIT;
        let len = ram.len();
        ram[len - used..].fill(0);
        records(&mut ram, 1);
        let servers = [server("a", 2)];
        let measured = scan(&ram[..], ram.len() as u64, &servers).unwrap();
        assert_eq!(measured.lines, ["stack a 1600 of 2 pages", "heap a 3 pages uncapped"]);
        assert!(measured.failures.is_empty());
    }

    #[test]
    fn scanner_refuses_missing_duplicate_and_too_little_margin() {
        let server = [server("a", 1)];
        let empty = vec![0; PAGE_SIZE];
        assert!(
            scan(&empty[..], empty.len() as u64, &server).unwrap().failures[0].contains("no stack paint")
        );
        let mut duplicate = painted(1, 1);
        duplicate.extend_from_slice(&stack_paint(1, 0).to_le_bytes());
        assert!(
            scan(&duplicate[..], duplicate.len() as u64, &server)
                .unwrap_err()
                .to_string()
                .contains("found twice")
        );
        let mut used = painted(1, 1);
        used[PAGE_SIZE - 3000..].fill(0);
        records(&mut used, 1);
        let measured = scan(&used[..], used.len() as u64, &server).unwrap();
        assert_eq!(measured.lines[0], "stack a 3000 of 1 pages");
        assert!(measured.failures[0].contains("stack needs"));
    }

    #[test]
    fn scanner_ignores_a_paint_word_copied_to_another_stack_slot() {
        let mut ram = painted(1, 1);
        ram[PAGE_SIZE - 16..PAGE_SIZE - 8].copy_from_slice(&stack_paint(1, 511).to_le_bytes());
        records(&mut ram, 1);
        let servers = [server("a", 1)];
        let measured = scan(&ram[..], ram.len() as u64, &servers).unwrap();
        assert!(measured.failures.is_empty());
        assert_eq!(measured.lines[0], "stack a 16 of 1 pages");
    }

    #[test]
    fn scanner_does_not_credit_a_partial_word_that_looks_out_of_range() {
        let mut ram: Vec<u8> = (1..=10).flat_map(|tag| painted(1, tag)).collect();
        let servers: Vec<_> = (1..=10).map(|tag| server(&format!("s{tag}"), 1)).collect();
        let at = 9 * PAGE_SIZE + 402 * UNIT;
        // The rv32 dump's overlapping bytes: an unaligned paint word at +2 leaves an aligned
        // magic/tag at +8 with index 0x5354. Its page slot is 403, not 0x5354 % 512 = 340.
        ram[at..at + 2 * UNIT].copy_from_slice(&[
            0xcd, 0x60, 0x77, 0x13, 0x0a, 0x00, 0x4b, 0x41, 0x54, 0x53, 0x0a, 0x00, 0x4b, 0x41, 0x54, 0x53,
        ]);
        assert_eq!(&ram[at + 2..at + 2 + UNIT], &stack_paint(10, 0x1377).to_le_bytes());
        records(&mut ram, 10);
        let measured = scan(&ram[..], ram.len() as u64, &servers).unwrap();
        assert!(measured.failures.is_empty());
        assert_eq!(measured.lines[9], "stack s10 880 of 1 pages");
    }

    #[test]
    fn scanner_refuses_a_page_congruent_out_of_range_marker() {
        let mut ram: Vec<u8> = (1..=10).flat_map(|tag| painted(1, tag)).collect();
        let servers: Vec<_> = (1..=10).map(|tag| server(&format!("s{tag}"), 1)).collect();
        let index = 0x5354u16;
        let at = 9 * PAGE_SIZE + usize::from(index) % (PAGE_SIZE / UNIT) * UNIT;
        ram[at..at + UNIT].copy_from_slice(&stack_paint(10, index).to_le_bytes());
        assert!(
            scan(&ram[..], ram.len() as u64, &servers)
                .unwrap_err()
                .to_string()
                .contains("s10: stack paint index 21332 is outside its stack")
        );
    }

    /// A capped heap passes with its cap at least twice its peak; an uncapped one is only
    /// reported. The record's cap must be the one the manifest declares, so the record is the
    /// block's.
    #[test]
    fn scanner_reports_each_heap_and_holds_a_cap_to_twice_its_peak() {
        let mut ram = painted(1, 1);
        ram.extend(painted(1, 2));
        // Across a chunk's end: the record is read whatever the read sizes.
        ram.resize(CHUNK - 2 * UNIT, 0);
        ram.extend(record(1, 40, 20));
        ram.extend(record(2, 0, 9));
        ram.resize(ram.len().next_multiple_of(PAGE_SIZE), 0);
        let capped = |cap| [Server { heap_pages: cap, ..server("a", 1) }, server("b", 1)];
        let measured = scan(&ram[..], ram.len() as u64, &capped(40)).unwrap();
        assert!(measured.failures.is_empty(), "{:?}", measured.failures);
        assert_eq!(measured.lines[2..], ["heap a 20 of 40 pages", "heap b 9 pages uncapped"]);
        // The record says 40; a manifest saying otherwise was not what the server ran with.
        let measured = scan(&ram[..], ram.len() as u64, &capped(39)).unwrap();
        assert_eq!(measured.failures, ["a: heap record capped at 40 pages, declared 39"]);
        // One page short of twice the peak.
        let at = CHUNK - 2 * UNIT;
        ram[at..at + 4 * UNIT].copy_from_slice(&record(1, 40, 21));
        let measured = scan(&ram[..], ram.len() as u64, &capped(40)).unwrap();
        assert_eq!(measured.failures, ["a: heap needs 42 pages for twice its 21-page peak, capped at 40"]);
    }

    #[test]
    fn scanner_refuses_a_missing_or_duplicated_heap_record() {
        let mut ram = painted(1, 1);
        let servers = [server("a", 1)];
        let measured = scan(&ram[..], ram.len() as u64, &servers).unwrap();
        assert_eq!(measured.failures, ["a: no heap record found"]);
        // The magic with a tag no server has, or with none, is not a record.
        ram.extend(record(0, 0, 1));
        ram.extend(record(2, 0, 1));
        let measured = scan(&ram[..], ram.len() as u64, &servers).unwrap();
        assert_eq!(measured.failures, ["a: no heap record found"]);
        ram.extend(record(1, 0, 1));
        ram.extend(record(1, 0, 1));
        let error = scan(&ram[..], ram.len() as u64, &servers).unwrap_err();
        assert!(error.to_string().contains("a: heap record found twice"));
    }
}
