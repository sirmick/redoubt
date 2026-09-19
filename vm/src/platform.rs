//! Everything the VM needs from the operating system, and nothing more.
//!
//! The VM is `no_std` and has no other way to reach the outside world. A [`Platform`] is chosen
//! by whoever embeds the VM: `beamlet-posix` for development and differential testing on a host,
//! and later a Xous one whose methods are IPC calls to servers. Keeping this surface small is
//! what makes the VM auditable: to know what BEAM code can do to the system, read this trait.
//!
//! Capability discipline: the platform decides what a VM instance may reach. The VM itself holds
//! no ambient authority (no filesystem, no network, no clock it did not get from here).

use alloc::string::String;
use alloc::vec::Vec;

/// Services the host operating system provides to one VM instance.
pub trait Platform {
    /// Monotonic time in microseconds since an arbitrary fixed point. Never goes backwards.
    fn monotonic_us(&mut self) -> u64;

    /// Wall-clock time in microseconds since the Unix epoch, if the platform has a clock.
    fn system_time_us(&mut self) -> Option<u64>;

    /// Block until `deadline` (a [`Platform::monotonic_us`] value) passes, or indefinitely for
    /// `None`. Called only when no process can run. May return early (spuriously, or because an
    /// external event arrived); the VM rechecks its timers either way.
    fn idle(&mut self, deadline: Option<u64>);

    /// Write bytes to the VM's console (the `user` I/O device).
    fn console_write(&mut self, bytes: &[u8]);

    /// Input typed at the console, if any has arrived. Must not block: the VM calls it between
    /// time slices, and [`Platform::idle`] is where it waits (an `idle` call should return when
    /// input arrives). The default is a console with no input at all.
    fn console_read(&mut self) -> ConsoleInput {
        ConsoleInput::Eof
    }

    /// Fill `buf` from a cryptographically secure source. On failure the VM raises rather than
    /// using a weaker source.
    fn random(&mut self, buf: &mut [u8]) -> Result<(), PlatformError>;

    /// The bytes of the `.beam` file for `module`, if this VM may load it. This is the only way
    /// code enters the VM, so it is where a platform enforces signing or an allowlist.
    fn load_module(&mut self, module: &str) -> Option<Vec<u8>>;

    /// The `.app` specification of application `app` (the text of `app.app`), if this VM may
    /// start it. The default is none: applications are then unavailable.
    fn load_app(&mut self, app: &str) -> Option<Vec<u8>> {
        let _ = app;
        None
    }

    /// Where the `.beam` file [`Platform::load_module`] gives for `module` appears in the VM's
    /// own file system, if it does (for `code:which/1`, and tools that read chunks from it).
    fn module_file(&mut self, module: &str) -> Option<String> {
        let _ = module;
        None
    }

    /// The file system this VM may use. The default is none: `file` operations then fail
    /// with `enotsup`.
    fn files(&mut self) -> Option<&mut dyn Files> {
        None
    }

    /// The programs this VM may start, behind ports (`open_port/2`, and so `os:cmd/1`). The
    /// default is none: opening such a port then fails with `eacces`. Output from programs is
    /// an external event: [`Platform::idle`] should return when some arrives.
    fn programs(&mut self) -> Option<&mut dyn Programs> {
        None
    }
}

/// Running other programs. A program is outside the VM altogether (an OS process with rights
/// of its own), so this is a large grant, made only when the embedder chooses to.
pub trait Programs {
    /// Start a program with its standard input and output connected to the VM.
    fn spawn(&mut self, spawn: &Spawn) -> Result<Spawned, FileError>;
    /// Queue bytes for the program's standard input. Must not block.
    fn write(&mut self, handle: u64, data: &[u8]) -> Result<(), FileError>;
    /// Stop talking to the program: close its input and forget its output. The program is not
    /// killed (as in BEAM, it sees end of input and a closed output).
    fn close(&mut self, handle: u64);
    /// The next event from any program, if one has arrived. Must not block.
    fn poll(&mut self) -> Option<(u64, ProgramEvent)>;
}

/// What to start: a command line for the shell, or an executable file and its arguments. Paths
/// are the VM's own, absolute; the platform maps them to its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Program {
    Shell(String),
    Executable { path: String, arg0: Option<String>, args: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spawn {
    pub program: Program,
    /// The program's whole environment (the VM's, with the port's changes applied).
    pub env: Vec<(String, String)>,
    /// Its working directory: a VM path.
    pub cwd: String,
    /// Whether the VM writes to its input (else the program's input is empty).
    pub input: bool,
    /// Whether the VM reads its output (else the output is discarded).
    pub output: bool,
    /// Its error output goes with its output (else wherever the VM's own goes).
    pub stderr_to_stdout: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spawned {
    pub handle: u64,
    /// The OS process id, if the platform has such a thing.
    pub os_pid: Option<u64>,
}

/// Something that happened to a program, in order: output, then its end, then its exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramEvent {
    Output(Vec<u8>),
    /// Its output has ended.
    Eof,
    /// It has exited: the status, or 128 plus the signal that ended it.
    Exit(i32),
}

/// A file system, as the VM's `file` module sees it (through OTP's `prim_file`).
///
/// Paths are absolute within the file system the platform chose to expose (`/` is its root,
/// not necessarily the host's) and already normalized by the VM: no `.` or `..` components, no
/// empty ones. A platform must still refuse anything that would leave its root, such as a
/// symbolic link pointing out of it. Handles are the platform's own numbers; the VM closes a
/// handle when the process that opened it exits.
///
/// Calls are synchronous for now. On xous64 this becomes a 9P client (see DESIGN.md), and the
/// same operations map onto walk/open/read/write/stat/clunk.
pub trait Files {
    fn open(&mut self, path: &str, mode: OpenMode) -> Result<u64, FileError>;
    fn close(&mut self, handle: u64);
    /// Up to `len` bytes from the current position; empty at end of file.
    fn read(&mut self, handle: u64, len: usize) -> Result<Vec<u8>, FileError>;
    /// All of `data`, at the current position (or the end, for a file opened to append).
    fn write(&mut self, handle: u64, data: &[u8]) -> Result<(), FileError>;
    /// Up to `len` bytes at `offset`, not moving the position; empty past the end.
    fn pread(&mut self, handle: u64, offset: u64, len: usize) -> Result<Vec<u8>, FileError>;
    fn pwrite(&mut self, handle: u64, offset: u64, data: &[u8]) -> Result<(), FileError>;
    /// Move the position; returns the new one.
    fn seek(&mut self, handle: u64, to: SeekFrom) -> Result<u64, FileError>;
    /// Cut the file at the current position.
    fn truncate(&mut self, handle: u64) -> Result<(), FileError>;
    fn sync(&mut self, handle: u64) -> Result<(), FileError>;
    fn handle_info(&mut self, handle: u64) -> Result<FileInfo, FileError>;
    /// Information about `path`; about a symbolic link itself unless `follow`.
    fn info(&mut self, path: &str, follow: bool) -> Result<FileInfo, FileError>;
    /// The names in a directory (not `.` or `..`), as raw bytes.
    fn list_dir(&mut self, path: &str) -> Result<Vec<Vec<u8>>, FileError>;
    fn make_dir(&mut self, path: &str) -> Result<(), FileError>;
    fn delete(&mut self, path: &str) -> Result<(), FileError>;
    fn del_dir(&mut self, path: &str) -> Result<(), FileError>;
    fn rename(&mut self, from: &str, to: &str) -> Result<(), FileError>;
    /// The target of a symbolic link.
    fn read_link(&mut self, path: &str) -> Result<Vec<u8>, FileError> {
        let _ = path;
        Err(FileError::Einval)
    }
    /// Set access and modification times (seconds since the Unix epoch).
    fn set_times(&mut self, path: &str, atime: i64, mtime: i64) -> Result<(), FileError> {
        let _ = (path, atime, mtime);
        Err(FileError::Enotsup)
    }
    /// Set the permission bits.
    fn set_permissions(&mut self, path: &str, mode: u32) -> Result<(), FileError> {
        let _ = (path, mode);
        Err(FileError::Enotsup)
    }
    /// Make a symbolic link at `link` whose target is `target`, stored as given (not resolved:
    /// a link is resolved when followed, by the platform, which must keep it inside the root).
    fn make_symlink(&mut self, target: &[u8], link: &str) -> Result<(), FileError> {
        let _ = (target, link);
        Err(FileError::Enotsup)
    }
    /// Make a hard link `new` to the file `existing`.
    fn make_link(&mut self, existing: &str, new: &str) -> Result<(), FileError> {
        let _ = (existing, new);
        Err(FileError::Enotsup)
    }
}

/// How to open a file, from Erlang's modes (`read`, `write`, `append`, `exclusive`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpenMode {
    pub read: bool,
    pub write: bool,
    pub append: bool,
    /// Create the file if it does not exist.
    pub create: bool,
    /// Fail with `eexist` if it does.
    pub exclusive: bool,
    /// Empty it on opening.
    pub truncate: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeekFrom {
    Start(u64),
    Current(i64),
    End(i64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Regular,
    Directory,
    Symlink,
    Other,
}

/// What `file:read_file_info/1` reports. Times are seconds since the Unix epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileInfo {
    pub size: u64,
    pub kind: FileKind,
    pub readable: bool,
    pub writable: bool,
    pub atime: i64,
    pub mtime: i64,
    pub ctime: i64,
    /// Unix permission bits and file type, as `stat` gives them.
    pub mode: u32,
    pub links: u64,
    pub inode: u64,
    pub uid: u32,
    pub gid: u32,
}

/// Why a file operation failed: the POSIX error names Erlang reports (`{error, enoent}`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileError {
    Eacces,
    Ebadf,
    Eexist,
    Einval,
    Eio,
    Eisdir,
    Eloop,
    Emfile,
    Enametoolong,
    Enoent,
    Enospc,
    Enotdir,
    Enotempty,
    Enotsup,
    Eperm,
    Erofs,
    Exdev,
}

impl FileError {
    pub fn name(self) -> &'static str {
        match self {
            FileError::Eacces => "eacces",
            FileError::Ebadf => "ebadf",
            FileError::Eexist => "eexist",
            FileError::Einval => "einval",
            FileError::Eio => "eio",
            FileError::Eisdir => "eisdir",
            FileError::Eloop => "eloop",
            FileError::Emfile => "emfile",
            FileError::Enametoolong => "enametoolong",
            FileError::Enoent => "enoent",
            FileError::Enospc => "enospc",
            FileError::Enotdir => "enotdir",
            FileError::Enotempty => "enotempty",
            FileError::Enotsup => "enotsup",
            FileError::Eperm => "eperm",
            FileError::Erofs => "erofs",
            FileError::Exdev => "exdev",
        }
    }
}

/// What [`Platform::console_read`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleInput {
    /// Nothing yet.
    Nothing,
    Data(Vec<u8>),
    /// The input has ended; there will be no more.
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformError {
    Unavailable,
}
