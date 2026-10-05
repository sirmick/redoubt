//! The userland disk, as plain files (docs/kernel/boot.md, R75 (verified userland);
//! docs/userland/beamlet.md, "beamlet on Redoubt"). Each module and application resource the
//! system resolves by name is the file of that name at the root of the userland volume
//! (`Elixir.Enum.beam`, `elixir.app`), read through the volume's `fsd`, which reads it through
//! its `verityd` (docs/servers/verityd.md, R76 (verified volumes)). beamlet checks nothing itself:
//! a block that does not hash to the root the signed manifest pins never reaches `fsd`, which
//! then serves the volume as corrupt.
//!
//! A name the volume's `fsd` answers `not_found` to is absent, and the VM's lookup goes on as for
//! any name it lacks. Any other refusal, at the open or on a read (`corrupt` from a volume `fsd`
//! serves as corrupt, a block `verityd` failed, a device error), loads nothing, is said once on the
//! console naming the file and the error's name, and is never looked for anywhere else.

use alloc::vec::Vec;

use redoubt_client::{Error, Name};

use crate::{Modules, Unloaded};

/// The longest name a file may have: a module's atom, at most 255 characters, and `.beam`.
const MAX_NAME: usize = 260;

/// Why a file gave nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unread {
    /// The volume's `fsd` answered `not_found` at the open: there is no such file.
    Absent,
    /// Any other refusal, at the open or on a read, and why, naming the error.
    Failed(&'static str),
}

/// What a refusal by the volume's `fsd` means for a lookup, by the error's name
/// (servers/wire.md, "Error names"): `not_found` at the open is absent; any other refusal, and any
/// on a read, failed, with the reason beamlet says, naming the error.
pub fn unread(e: Error, at_open: bool) -> Unread {
    Unread::Failed(match e {
        Error::Rerror(Name::NotFound) if at_open => return Unread::Absent,
        Error::Rerror(Name::NotFound) => "its file could not be read: not_found",
        Error::Rerror(Name::Other) => "its file could not be read: other",
        Error::Disconnected => "its file could not be read: disconnected",
        _ => "its file could not be read: failed",
    })
}

/// Where the files are: the userland volume's `fsd` on the machine.
pub trait Files: Send {
    /// The bytes of the file `name` at the volume's root, read whole.
    fn read(&mut self, name: &str) -> Result<Vec<u8>, Unread>;
}

/// Modules from `files`, by the name the VM asks for.
pub struct Disk<F: Files> {
    files: F,
    loaded: usize,
}

impl<F: Files> Disk<F> {
    pub fn new(files: F) -> Disk<F> { Disk { files, loaded: 0 } }

    /// The files given so far.
    pub fn loaded(&self) -> usize { self.loaded }
}

/// A module's file or an application resource's: printable ASCII, no space, no `/`, not starting
/// with `.`, ending in `.beam` or `.app`, at most [`MAX_NAME`] bytes. Nothing else is asked of
/// the volume, so no name the VM makes up can walk anywhere but its root.
pub fn valid_name(name: &str) -> bool {
    name.len() <= MAX_NAME
        && !name.starts_with('.')
        && (name.ends_with(".beam") || name.ends_with(".app"))
        && name.bytes().all(|b| b.is_ascii_graphic() && b != b'/')
}

impl<F: Files> Modules for Disk<F> {
    fn load(&mut self, file: &str) -> Result<Vec<u8>, Unloaded> {
        if !valid_name(file) {
            return Err(Unloaded::Absent);
        }
        match self.files.read(file) {
            Ok(bytes) => {
                self.loaded += 1;
                Ok(bytes)
            }
            Err(Unread::Absent) => Err(Unloaded::Absent),
            Err(Unread::Failed(why)) => Err(Unloaded::Refused(why)),
        }
    }
}
