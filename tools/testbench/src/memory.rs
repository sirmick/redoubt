//! A stopped guest's RAM is the witness for first-thread stack use. The paint is public, so any
//! guest, including another server, can forge it; the result measures the paths these cases drove,
//! not an adversarial proof.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use redoubt_client::launch::STACK_PAINT;
use redoubt_sys::PAGE_SIZE;
use serde_json::Value;
use stub::MAX_STACK_PAGES;

use crate::case::{Boot, Program};

const UNIT: usize = 8;
const CHUNK: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct Stack {
    pub name: String,
    pub pages: usize,
}

#[derive(Debug)]
pub struct Measurement {
    pub lines: Vec<String>,
    pub failures: Vec<String>,
}

/// Read exactly the manifest a memory case puts in the bundle, including the case's server
/// overrides. The manifest order is the tag order used by `init`.
pub fn stacks(boot: &Boot, workspace: &Path) -> Result<Vec<Stack>> {
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
            let pages = match server.get("stack_pages") {
                Some(value) => value
                    .as_str()
                    .with_context(|| format!("servers[{i}].stack_pages is not a decimal string"))?
                    .parse::<usize>()?,
                None => redoubt_client::launch::STACK_PAGES,
            };
            ensure!((1..=MAX_STACK_PAGES).contains(&pages), "{name}: invalid stack_pages {pages}");
            Ok(Stack { name: name.to_string(), pages })
        })
        .collect()
}

/// Scan every aligned RAM unit, recording where each tagged stack unit survived. A duplicate
/// unit is ambiguous even if the copy came from the guest's ordinary data.
pub fn scan(mut ram: impl Read, bytes: u64, servers: &[Stack]) -> Result<Measurement> {
    let mut seen: Vec<Vec<bool>> = servers.iter().map(|s| vec![false; s.pages * PAGE_SIZE / UNIT]).collect();
    let mut chunk = [0u8; CHUNK];
    let mut left = bytes;
    while left > 0 {
        let n = left.min(CHUNK as u64) as usize;
        ram.read_exact(&mut chunk[..n]).context("reading the QMP RAM dump")?;
        let first_unit = (bytes - left) / UNIT as u64;
        for (at, unit) in chunk[..n].chunks_exact(UNIT).enumerate() {
            let word = u64::from_le_bytes(unit.try_into().unwrap());
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
        if server.pages < required {
            failures.push(format!(
                "{}: stack needs {required} pages for twice its {peak}-byte peak, declared {}",
                server.name, server.pages
            ));
        }
        lines.push(format!("stack {} {peak} of {} pages", server.name, server.pages));
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

    #[test]
    fn scanner_measures_the_lowest_missing_unit() {
        let mut ram = painted(2, 1);
        let used = 200 * UNIT;
        let len = ram.len();
        ram[len - used..].fill(0);
        let servers = [Stack { name: "a".into(), pages: 2 }];
        let measured = scan(&ram[..], ram.len() as u64, &servers).unwrap();
        assert_eq!(measured.lines, ["stack a 1600 of 2 pages"]);
        assert!(measured.failures.is_empty());
    }

    #[test]
    fn scanner_refuses_missing_duplicate_and_too_little_margin() {
        let server = [Stack { name: "a".into(), pages: 1 }];
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
        let measured = scan(&used[..], used.len() as u64, &server).unwrap();
        assert_eq!(measured.lines, ["stack a 3000 of 1 pages"]);
        assert!(measured.failures[0].contains("stack needs"));
    }

    #[test]
    fn scanner_ignores_a_paint_word_copied_to_another_stack_slot() {
        let mut ram = painted(1, 1);
        ram[PAGE_SIZE - 16..PAGE_SIZE - 8].copy_from_slice(&stack_paint(1, 511).to_le_bytes());
        let servers = [Stack { name: "a".into(), pages: 1 }];
        let measured = scan(&ram[..], ram.len() as u64, &servers).unwrap();
        assert!(measured.failures.is_empty());
        assert_eq!(measured.lines, ["stack a 16 of 1 pages"]);
    }

    #[test]
    fn scanner_does_not_credit_a_partial_word_that_looks_out_of_range() {
        let mut ram: Vec<u8> = (1..=10).flat_map(|tag| painted(1, tag)).collect();
        let servers: Vec<_> = (1..=10).map(|tag| Stack { name: format!("s{tag}"), pages: 1 }).collect();
        let at = 9 * PAGE_SIZE + 402 * UNIT;
        // The rv32 dump's overlapping bytes: an unaligned paint word at +2 leaves an aligned
        // magic/tag at +8 with index 0x5354. Its page slot is 403, not 0x5354 % 512 = 340.
        ram[at..at + 2 * UNIT].copy_from_slice(&[
            0xcd, 0x60, 0x77, 0x13, 0x0a, 0x00, 0x4b, 0x41, 0x54, 0x53, 0x0a, 0x00, 0x4b, 0x41, 0x54, 0x53,
        ]);
        assert_eq!(&ram[at + 2..at + 2 + UNIT], &stack_paint(10, 0x1377).to_le_bytes());
        let measured = scan(&ram[..], ram.len() as u64, &servers).unwrap();
        assert!(measured.failures.is_empty());
        assert_eq!(measured.lines[9], "stack s10 880 of 1 pages");
    }

    #[test]
    fn scanner_refuses_a_page_congruent_out_of_range_marker() {
        let mut ram: Vec<u8> = (1..=10).flat_map(|tag| painted(1, tag)).collect();
        let servers: Vec<_> = (1..=10).map(|tag| Stack { name: format!("s{tag}"), pages: 1 }).collect();
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
}
