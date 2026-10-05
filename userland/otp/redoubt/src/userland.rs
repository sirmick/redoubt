//! The userland disk, as plain files (docs/kernel/boot.md, R75 (verified userland);
//! docs/userland/beamlet.md, "beamlet on Redoubt"). Each module and application resource the
//! system resolves by name is the file of that name at the root of the userland volume
//! (`Elixir.Enum.beam`, `elixir.app`), read through the volume's `fsd`, which reads it through
//! its `verityd` (docs/servers/verityd.md, R76 (verified volumes)). beamlet checks nothing itself:
//! a block that does not hash to the root the signed manifest pins never reaches `fsd`, which
//! then serves the volume as corrupt.
//!
//! A name the volume does not hold is absent, and the VM's lookup goes on as for any name it
//! lacks. A file that opens but cannot be read whole loads nothing, is said once on the console,
//! and is never looked for anywhere else.

use alloc::vec::Vec;

use crate::{Modules, Unloaded};

/// The longest name a file may have: a module's atom, at most 255 characters, and `.beam`.
const MAX_NAME: usize = 260;

/// Why a file gave nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unread {
    /// The volume holds no such file, or would not open it.
    Absent,
    /// It opened, but a read failed: a block that did not check, or a volume served as corrupt.
    Failed,
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
            Err(Unread::Failed) => Err(Unloaded::Refused("its file could not be read")),
        }
    }
}
