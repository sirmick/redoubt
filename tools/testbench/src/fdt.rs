//! Edits to a flattened device tree, for the boot cases that hand the loader a tree no machine
//! would (docs/testbench.md, "Hostile inputs"): the tree QEMU built for the run, dumped, changed
//! here and given back with `-dtb`. Every edit is made in place. A dropped property becomes
//! `FDT_NOP` tokens, a shortened one keeps its header and leaves its tail as `FDT_NOP`s, and an
//! overwritten one keeps its length; so no byte moves, and the header's offsets and sizes stay
//! true without being rewritten.

use anyhow::{Context, Result, bail, ensure};

use crate::case::TreeEdit;

const MAGIC: u32 = 0xd00d_feed;
const BEGIN_NODE: u32 = 1;
const END_NODE: u32 = 2;
const PROP: u32 = 3;
const NOP: u32 = 4;
const END: u32 = 9;

/// One property as the structure block holds it: its token's offset, its data's length and offset.
struct Property {
    token: usize,
    len: usize,
    data: usize,
}

/// Apply `edits` to `dtb` in place. Each names a property by its path (`/chosen/rng-seed`),
/// which must exist.
pub fn edit(dtb: &mut [u8], edits: &[TreeEdit]) -> Result<()> {
    for edit in edits {
        let (path, what) = match edit {
            TreeEdit::Drop { drop } => (drop, "drop"),
            TreeEdit::Shrink { shrink, .. } => (shrink, "shrink"),
            TreeEdit::Set { set, .. } => (set, "set"),
        };
        let property = find(dtb, path).with_context(|| format!("{what} = {path:?}"))?;
        let padded = |len: usize| len.next_multiple_of(4);
        match edit {
            TreeEdit::Drop { .. } => nop(dtb, property.token..property.data + padded(property.len)),
            TreeEdit::Shrink { to, .. } => {
                ensure!(*to < property.len, "shrink = {path:?} to {to}: it has {} bytes", property.len);
                dtb[property.token + 4..property.token + 8].copy_from_slice(&(*to as u32).to_be_bytes());
                nop(dtb, property.data + padded(*to)..property.data + padded(property.len));
            }
            TreeEdit::Set { words, .. } => {
                ensure!(
                    words.len() * 4 == property.len,
                    "set = {path:?}: {} words for a {}-byte property",
                    words.len(),
                    property.len
                );
                for (i, word) in words.iter().enumerate() {
                    dtb[property.data + i * 4..][..4].copy_from_slice(&word.to_be_bytes());
                }
            }
        }
    }
    Ok(())
}

/// Fill `range` (a token boundary to a token boundary) with `FDT_NOP`.
fn nop(dtb: &mut [u8], range: core::ops::Range<usize>) {
    for word in dtb[range].chunks_exact_mut(4) {
        word.copy_from_slice(&NOP.to_be_bytes());
    }
}

/// The property at `path`: the node's path from the root, then the property's name.
fn find(dtb: &[u8], path: &str) -> Result<Property> {
    let (node, name) = path.rsplit_once('/').context("a property path is /node/.../property")?;
    let node = if node.is_empty() { "/" } else { node };
    let mut found = None;
    walk(dtb, &mut |at, prop, property| {
        if at == node && prop == name {
            found = Some(property);
        }
    })?;
    found.with_context(|| format!("the device tree has no {path}"))
}

/// Every property in `dtb`, with its node's path, in structure order.
fn walk(dtb: &[u8], seen: &mut dyn FnMut(&str, &str, Property)) -> Result<()> {
    let word = |at: usize| -> Result<u32> {
        Ok(u32::from_be_bytes(dtb.get(at..at + 4).context("a device tree cut short")?.try_into()?))
    };
    let string = |at: usize| -> Result<&str> {
        let bytes = dtb.get(at..).context("a device tree cut short")?;
        let end = bytes.iter().position(|b| *b == 0).context("an unterminated string")?;
        Ok(core::str::from_utf8(&bytes[..end])?)
    };
    ensure!(word(0)? == MAGIC, "not a flattened device tree");
    let strings = word(12)? as usize;
    let mut at = word(8)? as usize;
    let mut path: Vec<&str> = Vec::new();
    loop {
        match word(at)? {
            BEGIN_NODE => {
                let name = string(at + 4)?;
                path.push(name);
                at += 4 + (name.len() + 1).next_multiple_of(4);
            }
            END_NODE => {
                path.pop().context("a node ended that never began")?;
                at += 4;
            }
            PROP => {
                let len = word(at + 4)? as usize;
                let name = string(strings + word(at + 8)? as usize)?;
                // The root's name is empty, so its children's paths start with one `/`.
                let node = if path.len() <= 1 { "/".to_string() } else { path.join("/") };
                seen(&node, name, Property { token: at, len, data: at + 12 });
                at += 12 + len.next_multiple_of(4);
            }
            NOP => at += 4,
            END => return Ok(()),
            other => bail!("an unknown device tree token {other:#x} at {at:#x}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tree with `/chosen/rng-seed` (32 bytes), `/cpus/timebase-frequency` (one cell) and a
    /// device under `/soc` with one interrupt, as QEMU's `virt` lays them out.
    fn tree() -> Vec<u8> {
        #[derive(Default)]
        struct Blob {
            structure: Vec<u8>,
            strings: Vec<u8>,
        }
        impl Blob {
            fn word(&mut self, w: u32) -> &mut Self {
                self.structure.extend(w.to_be_bytes());
                self
            }

            fn padded(&mut self, bytes: &[u8]) -> &mut Self {
                self.structure.extend(bytes);
                self.structure.resize(self.structure.len().next_multiple_of(4), 0);
                self
            }

            fn begin(&mut self, name: &str) -> &mut Self {
                self.word(BEGIN_NODE).padded(&[name.as_bytes(), b"\0"].concat())
            }

            fn prop(&mut self, name: &str, value: &[u8]) -> &mut Self {
                let offset = self.strings.len() as u32;
                self.strings.extend(name.as_bytes());
                self.strings.push(0);
                self.word(PROP).word(value.len() as u32).word(offset).padded(value)
            }
        }
        let mut blob = Blob::default();
        blob.begin("")
            .begin("chosen")
            .prop("rng-seed", &[7; 32])
            .prop("bootargs", b"")
            .word(END_NODE)
            .begin("cpus")
            .prop("timebase-frequency", &10_000_000u32.to_be_bytes())
            .word(END_NODE)
            .begin("soc")
            .begin("virtio_mmio@10008000")
            .prop("interrupts", &8u32.to_be_bytes())
            .word(END_NODE)
            .word(END_NODE)
            .word(END_NODE)
            .word(END);
        let mut dtb = vec![0; 40];
        dtb[0..4].copy_from_slice(&MAGIC.to_be_bytes());
        dtb[8..12].copy_from_slice(&40u32.to_be_bytes());
        dtb[12..16].copy_from_slice(&(40 + blob.structure.len() as u32).to_be_bytes());
        dtb.extend(blob.structure);
        dtb.extend(blob.strings);
        dtb
    }

    /// Every property's path, length and data, as a re-walk reads them.
    fn properties(dtb: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut all = Vec::new();
        walk(dtb, &mut |node, name, p| {
            let path = if node == "/" { format!("/{name}") } else { format!("{node}/{name}") };
            all.push((path, dtb[p.data..p.data + p.len].to_vec()));
        })
        .unwrap();
        all
    }

    #[test]
    fn a_dropped_property_is_gone_and_the_rest_still_reads() {
        let mut dtb = tree();
        let before = properties(&dtb);
        assert_eq!(before.len(), 4);
        edit(&mut dtb, &[TreeEdit::Drop { drop: "/chosen/rng-seed".into() }]).unwrap();
        let after = properties(&dtb);
        assert_eq!(after, before[1..].to_vec(), "only rng-seed went; nothing moved");
    }

    #[test]
    fn a_shortened_property_keeps_its_first_bytes() {
        let mut dtb = tree();
        edit(&mut dtb, &[TreeEdit::Shrink { shrink: "/chosen/rng-seed".into(), to: 8 }]).unwrap();
        let after = properties(&dtb);
        assert_eq!(after[0], ("/chosen/rng-seed".to_string(), vec![7; 8]));
        assert_eq!(after.len(), 4, "the properties after it are still there");
        let short = edit(&mut dtb, &[TreeEdit::Shrink { shrink: "/chosen/rng-seed".into(), to: 8 }]);
        assert!(short.is_err(), "a shrink must shorten");
    }

    #[test]
    fn an_overwritten_property_has_the_words_and_its_length() {
        let mut dtb = tree();
        edit(
            &mut dtb,
            &[
                TreeEdit::Set { set: "/cpus/timebase-frequency".into(), words: vec![0] },
                TreeEdit::Set { set: "/soc/virtio_mmio@10008000/interrupts".into(), words: vec![1024] },
            ],
        )
        .unwrap();
        let after = properties(&dtb);
        assert_eq!(after[2].1, 0u32.to_be_bytes());
        assert_eq!(after[3].1, 1024u32.to_be_bytes());
        let wide =
            edit(&mut dtb, &[TreeEdit::Set { set: "/cpus/timebase-frequency".into(), words: vec![0, 0] }]);
        assert!(wide.is_err(), "a set keeps the property's length");
    }

    #[test]
    fn a_property_the_tree_lacks_is_an_error_not_a_quiet_boot() {
        let mut dtb = tree();
        let missing = edit(&mut dtb, &[TreeEdit::Drop { drop: "/chosen/stdout-path".into() }]);
        assert!(missing.unwrap_err().to_string().contains("drop = \"/chosen/stdout-path\""));
        assert_eq!(properties(&dtb).len(), 4, "untouched");
    }
}
