//! The boot pack (docs/userland/beamlet.md, "beamlet on Redoubt"): a lookup takes a name the pack
//! holds from it and never asks the volume; a name it does not hold is the volume's file, as
//! before; the pack goes once every entry has been taken; and a pack that does not check, whole,
//! is refused before any of it is used.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use beamlet_redoubt::Modules;
use beamlet_redoubt::pack::{MAGIC, Pack, VERSION};
use beamlet_redoubt::userland::{Disk, Files, Unread};

/// Files by name, counting what was asked of them.
#[derive(Default)]
struct Volume {
    files: HashMap<String, Vec<u8>>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Files for Volume {
    fn read(&mut self, name: &str) -> Result<Vec<u8>, Unread> {
        self.asked.lock().unwrap().push(name.into());
        self.files.get(name).cloned().ok_or(Unread::Absent)
    }
}

/// A module the loader would read as far as its name: `FOR1`, `BEAM` and an `AtU8` chunk in
/// OTP 28's form whose first atom is `name`, then `rest`.
fn beam(name: &str, rest: &[u8]) -> Vec<u8> {
    let mut atoms = (-1i32).to_be_bytes().to_vec();
    let n = name.len();
    if n < 16 {
        atoms.push((n as u8) << 4);
    } else {
        atoms.extend([(((n >> 3) & 0xe0) as u8) | 0x08, (n & 0xff) as u8]);
    }
    atoms.extend(name.as_bytes());
    atoms.extend(rest);
    let mut body = b"BEAMAtU8".to_vec();
    body.extend((atoms.len() as u32).to_be_bytes());
    body.extend(&atoms);
    while body.len() % 4 != 0 {
        body.push(0);
    }
    let mut out = b"FOR1".to_vec();
    out.extend((body.len() as u32).to_be_bytes());
    out.extend(body);
    out
}

/// A pack of `entries`, in their order, written as the image's builder writes one.
fn pack(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut index = Vec::new();
    let header = 12 + entries.iter().map(|(n, _)| 10 + n.len()).sum::<usize>();
    let mut offset = header;
    for (name, bytes) in entries {
        index.extend((name.len() as u16).to_le_bytes());
        index.extend(name.as_bytes());
        index.extend((offset as u32).to_le_bytes());
        index.extend((bytes.len() as u32).to_le_bytes());
        offset += bytes.len();
    }
    let mut out = MAGIC.to_vec();
    out.extend(VERSION.to_le_bytes());
    out.extend((entries.len() as u32).to_le_bytes());
    out.extend(index);
    for (_, bytes) in entries {
        out.extend(*bytes);
    }
    out
}

/// A name the pack holds is its bytes, with nothing asked of the volume, as often as it is
/// asked for while the pack lasts; a name it lacks is read from the volume; once every entry has
/// been taken the pack goes, and a later lookup of one of its names reads the volume's file.
#[test]
fn a_packed_module_comes_from_the_pack_and_any_other_from_the_volume() {
    let (enum_, lists) = (beam("Elixir.Enum", b"enum"), beam("lists", b"lists"));
    let bytes = pack(&[
        ("Elixir.Enum.beam", &enum_),
        ("elixir.app", b"{application,elixir,[]}."),
        ("lists.beam", &lists),
    ]);
    let pack = Pack::parse(bytes.clone()).unwrap();
    assert_eq!((pack.len(), pack.size()), (3, bytes.len()));
    let mut volume = Volume::default();
    volume.files.insert("Elixir.Version.beam".into(), b"FOR1 version".to_vec());
    volume.files.insert("lists.beam".into(), b"FOR1 lists from the volume".to_vec());
    let asked = Arc::clone(&volume.asked);
    let mut disk = Disk::with_pack(volume, Some(pack));
    assert!(disk.packs("lists.beam") && !disk.packs("Elixir.Version.beam"));

    assert_eq!(disk.load("Elixir.Enum.beam").unwrap(), enum_);
    assert_eq!(disk.load("Elixir.Enum.beam").unwrap(), enum_);
    assert_eq!(disk.load("elixir.app").unwrap(), b"{application,elixir,[]}.");
    assert_eq!(disk.load("Elixir.Version.beam").unwrap(), b"FOR1 version");
    assert!(asked.lock().unwrap().iter().eq(["Elixir.Version.beam"]));
    assert_eq!(disk.packed(), 3);

    // The last entry spends the pack.
    assert_eq!(disk.load("lists.beam").unwrap(), lists);
    assert!(!disk.packs("lists.beam"));
    assert_eq!(disk.load("lists.beam").unwrap(), b"FOR1 lists from the volume");
    assert_eq!(disk.packed(), 4);
    assert!(asked.lock().unwrap().iter().eq(["Elixir.Version.beam", "lists.beam"]));
}

/// A pack is checked whole before any of it is used: a truncated entry, an entry whose length
/// is wrong either way, a module whose own name is not its entry's, an index out of order or
/// naming a name twice, a name that is not a module's or a resource's, and a file that is not a
/// pack of this version are each refused, with why.
#[test]
fn a_pack_with_a_bad_entry_is_refused_whole() {
    let (a, b) = (beam("a", b"aaaa"), beam("b", b"bbbb"));
    let good = pack(&[("a.beam", &a), ("b.beam", &b)]);
    assert!(Pack::parse(good.clone()).is_ok());
    let refused = |bytes: Vec<u8>| Pack::parse(bytes).err().expect("refused");

    assert_eq!(refused(good[..good.len() - 1].to_vec()), "an entry runs past the file's end");
    let mut longer = good.clone();
    longer.push(0);
    assert_eq!(refused(longer), "the file goes on past its last entry");
    // `a.beam`'s length, one more than it is: `b.beam` no longer starts where it ends.
    let mut wrong = good.clone();
    let at = 12 + 2 + "a.beam".len() + 4;
    wrong[at] += 1;
    assert_eq!(refused(wrong), "an entry's bytes are not where the one before it ends");
    assert_eq!(
        refused(pack(&[("a.beam", &a), ("c.beam", &b)])),
        "an entry's module is not the one its name says"
    );
    assert_eq!(
        refused(pack(&[("a.beam", &b[..b.len() - 4])])),
        "an entry's module is not the one its name says"
    );
    assert_eq!(
        refused(pack(&[("b.beam", &b), ("a.beam", &a)])),
        "its index is not in strictly ascending name order"
    );
    assert_eq!(
        refused(pack(&[("a.beam", &a), ("a.beam", &a)])),
        "its index is not in strictly ascending name order"
    );
    assert_eq!(refused(pack(&[("../a.beam", &a)])), "an entry's name is not a module's or a resource's");
    assert_eq!(refused(pack(&[("a.erl", &a)])), "an entry's name is not a module's or a resource's");
    assert_eq!(refused(good[..20].to_vec()), "its index is truncated");
    let mut count = good.clone();
    count[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(refused(count), "its entry count is larger than the file");
    let mut magic = good.clone();
    magic[0] ^= 1;
    assert_eq!(refused(magic), "it is not a boot pack");
    let mut version = good;
    version[4] += 1;
    assert_eq!(refused(version), "its version is not this beamlet's");

    // A module name long enough for the two-byte length form.
    let long = "Elixir.Redoubt.Shell.Evaluator";
    assert!(Pack::parse(pack(&[(&format!("{long}.beam"), &beam(long, b""))])).is_ok());
}
