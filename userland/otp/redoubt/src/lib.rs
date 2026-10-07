//! beamlet's `Platform` on Redoubt (docs/userland/beamlet.md, "beamlet on Redoubt"): the VM's
//! one boundary, answered by the client library (`redoubt-client`) and the runtime's kernel calls.
//!
//! It serves the console, the clock and randomness; the module source is the embedder's
//! ([`Modules`]): the userland volume's files on the machine, read through a verified volume
//! ([`userland`]), directories on a host; files are the namespace's, over 9P ([`files`]); there
//! are no programs yet. Its I/O is asynchronous
//! underneath ([`io`]; beamlet.md, "Asynchronous underneath, synchronous on top"): a request goes
//! through the client library's hub from the VM's own thread without waiting for its answer, a
//! waiter thread per connection collects the answers, and [`Platform::idle`] is where the VM
//! waits for them. The VM's thread waits for a server itself only for typed calls, which no hub
//! carries (the console's size, `littlefsd`'s rename), and for module lookups, since loading code
//! is synchronous in the VM.
//!
//! - **The console** is `/dev/cons` in the process's namespace, opened once. One read is out on the hub at a
//!   time, parked by the server until there is typing, and one write: the bytes the VM writes wait here, in
//!   order, for the write before them, since the console's share is a page a badge and a write is a page.
//! - **Time** is the kernel's microseconds since boot, and `system_time_us` is `None`: there is no wall clock
//!   until M5 (persist, install, share) brings one. **Randomness** is the kernel's.
//!
//! The same code runs on the machine and, on a host, on the fake kernel: only where modules come
//! from differs ([`Modules`]). [`run`] is the program both run: the machine's `beamlet` and the
//! host's `fake-redoubt`.

#![cfg_attr(not(feature = "fake"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod files;
#[cfg(feature = "fake")]
pub mod fixture;
pub mod io;
mod jobs;
pub mod pack;
mod pool;
mod serve;
pub mod system;
pub mod userland;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::sync::Arc;
use alloc::vec::Vec;

use beamlet_vm::bif::NativeSpec;
use beamlet_vm::memory::HeapPages;
use beamlet_vm::platform::{ConsoleInput, Files, Lookup, Platform, PlatformError, System};
use beamlet_vm::vm::{Config, Limits};
use beamlet_vm::{Class, Vm};
pub use files::posix;
use redoubt_client::aio::{Conn, Done, MAX_WRITE, Outcome};
use redoubt_client::console::Console;
use redoubt_client::ns::Namespace;
use redoubt_client::{Error, Lend, Refusal};
use redoubt_rt::abi::{FOREVER, Handle, PAGE_SIZE};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::Buffer;
use redoubt_rt::startup::Startup;

use crate::io::Io;

/// Where the VM's modules and applications come from, by file name (`lists.beam`, `kernel.app`).
/// `Send`, as the platform is: the VM's schedulers may share it.
pub trait Modules: Send {
    fn load(&mut self, file: &str) -> Result<Vec<u8>, Unloaded>;

    /// The loads the boot pack answered so far ([`pack`]), which `boot-stats` says apart.
    fn packed(&self) -> u64 { 0 }
}

/// Why [`Modules::load`] gave nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unloaded {
    /// The source has no such name: the VM's lookup goes on, as for any name it lacks.
    Absent,
    /// The source has it, but could not give its bytes, for this reason (a block that did not
    /// check under R76): the platform says so on the console, and the VM finds nothing
    /// (R75 (verified userland)).
    Refused(&'static str),
}

/// The most bytes the VM may have written that the console has not yet taken: past it, a write
/// waits for the console in place, as a write to a slow terminal does.
const MAX_OUTPUT: usize = 64 * 1024;
/// How long the platform's end waits for the console to take what is still to write (µs).
const FLUSH_US: u64 = 2_000_000;

/// The platform of one VM, on Redoubt.
pub struct Redoubt {
    lend: Lend,
    /// The console's open file, for its size; its I/O goes through [`Redoubt::io`].
    console: Arc<Console>,
    io: Io,
    cons: ConsoleIo,
    /// The namespace's files, open files and file operations.
    files: files::Table,
    /// The system calls' handles and events ([`system`]).
    sys: system::Sys,
    /// Say what the I/O cost when the VM ends ([`REPORT_IO`]).
    report_io: bool,
    modules: Box<dyn Modules>,
    /// What the lookups cost (`boot-stats`).
    #[cfg(feature = "boot-stats")]
    loads: Loads,
}

/// The console on the hub: at most one read out, parked by the server until there is typing, and at
/// most one write out, with the bytes after it waiting here in order.
struct ConsoleIo {
    conn: Conn,
    fid: u32,
    /// The read out, by its tag; and a read's buffer while none is.
    reading: Option<u16>,
    read_buffer: Option<Buffer>,
    /// What was read and the VM has not taken yet.
    input: VecDeque<u8>,
    /// Whether reading has begun, and whether the input has ended.
    started: bool,
    ended: bool,
    /// The write out, by its tag; and a write's buffer while none is.
    writing: Option<u16>,
    write_buffer: Option<Buffer>,
    /// What the VM wrote that the console has not taken, the write out's bytes first.
    output: VecDeque<u8>,
    /// The console took nothing, or has gone: nothing more is written.
    gone: bool,
}

impl ConsoleIo {
    /// Takes `done` if it is the console's read or write; anything else, a file operation on
    /// `/dev/cons` on the same connection among them, is handed back.
    fn take(&mut self, io: &mut Io, done: Done) -> Option<Done> {
        if done.conn != self.conn || (self.reading != Some(done.tag) && self.writing != Some(done.tag)) {
            return Some(done);
        }
        if self.reading == Some(done.tag) {
            self.reading = None;
            match done.outcome {
                Outcome::Read(0) => self.ended = true,
                Outcome::Read(n) => {
                    if let Some(buffer) = &done.buffer {
                        self.input.extend(&buffer[..n.min(buffer.len())]);
                    }
                }
                // Over the console's share for a moment: asked again below.
                Outcome::Busy => {}
                // Refused, flushed, or the console has gone: there is no more input.
                _ => self.ended = true,
            }
            self.read_buffer = done.buffer;
            self.read(io);
        } else if self.writing == Some(done.tag) {
            self.writing = None;
            match done.outcome {
                Outcome::Wrote(n) if n > 0 => drop(self.output.drain(..(n as usize).min(self.output.len()))),
                Outcome::Busy => {}
                // A console that takes nothing, or has gone, gets no more.
                _ => self.stop_writing(),
            }
            self.write_buffer = done.buffer;
            self.write(io);
        }
        None
    }

    /// Puts a read out, unless one is or the input has ended.
    fn read(&mut self, io: &mut Io) {
        if self.reading.is_some() || self.ended {
            return;
        }
        let buffer = match self.read_buffer.take() {
            Some(buffer) => Ok(buffer),
            None => Buffer::new(1).map_err(Error::from),
        };
        match buffer.and_then(|buffer| io.request().read(self.conn, self.fid, 0, buffer)) {
            Ok(tag) => self.reading = Some(tag),
            // With nothing to read into, or no way to ask, there is no input.
            Err(_) => self.ended = true,
        }
    }

    /// Puts the next write out, unless one is or there is nothing to write.
    fn write(&mut self, io: &mut Io) {
        if self.writing.is_some() || self.output.is_empty() || self.gone {
            return;
        }
        let buffer = match self.write_buffer.take() {
            Some(buffer) => Ok(buffer),
            None => Buffer::new(1).map_err(Error::from),
        };
        let written = buffer.and_then(|mut buffer| {
            let n = self.output.len().min(MAX_WRITE).min(buffer.len());
            for (slot, byte) in buffer[..n].iter_mut().zip(self.output.iter()) {
                *slot = *byte;
            }
            io.request().write(self.conn, self.fid, 0, buffer, n)
        });
        match written {
            Ok(tag) => self.writing = Some(tag),
            Err(_) => self.stop_writing(),
        }
    }

    fn stop_writing(&mut self) {
        self.gone = true;
        self.output.clear();
    }

    /// Whether anything written is still to reach the console.
    fn writes_pending(&self) -> bool { !self.gone && (self.writing.is_some() || !self.output.is_empty()) }
}

/// What the VM's lookups cost, said at its first console read (`boot-stats`, test-only, for the
/// bench's boot-profile cases): each outcome's count, the bytes found, and the guest time spent
/// inside them, against the time since the platform started.
#[cfg(feature = "boot-stats")]
#[derive(Default)]
struct Loads {
    found: u64,
    absent: u64,
    refused: u64,
    bytes: u64,
    us: u64,
    started: u64,
}

/// ` [t=N]`: `time_now` in µs, the stamp on a line the boot profile times (`boot-stats`).
#[cfg(feature = "boot-stats")]
pub fn stamp() -> alloc::string::String { format!(" [t={}]", redoubt_rt::handle::time_now().unwrap_or(0)) }

impl Redoubt {
    /// The platform of a process started with `startup`, whose namespace holds `/dev/cons`, which
    /// serves multiplexed sessions.
    pub fn new(startup: &Startup, modules: Box<dyn Modules>) -> Result<Redoubt, Error> {
        // A bind it cannot make is refused before any server is asked anything.
        let binds = binds(startup).map_err(|_| Refusal::BadPath)?;
        let mut lend = Lend::new(1)?;
        let mut ns = Namespace::from_startup(startup, &mut lend)?;
        for (prefix, handle) in binds {
            let conn = redoubt_client::file::Connection::attach(Endpoint::from_handle(handle), &mut lend)?;
            ns.bind(prefix, conn)?;
        }
        let console = Arc::new(Console::open(&ns, &mut lend)?);
        let mut io = Io::new()?;
        // Before any waiter: the first wake-up on the VM's endpoint is the first call thread's.
        let (pool, labels) = pool::Pool::start(io.wake()).map_err(|_| Error::Unexpected)?;
        let sys = system::Sys::new(startup, &ns, labels, pool);
        let conn = io.connect(console.file().connection())?;
        let cons = ConsoleIo {
            conn,
            fid: console.file().fid(),
            reading: None,
            read_buffer: None,
            input: VecDeque::new(),
            started: false,
            ended: false,
            writing: None,
            write_buffer: None,
            output: VecDeque::new(),
            gone: false,
        };
        Ok(Redoubt {
            lend,
            console,
            io,
            cons,
            files: files::Table::new(ns),
            sys,
            report_io: startup.args().any(|arg| arg == REPORT_IO),
            modules,
            #[cfg(feature = "boot-stats")]
            loads: Loads { started: redoubt_rt::handle::time_now().unwrap_or(0), ..Loads::default() },
        })
    }

    /// The waiter threads started so far: one per connection the VM has used.
    pub fn waiters(&self) -> usize { self.io.waiters() }

    /// The requests handed to the hub so far.
    pub fn requests(&self) -> u64 { self.io.requests() }

    /// Starts reading the console, the first time input is asked for. A `boot-stats` build says so,
    /// with the time and what the lookups cost: for the shell, its prompt is drawn and waiting.
    fn start_reading(&mut self) {
        #[cfg(feature = "boot-stats")]
        {
            self.console_write(format!("beamlet: first console read{}\n", stamp()).as_bytes());
            let Loads { found, absent, refused, bytes, us, started } = self.loads;
            let since = redoubt_rt::handle::time_now().unwrap_or(0).saturating_sub(started);
            let packed = self.modules.packed();
            self.console_write(
                format!(
                    "beamlet: boot-stats: loads {} (found {found}, of them {packed} from the pack, absent {absent}, \
                     refused {refused}), {bytes} bytes, {us} us in loads, {since} us since the platform started\n",
                    found + absent + refused
                )
                .as_bytes(),
            );
        }
        self.cons.started = true;
        self.cons.read(&mut self.io);
    }

    /// `file` from the module source, for `name`: one the source refuses is said on the console,
    /// naming it and why, and ends this lookup.
    fn load(&mut self, name: &str, file: &str) -> Lookup {
        #[cfg(feature = "boot-stats")]
        let begun = redoubt_rt::handle::time_now().unwrap_or(0);
        let loaded = self.modules.load(file);
        #[cfg(feature = "boot-stats")]
        {
            let l = &mut self.loads;
            l.us += redoubt_rt::handle::time_now().unwrap_or(0).saturating_sub(begun);
            match &loaded {
                Ok(bytes) => (l.found, l.bytes) = (l.found + 1, l.bytes + bytes.len() as u64),
                Err(Unloaded::Absent) => l.absent += 1,
                Err(Unloaded::Refused(_)) => l.refused += 1,
            }
        }
        match loaded {
            Ok(bytes) => Lookup::Found(bytes),
            Err(Unloaded::Absent) => Lookup::Absent,
            Err(Unloaded::Refused(why)) => {
                self.console_write(format!("beamlet: {name} not loaded: {why}\n").as_bytes());
                Lookup::Refused
            }
        }
    }

    /// Takes what the hub has completed, without waiting.
    fn take_completed(&mut self) {
        self.io.take_waiting();
        self.dispatch();
    }

    /// Hands each completion to whoever's request it was.
    fn dispatch(&mut self) {
        while let Some(done) = self.io.completed() {
            let Some(done) = self.cons.take(&mut self.io, done) else { continue };
            // Anything else is no one's: dropped, with its buffer.
            self.files.take(&mut self.io, done);
        }
        while let Some(delivery) = self.io.other() {
            // A wake-up of no thread of the platform's: what it brought is closed.
            if let Some(other) = self.sys.deliver(self.io.wake(), delivery) {
                for handle in other.handles.as_slice().iter().flatten() {
                    let _ = redoubt_rt::handle::close(*handle);
                }
            }
        }
        self.sys.collect();
    }
}

/// The platform's end: what the VM wrote reaches the console first, waiting for it at most
/// [`FLUSH_US`], so the VM's last words are not lost to its exit.
impl Drop for Redoubt {
    fn drop(&mut self) {
        if self.report_io {
            let line = format!(
                "beamlet: io: {} requests through the hub; threads: 1 scheduler, {} waiters\n",
                self.io.requests(),
                self.io.waiters()
            );
            self.console_write(line.as_bytes());
        }
        let until = redoubt_rt::handle::time_now().unwrap_or(0).saturating_add(FLUSH_US);
        while self.cons.writes_pending() {
            let now = redoubt_rt::handle::time_now().unwrap_or(until);
            if now >= until {
                return;
            }
            self.io.wait(until - now);
            self.dispatch();
        }
    }
}

/// Writes `bytes` to `console`, as many requests as it takes.
fn write_all(console: &Console, lend: &mut Lend, bytes: &[u8]) {
    let mut rest = bytes;
    while !rest.is_empty() {
        match console.write(lend, rest) {
            Ok(n) if n > 0 => rest = &rest[n.min(rest.len())..],
            // A console that takes nothing, or has gone, gets no more.
            _ => return,
        }
    }
}

/// Writes `line` and a newline to `console`, as best it can: a console that does not answer
/// leaves nobody to tell. `beamlet` says with it why it exits before there is a VM, and `run` how
/// the VM ended, once the VM's own writes have reached it.
pub fn say(console: &Console, line: &str) {
    if let Ok(mut lend) = Lend::new(1) {
        write_all(console, &mut lend, format!("{line}\n").as_bytes());
    }
}

impl Platform for Redoubt {
    fn monotonic_us(&mut self) -> u64 { redoubt_rt::handle::time_now().unwrap_or(0) }

    /// No wall clock exists until time sync does, in M5 (persist, install, share).
    fn system_time_us(&mut self) -> Option<u64> { None }

    /// Waits on the VM's own endpoint, where the waiters' wake-ups arrive, until a completion or
    /// `deadline`. After the console's end a timer still wants its deadline: the wait then sleeps
    /// until it, rather than returning at once and spinning.
    fn idle(&mut self, deadline: Option<u64>) {
        self.take_completed();
        if !self.cons.input.is_empty() {
            return;
        }
        let timeout = match deadline {
            Some(deadline) => deadline.saturating_sub(self.monotonic_us()),
            // Nothing will arrive, so nothing would wake the VM: return, and it gives up.
            None if (!self.cons.started || self.cons.ended) && !self.files.busy() && !self.sys.busy() => {
                return;
            }
            None => FOREVER,
        };
        self.io.wait(timeout);
        self.dispatch();
    }

    /// Queues `bytes` behind what is already queued, and sends what the console will take; past
    /// [`MAX_OUTPUT`] waiting, waits in place for the console to take some.
    fn console_write(&mut self, bytes: &[u8]) {
        if self.cons.gone {
            return;
        }
        self.cons.output.extend(bytes);
        self.cons.write(&mut self.io);
        while self.cons.output.len() > MAX_OUTPUT && self.cons.writes_pending() {
            self.io.wait(FOREVER);
            self.dispatch();
        }
    }

    /// Asked afresh each time, never cached: the console's size can change.
    fn console_size(&mut self) -> Option<(u16, u16)> { self.console.size(&mut self.lend).ok().flatten() }

    fn console_read(&mut self) -> ConsoleInput {
        if !self.cons.started {
            self.start_reading();
        }
        self.take_completed();
        if !self.cons.input.is_empty() {
            return ConsoleInput::Data(self.cons.input.drain(..).collect());
        }
        if self.cons.ended { ConsoleInput::Eof } else { ConsoleInput::Nothing }
    }

    fn random(&mut self, buf: &mut [u8]) -> Result<(), PlatformError> {
        redoubt_rt::handle::random(buf).map_err(|_| PlatformError::Unavailable)
    }

    fn load_module(&mut self, module: &str) -> Lookup { self.load(module, &format!("{module}.beam")) }

    fn load_app(&mut self, app: &str) -> Lookup { self.load(app, &format!("{app}.app")) }

    /// The namespace's files over 9P ([`files`]).
    fn files(&mut self) -> Option<&mut dyn Files> { Some(self) }

    /// The system's calls ([`system`]).
    fn system(&mut self) -> Option<&mut dyn System> { Some(self) }
}

/// The argument that binds a named handle the VM was given at a prefix of its namespace,
/// `bind=PREFIX=HANDLE` (`bind=/home/alice=littlefsd:data`): the `bind/2` a session performs for
/// itself, for a VM `init` launches, whose namespace holds only `/dev/cons`. Under the steward the
/// session's namespace does this. It creates no authority: the handle was handed already.
pub const BIND: &str = "bind=";

/// The `bind=PREFIX=HANDLE` arguments of `startup`, each prefix clean and absolute and each handle
/// one the block names; otherwise the first argument that is not.
pub fn binds<'a>(startup: &Startup<'a>) -> Result<Vec<(&'a str, Handle)>, &'a str> {
    let mut out = Vec::new();
    for arg in startup.args().filter(|arg| arg.starts_with(BIND)) {
        let bound = arg[BIND.len()..].split_once('=').and_then(|(prefix, name)| {
            let handle = startup.handle(name)?;
            redoubt_rt::path::is_clean_absolute(prefix).then_some((prefix, handle))
        });
        out.push(bound.ok_or(arg)?);
    }
    Ok(out)
}

/// The argument that gives the VM its budget's pages, required on the machine: a program cannot
/// read its own budget (it holds no budget handle), so the manifest that sets the budget says it
/// again here.
pub const BUDGET_PAGES: &str = "budget_pages=";

/// The argument that has the platform say, when the VM ends, how many requests went through the
/// hub and how many threads it ran: the scheduler and the waiters, and no thread per request.
pub const REPORT_IO: &str = "report_io";

/// The argument that has the VM print its memory breakdown at its first prompt
/// ([`beamlet_vm::memory::footprint`]); absent, it prints none.
pub const REPORT_MEMORY: &str = "report_memory";

/// The share of the budget each of the heap and ETS limits gets: one part in this many
/// (docs/userland/beamlet.md, "Limits inside one VM").
pub const LIMIT_SHARE: u64 = 16;

/// The pages of `budget_pages=N` among `args`: exactly one, with N a decimal number above zero;
/// otherwise `None`.
pub fn budget_pages<'a>(args: impl Iterator<Item = &'a str>) -> Option<u64> {
    let mut given = args.filter_map(|arg| arg.strip_prefix(BUDGET_PAGES));
    let (Some(n), None) = (given.next(), given.next()) else { return None };
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok().filter(|&pages| pages > 0)
}

/// The VM's limits for a budget of `budget_pages` pages: one process's heap (`max_heap_words`) and
/// all ETS tables together (`max_ets_words`) each get a sixteenth of it ([`LIMIT_SHARE`]), in
/// machine words: the same bytes at either width. A flooding process peaks at about four times its
/// heap limit (the old heap, the collector's copy and its growth), so with a budget at least twice
/// what the VM uses on its own, one flooding process or table meets its limit, and is killed or
/// refused in Erlang, while the VM still has pages. Without a budget, the VM's defaults.
pub fn limits(budget_pages: Option<u64>) -> Limits {
    let mut limits = Limits::default();
    if let Some(pages) = budget_pages {
        let share =
            pages.saturating_mul(PAGE_SIZE as u64) / LIMIT_SHARE / core::mem::size_of::<usize>() as u64;
        limits.max_heap_words = share;
        limits.max_ets_words = share;
    }
    limits
}

/// Runs `module:function()` in a new VM on the platform of the process started with `startup`,
/// with the natives the shell's modules need, and writes how it ended to the console: the value
/// it returned, or the exception that ended it. The result is the process's exit code: 0 once the
/// function ran, however it ended; 1 if the VM could not start it or failed. The VM's limits
/// are sized to its budget, `budget_pages`, if the embedder knows it ([`limits`]). With
/// `report_memory`, the VM prints its memory breakdown when it first waits for console input.
pub fn run(
    startup: &Startup,
    modules: Box<dyn Modules>,
    module: &str,
    function: &str,
    budget_pages: Option<u64>,
    report_memory: Option<HeapPages>,
) -> u32 {
    // Without a console there is nowhere to say why.
    let Ok(platform) = Redoubt::new(startup, modules) else { return 1 };
    let console = Arc::clone(&platform.console);
    let natives: &'static [NativeSpec] =
        Box::leak([beamlet_crypto::NATIVES, beamlet_re::NATIVES].concat().into_boxed_slice());
    let mut vm =
        Vm::with_config(Box::new(platform), Config { natives, limits: limits(budget_pages), report_memory });
    let (line, code) = match vm.spawn(module, function, |_| Vec::new()) {
        Err(e) => (format!("beamlet: {module}:{function} did not start: {:?} {}", e.class, e.reason), 1),
        Ok(first) => match vm.run(first) {
            Ok(Ok(value)) => (format!("{value}"), 0),
            Ok(Err(e)) => {
                let class = match e.class {
                    Class::Error => "error",
                    Class::Exit => "exit",
                    Class::Throw => "throw",
                };
                (format!("{{'EXCEPTION',{class},{}}}", e.reason), 0)
            }
            Err(e) => (format!("beamlet: {e:?}"), 1),
        },
    };
    // The platform's end sends what the VM wrote first, so the line comes after it.
    drop(vm);
    say(&console, &line);
    code
}
