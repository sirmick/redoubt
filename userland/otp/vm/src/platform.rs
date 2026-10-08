//! Everything the VM needs from the operating system, and nothing more.
//!
//! The VM is `no_std` and has no other way to reach the outside world. A [`Platform`] is chosen
//! by whoever embeds the VM: `beamlet` for development and differential testing on a host,
//! and `beamlet-redoubt`, whose methods use kernel calls and IPC to servers. Keeping this surface
//! small makes the VM auditable: to know what BEAM code can do to the system, read this trait.
//!
//! Capability discipline: the platform decides what a VM instance may reach. The VM itself holds
//! no ambient authority (no filesystem, no network, no clock it did not get from here).

use alloc::string::String;
use alloc::vec::Vec;

/// Services the host operating system provides to one VM instance. With several schedulers
/// (the `std` feature) it is used from any of their threads, one at a time.
pub trait Platform: crate::sync::Sendable {
    /// Monotonic time in microseconds since an arbitrary fixed point. Never goes backwards.
    fn monotonic_us(&mut self) -> u64;

    /// Wall-clock time in microseconds since the Unix epoch, if the platform has a clock. Without
    /// one the VM's system time is [`system_time_us`]'s.
    fn system_time_us(&mut self) -> Option<u64>;

    /// Block until `deadline` (a [`Platform::monotonic_us`] value) passes, or indefinitely for
    /// `None`. Called only when no process can run. May return early (spuriously, or because an
    /// external event arrived); the VM rechecks its timers either way.
    fn idle(&mut self, deadline: Option<u64>);

    /// Write bytes to the VM's console (the `user` I/O device).
    fn console_write(&mut self, bytes: &[u8]);

    /// The console's current size, if known. A TUI application queries this to lay out its
    /// screen. The default is unknown, which is honest: a platform that has not asked its console
    /// server does not know, and must not claim a size it was never told. On Redoubt the embedder
    /// will answer by asking `/dev/cons` with a fresh `consol` `size` call on each query, not caching it
    /// (docs/USERLAND-API.md, "The console and the `Platform` contract"; answer 162).
    fn console_size(&mut self) -> Option<(u16, u16)> { None }

    /// The shell's driver has drawn its first prompt: the console is read, the banner is up, and
    /// a line is waited for. A platform that times a boot says so (`beamlet:prompt_drawn/0`); the
    /// default is nothing.
    fn prompt_drawn(&mut self) {}

    /// Input typed at the console, if any has arrived. Must not block: the VM calls it between
    /// time slices, and [`Platform::idle`] is where it waits (an `idle` call should return when
    /// input arrives). The default is a console with no input at all.
    fn console_read(&mut self) -> ConsoleInput { ConsoleInput::Eof }

    /// Whether a process of the VM reads the console now. With none, input the platform holds is
    /// no reason for [`Platform::idle`] to return, or the VM would spin on what nobody takes; the
    /// platform keeps it, as little as it can, for the next reader. The default holds nothing.
    fn console_listening(&mut self, _listening: bool) {}

    /// Fill `buf` from a cryptographically secure source. On failure the VM raises rather than
    /// using a weaker source.
    fn random(&mut self, buf: &mut [u8]) -> Result<(), PlatformError>;

    /// The system's `.beam` file for `module`, before the VM searches its code path. Only
    /// [`Lookup::Absent`] permits that search; a refused system object ends this lookup. This
    /// does not prevent code already held by the VM from using `code:load_binary/3`.
    fn load_module(&mut self, module: &str) -> Lookup;

    /// The `.app` specification of application `app` (the text of `app.app`), if this VM may
    /// start it. The default is absent: applications are then unavailable.
    fn load_app(&mut self, app: &str) -> Lookup {
        let _ = app;
        Lookup::Absent
    }

    /// Where the `.beam` file [`Platform::load_module`] gives for `module` appears in the VM's
    /// own file system, if it does (for `code:which/1`, and tools that read chunks from it).
    fn module_file(&mut self, module: &str) -> Option<String> {
        let _ = module;
        None
    }

    /// The file system this VM may use. The default is none: `file` operations then fail
    /// with `enotsup`.
    fn files(&mut self) -> Option<&mut dyn Files> { None }

    /// The programs this VM may start, behind ports (`open_port/2`, and so `os:cmd/1`). The
    /// default is none: opening such a port then fails with `eacces`. Output from programs is
    /// an external event: [`Platform::idle`] should return when some arrives.
    fn programs(&mut self) -> Option<&mut dyn Programs> { None }

    /// The system's own calls, which have no POSIX equivalent: the namespace, calls and serving,
    /// budgets, labels and launching (docs/userland/beamlet.md, "Natives"). The default is none:
    /// the `redoubt` natives then answer `{error, not_supported}`.
    fn system(&mut self) -> Option<&mut dyn System> { None }
}

/// A kernel object the platform holds for Erlang code: the value behind a handle's resource term.
/// Its type is the platform's, which tells the kinds apart; its drop is the platform's too (a
/// handle closes when its last copy is collected). Erlang code cannot make one or read it.
pub type Object = alloc::sync::Arc<crate::sync::AnyShared>;

/// Why a system call was refused: one of Redoubt's error names (`not_found`, `not_a_connection`),
/// never a POSIX one; Erlang sees it as `{error, Name}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused(pub &'static str);

/// A message as the wire carries it (docs/servers/wire.md, "The message convention"): four words,
/// a buffer if the message lends one (`None` for an inline message, which travels in its words
/// alone), and handles. The platform encodes nothing of a protocol; the generated clients do.
pub struct Message {
    pub words: [u64; 4],
    pub buffer: Option<Vec<u8>>,
    pub handles: Vec<Object>,
}

/// One entry of the namespace's table: a path's prefix or a named handle's name, the handle's
/// name when it came as one, and the handle.
pub struct Entry {
    pub path: String,
    pub name: Option<String>,
    pub handle: Object,
}

/// A child budget's spec (docs/kernel/budgets.md, "The calls"); a deadline, in the clock's
/// microseconds, makes it a lease.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BudgetSpec {
    pub pages: u64,
    pub processes: u64,
    pub weight: u64,
    /// `None` when left out: the VM's own label set, the only one a user-class caller's child may
    /// carry (docs/kernel/budgets.md, "Labels on budgets").
    pub labels: Option<Vec<u64>>,
    pub account: u64,
    pub deadline: Option<u64>,
}

/// A budget's limits and use: `(limit, used)` of pages and processes, and of weight `(limit,
/// carved)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub pages: (u64, u64),
    pub processes: (u64, u64),
    pub weight: (u64, u64),
}

/// A native program to start (docs/userland/native.md, "Launching from a session"): every
/// authority it gets is here, from the Erlang caller.
pub struct Launch {
    /// The program's bytes, which the caller read.
    pub image: Vec<u8>,
    /// The budget it runs in, which the caller carved.
    pub budget: Object,
    /// Its namespace: a clean absolute path and a connection each.
    pub namespace: Vec<(String, Object)>,
    /// Its named handles.
    pub handles: Vec<(String, Object)>,
    pub args: Vec<String>,
    pub stack_pages: Option<u64>,
    pub heap_pages: Option<u64>,
}

/// Something that happened for an Erlang process: [`System::poll`] names the process (the number
/// [`Files::asker`] uses) and this, which the VM sends it as a message.
pub enum Event {
    /// A request on an endpoint it serves: `{request, Request, Badge, Account, Labels, {Words,
    /// Buffer, Handles}}`, with `Request` a new resource holding `request` (to answer it; `nil` for
    /// a one-way send).
    Request { request: Option<Object>, badge: u64, account: u64, labels: Vec<u64>, message: Message },
    /// A call it made has ended: `{reply, Ref, {ok, {Words, Buffer, Handles}} | {error, Name}}`,
    /// `Ref` the reference its `call` returned.
    Reply { call: u64, result: Result<Message, Refused> },
    /// A job it launched has ended: `{exit, Job, Cause, Code}`, `Job` the reference its launch
    /// returned.
    Exit { job: u64, cause: &'static str, code: u64 },
}

/// The system's calls (docs/userland/beamlet.md, "Natives"). Each answers at once: a call that
/// waits for a server bounds its wait.
pub trait System {
    /// The handle `path` resolves to and the rest of the path: the longest matching prefix for an
    /// absolute path, the named handle for a name.
    fn lookup(&mut self, path: &str) -> Result<(Object, String), Refused>;
    /// Puts the connection `handle` at `prefix` as well.
    fn bind(&mut self, prefix: &str, handle: &Object) -> Result<(), Refused>;
    /// The table: the namespace's entries in binding order, then the named handles.
    fn table(&mut self) -> Vec<Entry>;
    /// Calls `to` for `asker`, waiting at most `timeout_us` for the reply, which arrives as
    /// [`Event::Reply`] naming `call`. No scheduler waits for it.
    fn call(
        &mut self,
        asker: u64,
        call: u64,
        to: &Object,
        message: Message,
        timeout_us: u64,
    ) -> Result<(), Refused>;
    /// Sends `message` on `to`, one way.
    fn send(&mut self, to: &Object, message: Message) -> Result<(), Refused>;
    /// Serves the endpoint `endpoint` (a receive right) for `asker`: its requests arrive as
    /// [`Event::Request`].
    fn serve(&mut self, asker: u64, endpoint: &Object) -> Result<(), Refused>;
    /// Answers `request` with `reply`.
    fn reply(&mut self, request: &Object, reply: Message) -> Result<(), Refused>;
    /// Carves a child from this VM's own budget, with this VM's labels if `spec` leaves them out.
    fn budget_create(&mut self, spec: &BudgetSpec) -> Result<Object, Refused>;
    /// Destroys `budget` and everything in it.
    fn budget_destroy(&mut self, budget: &Object) -> Result<(), Refused>;
    fn budget_usage(&mut self, budget: &Object) -> Result<Usage, Refused>;
    /// This VM's label set, fixed when its budget was made.
    fn labels(&mut self) -> Vec<u64>;
    /// What this VM was told of itself when it was launched as a session, if it was one.
    fn identity(&mut self) -> Option<Identity> { None }
    /// Starts `launch` for `asker`; its end arrives as [`Event::Exit`] naming `job`.
    fn launch(&mut self, asker: u64, job: u64, launch: Launch) -> Result<(), Refused>;
    /// The next event for a process, if one has come. Must not block; [`Platform::idle`] should
    /// return when one arrives.
    fn poll(&mut self) -> Option<(u64, Event)>;
}

/// What a session's VM is told of itself by the steward that launched it
/// (docs/userland/sessions.md, "What a session is told"): information, not authority, since what
/// it can reach is its handles whatever this says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    /// The principal's name.
    pub principal: String,
    /// Each label of the session's set: its name and id.
    pub labels: Vec<(String, u64)>,
    /// The context's name; `None` for the principal's default context or the console's session.
    pub context: Option<String>,
}

/// The result of a system module or application lookup. Refusal is terminal for this lookup;
/// absence alone permits a caller to look on another authorized path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    Found(Vec<u8>),
    Absent,
    Refused,
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
/// **An operation may finish later.** While the VM has named who asks ([`Files::asker`]), a
/// platform whose operations go to servers (Redoubt's, over 9P) may answer [`FileError::Later`]:
/// it has begun the operation, the VM parks the asking Erlang process, and when
/// [`Files::finished`] names the asker the VM makes the same call again and gets the result. With
/// no asker named (the VM's own code loading) every call finishes before it returns. A host's
/// calls always do.
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
    /// Copies the file `from` to the new file `to` within one file server, which copies it itself:
    /// the bytes copied. `exdev` when the two are on different servers, and by default `enotsup`:
    /// the caller copies through the VM instead.
    fn copy_file(&mut self, from: &str, to: &str) -> Result<u64, FileError> {
        let _ = (from, to);
        Err(FileError::Enotsup)
    }
    /// The target of a symbolic link.
    fn read_link(&mut self, path: &str) -> Result<Vec<u8>, FileError> {
        let _ = path;
        Err(FileError::Einval)
    }
    /// Set access and modification times (seconds since the Unix epoch). By default there are
    /// none to set: [`unsettable`](Files::unsettable).
    fn set_times(&mut self, path: &str, atime: i64, mtime: i64) -> Result<(), FileError> {
        let _ = (atime, mtime);
        self.unsettable(path)
    }
    /// Set the permission bits. By default there are none: [`unsettable`](Files::unsettable).
    fn set_permissions(&mut self, path: &str, mode: u32) -> Result<(), FileError> {
        let _ = mode;
        self.unsettable(path)
    }
    /// Set the owner and group, either `-1` for unchanged. By default there are none:
    /// [`unsettable`](Files::unsettable).
    fn set_owner(&mut self, path: &str, uid: i64, gid: i64) -> Result<(), FileError> {
        let _ = (uid, gid);
        self.unsettable(path)
    }
    /// The answer to setting a field the file system does not have: `enotsup` for a file that is
    /// there, and what looking it up finds otherwise (`enoent`). OTP's `write_file_info` takes
    /// `enotsup` as done, so without the look a change to a file that is not there, as
    /// `File.touch` of one in a missing directory, would be `ok`.
    fn unsettable(&mut self, path: &str) -> Result<(), FileError> {
        self.info(path, true)?;
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
    /// The whole file at `path`, if it is at most `max` bytes (else `Einval`, reading nothing):
    /// by default through `info`, `open`, `read` and `close`, one operation where those are many.
    fn read_file(&mut self, path: &str, max: usize) -> Result<Vec<u8>, FileError> {
        crate::bif::read_whole_file(self, path, max)
    }
    /// Who asks the calls that follow, until the next `asker`: a number the VM gives each Erlang
    /// process; `None` for the VM itself, whose calls finish before they return.
    fn asker(&mut self, asker: Option<u64>) { let _ = asker; }
    /// An asker whose operation, answered [`FileError::Later`], has finished since.
    fn finished(&mut self) -> Option<u64> { None }
    /// The asker has gone: its operation's result, when it comes, is dropped with what it holds.
    fn abandon(&mut self, asker: u64) { let _ = asker; }
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
    /// Whether `mode`, `links`, `inode`, `uid`, `gid`, the device and the access bits are the file's.
    /// A platform with no such fields says `false`, and `file` sees `undefined` for each
    /// (docs/userland/files.md, "Refuse visibly; report only real fields").
    pub unix: bool,
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
    Estale,
    Enomem,
    Efbig,
    Econnrefused,
    Etimedout,
    Ehostunreach,
    Eaddrinuse,
    Enotconn,
    /// Not an error: the operation has begun and finishes later ([`Files`]). It never reaches
    /// Erlang code.
    Later,
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
            FileError::Estale => "estale",
            FileError::Enomem => "enomem",
            FileError::Efbig => "efbig",
            FileError::Econnrefused => "econnrefused",
            FileError::Etimedout => "etimedout",
            FileError::Ehostunreach => "ehostunreach",
            FileError::Eaddrinuse => "eaddrinuse",
            FileError::Enotconn => "enotconn",
            FileError::Later => "eagain",
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

/// The VM's system time: the platform's wall clock, or, on a platform without one, the time
/// since boot counted from the Unix epoch, as a machine without a real-time clock keeps it.
/// Programs that read the time go on working (the logger stamps every event), and a date says
/// 1970. A check that a date has begun, a certificate's `notBefore`, then fails; a check only that
/// one has not passed, a token's expiry, then passes, whatever the token; and times from two
/// boots cannot be ordered, since each counts from 1970.
pub fn system_time_us(platform: &mut dyn Platform) -> u64 {
    match platform.system_time_us() {
        Some(us) => us,
        None => platform.monotonic_us(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A platform ten seconds after boot, with a wall clock or without one.
    struct Clock {
        wall: Option<u64>,
    }

    impl Platform for Clock {
        fn monotonic_us(&mut self) -> u64 { 10_000_000 }

        fn system_time_us(&mut self) -> Option<u64> { self.wall }

        fn idle(&mut self, _deadline: Option<u64>) {}

        fn console_write(&mut self, _bytes: &[u8]) {}

        fn random(&mut self, _buf: &mut [u8]) -> Result<(), PlatformError> { Err(PlatformError::Unavailable) }

        fn load_module(&mut self, _module: &str) -> Lookup { Lookup::Absent }
    }

    #[test]
    fn the_wall_clock_is_the_system_time() {
        assert_eq!(system_time_us(&mut Clock { wall: Some(1_790_000_000_000_000) }), 1_790_000_000_000_000);
    }

    #[test]
    fn without_a_wall_clock_the_epoch_is_boot() {
        assert_eq!(system_time_us(&mut Clock { wall: None }), 10_000_000);
    }
}
