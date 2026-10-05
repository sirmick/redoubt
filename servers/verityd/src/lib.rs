//! `verityd`: one verified volume (docs/servers/verityd.md). It holds a volume's range at `blkd`,
//! checks every block it reads through the volume's hash tree up to a root the signed manifest
//! pins, and serves the checked blocks on `blkd`'s own protocol to the volume's `fsd`
//! (R76 (verified volumes)).
//!
//! It holds no MMIO, interrupt or DMA, and mints nothing. Everything it does is here, so host
//! tests drive the same code against a fake range ([`Range`]); the program
//! (`src/bin/verityd.rs`) only wires the startup block to it.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;

use redoubt_rt::abi::MAX_LABELS;
use redoubt_rt::startup::valid_name;
use redoubt_verity::{Geometry, Hash};

pub mod blkd;
pub mod server;
pub mod volume;

pub use server::Verityd;
pub use volume::{Refusal, Volume};

/// `blkd`'s sector.
pub const SECTOR: u32 = 512;

/// A request to the range at `blkd` failed: `blkd` refused it, or the disk did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault;

/// What `blkd`'s `info` says of the range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub sectors: u64,
}

/// The range at `blkd` as `verityd` uses it (libs/wire/tables/blkd.md): its size, and whole
/// sectors read. `verityd` never writes it.
pub trait Range {
    /// The range's length in sectors (`info`).
    fn info(&mut self) -> Result<Size, Fault>;
    /// Reads `out.len() / SECTOR` sectors from `sector`.
    fn read(&mut self, sector: u64, out: &mut [u8]) -> Result<(), Fault>;
}

/// An argument `verityd` refuses: it then does not start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadArgs;

/// `verityd`'s arguments, all of them `init`'s (servers/init.md, "Volumes").
#[derive(Debug, PartialEq, Eq)]
pub struct Args<'a> {
    /// The manifest's name of the endpoint it receives on (`verity:system`).
    pub endpoint: &'a str,
    /// The volume's label set; empty when `init` passed no `labels=`.
    pub labels: Vec<u64>,
    /// The root the manifest pins.
    pub root: Hash,
    /// The volume's data blocks and their tree.
    pub geometry: Geometry,
}

/// A decimal number without leading zeros, the form `init` writes.
fn number(s: &str) -> Result<u64, BadArgs> {
    let canonical =
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    s.parse().ok().filter(|_| canonical).ok_or(BadArgs)
}

/// The arguments: `endpoint=NAME`, `root=<64 lowercase hex>` and `blocks=N` exactly once each,
/// and `labels=ID[,ID...]` at most once, its IDs distinct and at most [`MAX_LABELS`]. Anything
/// else, a block count of 0 or one whose tree does not count in sectors, is refused whole.
pub fn parse_args<'a>(args: impl Iterator<Item = &'a str>) -> Result<Args<'a>, BadArgs> {
    let (mut endpoint, mut labels, mut root, mut blocks) = (None, None, None, None);
    let once = |slot: bool| if slot { Err(BadArgs) } else { Ok(()) };
    for arg in args {
        let (key, value) = arg.split_once('=').ok_or(BadArgs)?;
        match key {
            "endpoint" if valid_name(value) => {
                once(endpoint.is_some())?;
                endpoint = Some(value);
            }
            "labels" => {
                once(labels.is_some())?;
                let mut set = Vec::new();
                for id in value.split(',') {
                    let id = number(id)?;
                    if set.contains(&id) || set.len() >= MAX_LABELS {
                        return Err(BadArgs);
                    }
                    set.try_reserve(1).map_err(|_| BadArgs)?;
                    set.push(id);
                }
                labels = Some(set);
            }
            "root" => {
                once(root.is_some())?;
                root = Some(redoubt_verity::from_hex(value).ok_or(BadArgs)?);
            }
            "blocks" => {
                once(blocks.is_some())?;
                blocks = Some(Geometry::new(number(value)?).ok_or(BadArgs)?);
            }
            _ => return Err(BadArgs),
        }
    }
    Ok(Args {
        endpoint: endpoint.ok_or(BadArgs)?,
        labels: labels.unwrap_or_default(),
        root: root.ok_or(BadArgs)?,
        geometry: blocks.ok_or(BadArgs)?,
    })
}

#[cfg(test)]
mod tests;
