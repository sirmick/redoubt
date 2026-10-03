//! The userland disk's check (docs/kernel/boot.md, R75 (verified userland)): `system.index` parsed
//! strictly, and a module given to the loader only if its object hashes to its entry.

use std::collections::HashMap;

use beamlet_redoubt::userland::{Checked, Index, Malformed, Objects, object_name};
use beamlet_redoubt::{Modules, Unloaded};
use redoubt_client::Error;
use sha2::{Digest, Sha256};

/// An index line for `bytes` under `name`.
fn line(name: &str, bytes: &[u8]) -> String {
    format!("{name} {} {}\n", object_name(&Sha256::digest(bytes).into()), bytes.len())
}

fn refused(text: &str, line: usize, why: &'static str) {
    assert_eq!(Index::parse(text.as_bytes()).unwrap_err(), Malformed { line, why }, "{text:?}");
}

/// One line per object, sorted, each LF-terminated, and nothing else: a single malformed line
/// refuses the whole index, naming its line.
#[test]
fn the_index_is_sorted_one_line_per_module_and_a_malformed_line_is_refused_whole() {
    let good = [line("Elixir.Enum.beam", b"enum"), line("kernel.app", b"app"), line("lists.beam", b"lists")]
        .concat();
    let index = Index::parse(good.as_bytes()).unwrap();
    assert_eq!(index.len(), 3);
    assert_eq!(index.get("lists.beam").unwrap().len, 5);
    assert!(index.get("lists").is_none());
    assert!(Index::parse(b"").unwrap().is_empty());

    let hash = object_name(&Sha256::digest(b"x").into());
    let three = "not three fields apart by single spaces";
    // Out of order, twice, and each malformed form, after a good first line.
    let first = line("a.beam", b"a");
    refused(
        &[line("b.beam", b"b"), line("a.beam", b"a")].concat(),
        2,
        "not after the line before it, in byte order",
    );
    refused(&[first.clone(), first.clone()].concat(), 2, "not after the line before it, in byte order");
    refused(&format!("{first}b.beam {hash} 1"), 2, "no LF at its end");
    refused(&format!("{first}b.beam {hash} 1\r\n"), 2, "not a length in decimal");
    refused(&format!("{first}b.beam  {hash} 1\n"), 2, three);
    refused(&format!("{first}b.beam {hash} 1 x\n"), 2, three);
    refused(&format!("{first}b.beam {hash}\n"), 2, three);
    refused(&format!("{first}\n"), 2, three);
    refused(&format!("{first}b.beam {} 1\n", hash.to_uppercase()), 2, "not 64 lowercase hex digits");
    refused(&format!("{first}b.beam {} 1\n", &hash[1..]), 2, "not 64 lowercase hex digits");
    refused(&format!("{first}b.beam {hash} 01\n"), 2, "not a length in decimal");
    refused(&format!("{first}b.beam {hash} 0\n"), 2, "not a length in decimal");
    refused(&format!("{first}b.beam {hash} +1\n"), 2, "not a length in decimal");
    refused(&format!("{first}b.beam {hash} 99999999999999999999\n"), 2, "not a length in decimal");
    let name = "not a module's or an application resource's file";
    refused(&format!("{first}b/c.beam {hash} 1\n"), 2, name);
    refused(&format!("{first}.b.beam {hash} 1\n"), 2, name);
    refused(&format!("{first}b.erl {hash} 1\n"), 2, name);
    refused(&format!("{first}{}.beam {hash} 1\n", "b".repeat(256)), 2, name);
    refused(&format!("{first}b\u{e9}.beam {hash} 1\n"), 2, name);
    let mut not_utf8 = format!("{first}b").into_bytes();
    not_utf8.extend_from_slice(b"\xff.beam 1\n");
    assert_eq!(Index::parse(&not_utf8).unwrap_err(), Malformed { line: 2, why: "not UTF-8" });
}

/// Objects by name, counting reads, as the userland disk's `fsd` serves them.
#[derive(Default)]
struct Disk {
    files: HashMap<String, Vec<u8>>,
    reads: usize,
}

impl Objects for Disk {
    fn read(&mut self, name: &str, max: u64) -> Result<Vec<u8>, Error> {
        self.reads += 1;
        let mut bytes = self.files.get(name).cloned().ok_or(Error::Rerror)?;
        bytes.truncate(max as usize);
        Ok(bytes)
    }
}

/// A module whose object hashes to its entry loads; a flipped byte, a missing object, a short
/// read and a longer one are each refused for their reason, once, with nothing retried; and a
/// name the index lacks is absent, with nothing read.
#[test]
fn a_module_loads_only_if_its_object_hashes_to_its_entry() {
    let modules: [(&str, &[u8]); 5] = [
        ("good.beam", b"FOR1 good"),
        ("flipped.beam", b"FOR1 flip"),
        ("gone.beam", b"FOR1 gone"),
        ("short.beam", b"FOR1 short"),
        ("long.beam", b"FOR1 long"),
    ];
    let mut names: Vec<&str> = modules.iter().map(|(n, _)| *n).collect();
    names.sort();
    let text: String =
        names.iter().map(|n| line(n, modules.iter().find(|(m, _)| m == n).unwrap().1)).collect();
    let mut disk = Disk::default();
    let at = |bytes: &[u8]| object_name(&Sha256::digest(bytes).into());
    for (name, bytes) in modules {
        let mut stored = bytes.to_vec();
        match name {
            "flipped.beam" => stored[4] ^= 1,
            "short.beam" => {
                stored.pop();
            }
            "long.beam" => stored.push(b'!'),
            _ => {}
        }
        if name != "gone.beam" {
            disk.files.insert(at(bytes), stored);
        }
    }
    let mut checked = Checked::new(Index::parse(text.as_bytes()).unwrap(), disk);
    assert_eq!(checked.load("good.beam"), Ok(b"FOR1 good".to_vec()));
    assert_eq!(
        checked.load("flipped.beam"),
        Err(Unloaded::Refused("its object does not match system.index"))
    );
    assert_eq!(checked.load("gone.beam"), Err(Unloaded::Refused("its object is missing")));
    assert_eq!(checked.load("short.beam"), Err(Unloaded::Refused("its object is short")));
    assert_eq!(checked.load("long.beam"), Err(Unloaded::Refused("its object does not match system.index")));
    assert_eq!(checked.loaded(), 1);
    assert_eq!(checked.load("absent.beam"), Err(Unloaded::Absent));
    assert_eq!(checked.load("good"), Err(Unloaded::Absent), "a module is found by its file");
    assert_eq!(checked.named(), 5);
}

/// An application resource is looked up under its file, `<app>.app`, and checked the same.
#[test]
fn an_application_resource_is_checked_as_a_module_is() {
    let app = b"{application,kernel,[]}.";
    let mut disk = Disk::default();
    disk.files.insert(object_name(&Sha256::digest(app).into()), app.to_vec());
    let mut checked = Checked::new(Index::parse(line("kernel.app", app).as_bytes()).unwrap(), disk);
    assert_eq!(checked.load("kernel.app"), Ok(app.to_vec()));
    assert_eq!(checked.load("kernel.beam"), Err(Unloaded::Absent));
}
