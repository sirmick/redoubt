//! The userland disk, checked object by object (docs/kernel/boot.md, R75 (verified userland);
//! docs/userland/beamlet.md, "beamlet on Redoubt"). `system.index`, in the signed bundle, names
//! each module and application resource the system resolves by name, by the file the VM asks for
//! (`Elixir.Enum.beam`, `elixir.app`), with the SHA-256 and length of its bytes; the disk holds each as one
//! object, `/<sha256 hex>`, served by an `fsd` the VM does not trust. A name's bytes reach the loader only if
//! they hash to the index's entry: a mismatch, a missing object or a short read loads nothing, is said once
//! on the console, and is never retried or looked for anywhere else.

use alloc::string::String;
use alloc::vec::Vec;

use redoubt_client::Error;
use sha2::{Digest, Sha256};

use crate::{Modules, Unloaded};

/// One entry of `system.index`: an object's SHA-256 and its length in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub sha256: [u8; 32],
    pub len: u64,
}

/// Why `system.index` was refused whole: the line, from 1, and what was wrong with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Malformed {
    pub line: usize,
    pub why: &'static str,
}

/// `system.index`, parsed strictly: one line per object, `<file> <sha256 hex> <bytes>`, sorted by
/// file with none twice, each LF-terminated, and nothing else.
#[derive(Debug)]
pub struct Index {
    entries: Vec<(String, Entry)>,
}

/// The longest name an entry may have: a module's atom, at most 255 characters, and `.beam`.
const MAX_NAME: usize = 260;

impl Index {
    /// Parses `bytes`; a single malformed line refuses the whole index.
    pub fn parse(bytes: &[u8]) -> Result<Index, Malformed> {
        let mut entries: Vec<(String, Entry)> = Vec::new();
        let mut rest = bytes;
        let mut line = 0;
        while !rest.is_empty() {
            line += 1;
            let bad = |why| Malformed { line, why };
            let Some(end) = rest.iter().position(|&b| b == b'\n') else {
                return Err(bad("no LF at its end"));
            };
            let text = core::str::from_utf8(&rest[..end]).map_err(|_| bad("not UTF-8"))?;
            rest = &rest[end + 1..];
            let mut fields = text.split(' ');
            let (Some(name), Some(hex), Some(len), None) =
                (fields.next(), fields.next(), fields.next(), fields.next())
            else {
                return Err(bad("not three fields apart by single spaces"));
            };
            if !valid_name(name) {
                return Err(bad("not a module's or an application resource's file"));
            }
            let sha256 = parse_hex(hex).ok_or_else(|| bad("not 64 lowercase hex digits"))?;
            let len = parse_len(len).ok_or_else(|| bad("not a length in decimal"))?;
            if entries.last().is_some_and(|(last, _)| last.as_bytes() >= name.as_bytes()) {
                return Err(bad("not after the line before it, in byte order"));
            }
            entries.push((String::from(name), Entry { sha256, len }));
        }
        Ok(Index { entries })
    }

    /// The entry for the file `name`, `<module>.beam` or `<app>.app`.
    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.entries.binary_search_by(|(n, _)| n.as_str().cmp(name)).ok().map(|i| &self.entries[i].1)
    }

    pub fn len(&self) -> usize { self.entries.len() }

    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

/// A module's file or an application resource's: printable ASCII, no space, no `/`, not starting
/// with `.`, ending in `.beam` or `.app`, at most [`MAX_NAME`] bytes.
fn valid_name(name: &str) -> bool {
    name.len() <= MAX_NAME
        && !name.starts_with('.')
        && (name.ends_with(".beam") || name.ends_with(".app"))
        && name.bytes().all(|b| b.is_ascii_graphic() && b != b'/')
}

fn parse_hex(hex: &str) -> Option<[u8; 32]> {
    let digit = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    };
    let hex = hex.as_bytes();
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, pair) in hex.chunks(2).enumerate() {
        out[i] = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Some(out)
}

/// A length: decimal digits, no sign, no leading zero, above zero.
fn parse_len(len: &str) -> Option<u64> {
    if len.is_empty() || len.starts_with('0') || !len.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    len.parse().ok()
}

/// The lowercase hex of `sha256`: an object's name on the disk.
pub fn object_name(sha256: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    sha256.iter().flat_map(|b| [HEX[usize::from(b >> 4)] as char, HEX[usize::from(b & 15)] as char]).collect()
}

/// Where the objects are: the userland disk's `fsd` on the machine.
pub trait Objects: Send {
    /// The bytes of the file `name` at the volume's root, at most `max` of them: reading stops
    /// there, so an object longer than its entry costs no more than one byte past it.
    fn read(&mut self, name: &str, max: u64) -> Result<Vec<u8>, Error>;
}

/// Modules from `objects`, each checked against `index` before it is given.
pub struct Checked<O: Objects> {
    index: Index,
    objects: O,
    loaded: usize,
}

impl<O: Objects> Checked<O> {
    pub fn new(index: Index, objects: O) -> Checked<O> { Checked { index, objects, loaded: 0 } }

    /// The index's objects.
    pub fn named(&self) -> usize { self.index.len() }

    /// The objects given so far, each checked.
    pub fn loaded(&self) -> usize { self.loaded }
}

impl<O: Objects> Modules for Checked<O> {
    /// A file the index does not have is absent; one it has loads only if its object's bytes are
    /// the entry's.
    fn load(&mut self, file: &str) -> Result<Vec<u8>, Unloaded> {
        let Some(entry) = self.index.get(file).copied() else { return Err(Unloaded::Absent) };
        let bytes = match self.objects.read(&object_name(&entry.sha256), entry.len.saturating_add(1)) {
            Ok(bytes) => bytes,
            Err(Error::Rerror) => return Err(Unloaded::Refused("its object is missing")),
            Err(_) => return Err(Unloaded::Refused("its object could not be read")),
        };
        if (bytes.len() as u64) < entry.len {
            return Err(Unloaded::Refused("its object is short"));
        }
        if bytes.len() as u64 != entry.len || Sha256::digest(&bytes)[..] != entry.sha256[..] {
            return Err(Unloaded::Refused("its object does not match system.index"));
        }
        self.loaded += 1;
        Ok(bytes)
    }
}
