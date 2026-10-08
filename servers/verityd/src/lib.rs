//! `verityd`: one verified volume (docs/servers/verityd.md). It holds a volume's range at `blkd`,
//! checks every block it reads through the volume's hash tree up to a root the signed manifest
//! pins, or that the volume's root block gives signed under the manifest's key, and serves the
//! checked blocks on `blkd`'s own protocol to the volume's `littlefsd` (R76 (verified volumes)).
//!
//! It holds no MMIO, interrupt or DMA, and mints nothing. Everything it does is here, so host
//! tests drive the same code against a fake range (`Range`); the program
//! (`src/bin/verityd.rs`) only wires the startup block to it.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;

use redoubt_fileserver::args::{label_set, number};
/// The range at `blkd` as `verityd` uses it: its size, and whole sectors read; `verityd` never
/// writes it.
pub use redoubt_fileserver::range::{Fault, Geometry as Size, Range, SECTOR};
use redoubt_rt::startup::valid_name;
use redoubt_verity::{Geometry, Hash};

pub mod server;
pub mod volume;

pub use server::Verityd;
pub use volume::{Refusal, Volume};

/// An argument `verityd` refuses: it then does not start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadArgs;

/// What the volume is checked against (docs/servers/verityd.md, "The root block, and the two
/// modes").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The root the manifest pins, and the volume's data blocks and their tree.
    Pinned { root: Hash, geometry: Geometry },
    /// The Ed25519 key the volume's root block is signed under, and the lowest version it may
    /// carry.
    Signed { key: [u8; 32], floor: u64 },
}

/// `verityd`'s arguments, all of them `init`'s (servers/init.md, "Volumes").
#[derive(Debug, PartialEq, Eq)]
pub struct Args<'a> {
    /// The manifest's name of the endpoint it receives on (`verity:system`).
    pub endpoint: &'a str,
    /// The volume's label set; empty when `init` passed no `labels=`.
    pub labels: Vec<u64>,
    pub mode: Mode,
}

/// The arguments: `endpoint=NAME` exactly once; one mode, pinned (`root=<64 lowercase hex>` and
/// `blocks=N`) or signed (`key=<64 lowercase hex>` and `floor=N`), each of its two exactly once;
/// and `labels=ID[,ID...]` at most once, its IDs distinct and at most `MAX_LABELS`. Anything
/// else, both modes or a part of one, a block count of 0 or one whose tree does not count in
/// sectors, is refused whole.
pub fn parse_args<'a>(args: impl Iterator<Item = &'a str>) -> Result<Args<'a>, BadArgs> {
    let (mut endpoint, mut labels, mut root, mut blocks) = (None, None, None, None);
    let (mut public, mut floor) = (None, None);
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
                labels = Some(label_set(value).map_err(|_| BadArgs)?);
            }
            "root" => {
                once(root.is_some())?;
                root = Some(redoubt_verity::from_hex(value).ok_or(BadArgs)?);
            }
            "blocks" => {
                once(blocks.is_some())?;
                blocks = Some(Geometry::new(number(value).map_err(|_| BadArgs)?).ok_or(BadArgs)?);
            }
            "key" => {
                once(public.is_some())?;
                public = Some(redoubt_verity::from_hex(value).ok_or(BadArgs)?);
            }
            "floor" => {
                once(floor.is_some())?;
                floor = Some(number(value).map_err(|_| BadArgs)?);
            }
            _ => return Err(BadArgs),
        }
    }
    let mode = match (root, blocks, public, floor) {
        (Some(root), Some(geometry), None, None) => Mode::Pinned { root, geometry },
        (None, None, Some(key), Some(floor)) => Mode::Signed { key, floor },
        _ => return Err(BadArgs),
    };
    Ok(Args { endpoint: endpoint.ok_or(BadArgs)?, labels: labels.unwrap_or_default(), mode })
}

#[cfg(test)]
mod tests;
