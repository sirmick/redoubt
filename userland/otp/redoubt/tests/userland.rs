//! The userland volume's files (docs/kernel/boot.md, R75 (verified userland)): each module and
//! application resource is the file of its name, read whole; a file that does not open is absent,
//! one whose read fails is refused, and nothing but a module's or a resource's name is asked for.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use beamlet_redoubt::userland::{Disk, Files, Unread, unread, valid_name};
use beamlet_redoubt::{Modules, Unloaded};
use redoubt_client::{Error, Name};

/// Files by name, as the userland volume's `littlefsd` serves them, counting reads; a file named in
/// `failing` is refused with a name other than `not_found`, as a block `verityd` fails is.
#[derive(Default)]
struct Volume {
    files: HashMap<String, Vec<u8>>,
    failing: Vec<String>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Files for Volume {
    fn read(&mut self, name: &str) -> Result<Vec<u8>, Unread> {
        self.asked.lock().unwrap().push(name.into());
        if self.failing.iter().any(|f| f == name) {
            return Err(Unread::Failed("its file could not be read: other"));
        }
        self.files.get(name).cloned().ok_or(Unread::Absent)
    }
}

/// A module loads by its file's name; a name the volume lacks is absent and the lookup goes on;
/// a file refused with any other name is refused, said once, naming it; and a name that is not
/// a module's or a resource's is never asked for.
#[test]
fn a_module_is_its_file_and_a_failed_read_is_refused() {
    let mut volume = Volume::default();
    volume.files.insert("Elixir.Enum.beam".into(), b"FOR1 enum".to_vec());
    volume.files.insert("elixir.app".into(), b"{application,elixir,[]}.".to_vec());
    volume.files.insert("Elixir.Version.beam".into(), b"FOR1 version".to_vec());
    volume.failing.push("Elixir.Version.beam".into());
    let asked = volume.asked.clone();
    let mut disk = Disk::new(volume);
    assert_eq!(disk.load("Elixir.Enum.beam"), Ok(b"FOR1 enum".to_vec()));
    assert_eq!(disk.load("elixir.app"), Ok(b"{application,elixir,[]}.".to_vec()));
    assert_eq!(
        disk.load("Elixir.Version.beam"),
        Err(Unloaded::Refused("its file could not be read: other")),
        "refused, naming the error"
    );
    assert_eq!(disk.load("Elixir.Absent.beam"), Err(Unloaded::Absent));
    assert_eq!(disk.loaded(), 2);
    for name in ["Elixir.Enum", "../Elixir.Enum.beam", "a/b.beam", ".hidden.beam", "b c.beam", "x.erl"] {
        assert!(!valid_name(name), "{name}");
        assert_eq!(disk.load(name), Err(Unloaded::Absent), "{name}");
    }
    assert_eq!(asked.lock().unwrap().len(), 4, "only the four good names were asked for");
    assert!(!valid_name(&format!("{}.beam", "b".repeat(256))));
    assert!(valid_name(&format!("{}.beam", "b".repeat(255))));
}

/// The error's name decides: `not_found` at the open is absent, and silent; `not_found` on a
/// read, `corrupt` and every other name (`Other`), a server gone, or anything else is refused,
/// naming it.
#[test]
fn not_found_at_the_open_is_absent_and_every_other_error_is_refused_by_name() {
    assert_eq!(unread(Error::Rerror(Name::NotFound), true), Unread::Absent);
    assert_eq!(
        unread(Error::Rerror(Name::NotFound), false),
        Unread::Failed("its file could not be read: not_found")
    );
    for at_open in [true, false] {
        assert_eq!(
            unread(Error::Rerror(Name::Other), at_open),
            Unread::Failed("its file could not be read: other")
        );
        assert_eq!(
            unread(Error::Disconnected, at_open),
            Unread::Failed("its file could not be read: disconnected")
        );
        assert_eq!(unread(Error::Unexpected, at_open), Unread::Failed("its file could not be read: failed"));
    }
}
