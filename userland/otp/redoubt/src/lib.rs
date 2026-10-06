//! beamlet's `Platform` on Redoubt (docs/userland/beamlet.md, "beamlet on Redoubt"): the VM's
//! one boundary, answered by the client library (`redoubt-client`) and the runtime's kernel calls.
//!
//! This first part serves the console, the clock and randomness; the module source is the
//! embedder's ([`Modules`]): the userland volume's files on the machine, read through a verified
//! volume ([`userland`]), directories on a host; there are no files and no programs
//! yet. What it proves is the shape the rest will take: a call that waits, here a console read,
//! is made by a thread of its own, never by the thread the VM runs on, and its result reaches the
//! VM as a message, which [`Platform::idle`] waits for (beamlet.md,
//! "Asynchronous underneath, synchronous on top"). The VM's thread never waits for input. It does
//! write to the console and ask its size itself, calls a live console answers at once; a console
//! that stops answering them stops the VM until those calls move to the I/O threads.
//!
//! - **The console** is `/dev/cons` in the process's namespace, opened once. The VM's thread writes to it and
//!   asks its size; a reader thread, with its own lend, reads it, and sends what it read to the VM's thread,
//!   eight bytes to a message, on an endpoint of the VM's own, then its end. The two threads share the open
//!   file and nothing else: a second open would take more fids than `consoled` allows one session.
//! - **Time** is the kernel's microseconds since boot, and `system_time_us` is `None`: there is no wall clock
//!   until M5 (persist, install, share) brings one. **Randomness** is the kernel's.
//!
//! The same code runs on the machine and, on a host, on the fake kernel: only how a thread is
//! started and where modules come from differ ([`Threads`], [`Modules`]). [`run`] is the program
//! both run: the machine's `beamlet` and the host's `fake-redoubt`.

#![cfg_attr(not(feature = "fake"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

#[cfg(feature = "fake")]
pub mod fixture;
pub mod userland;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use beamlet_vm::bif::NativeSpec;
use beamlet_vm::memory::HeapPages;
use beamlet_vm::platform::{ConsoleInput, Lookup, Platform, PlatformError};
use beamlet_vm::vm::{Config, Limits};
use beamlet_vm::{Class, Vm};
use redoubt_client::console::Console;
use redoubt_client::ns::Namespace;
use redoubt_client::{Error, Lend};
use redoubt_rt::abi::{FOREVER, PAGE_SIZE};
use redoubt_rt::handle::Endpoint;
use redoubt_rt::ipc::{Delivery, Event};
use redoubt_rt::startup::Startup;

/// Starting a thread that runs as this process: on the machine, the runtime's `thread::spawn`;
/// on a host, a host thread the fake kernel counts as this process. `Send`, as the platform is:
/// the VM's schedulers may share it.
pub trait Threads: Send {
    /// Runs `body` on a new thread; if none can be started, `body` is dropped unrun.
    fn spawn(&self, body: Box<dyn FnOnce() + Send + 'static>) -> Result<(), Error>;
}

/// Where the VM's modules and applications come from, by file name (`lists.beam`, `kernel.app`).
/// `Send`, as [`Threads`] is.
pub trait Modules: Send {
    fn load(&mut self, file: &str) -> Result<Vec<u8>, Unloaded>;
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

/// The badge the reader thread's messages come with: the VM thread's own mint off its endpoint.
const READER: u64 = 1;
/// Word 0 of a message from the reader: bytes follow (word 1 their count, words 2 and 3 them, four
/// each: a word is 32 bits on rv32, and the kernel refuses a wider one).
const BYTES: u64 = 1;
/// Word 0 of a message from the reader: the console has no more input.
const END: u64 = 2;
/// The most bytes one message holds.
const CHUNK: usize = 8;

/// The platform of one VM, on Redoubt.
pub struct Redoubt {
    lend: Lend,
    /// Shared with the reader thread, which reads while this thread writes.
    console: Arc<Console>,
    /// Where the reader thread's messages arrive.
    inbox: Endpoint,
    /// What the reader sent and the VM has not taken yet.
    input: VecDeque<u8>,
    reading: bool,
    ended: bool,
    threads: Box<dyn Threads>,
    modules: Box<dyn Modules>,
    /// What the lookups cost (`boot-stats`).
    #[cfg(feature = "boot-stats")]
    loads: Loads,
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
    /// The platform of a process started with `startup`, whose namespace holds `/dev/cons`.
    pub fn new(
        startup: &Startup,
        threads: Box<dyn Threads>,
        modules: Box<dyn Modules>,
    ) -> Result<Redoubt, Error> {
        let mut lend = Lend::new(1)?;
        let ns = Namespace::from_startup(startup, &mut lend)?;
        let console = Arc::new(Console::open(&ns, &mut lend)?);
        let inbox = Endpoint::create()?;
        Ok(Redoubt {
            lend,
            console,
            inbox,
            input: VecDeque::new(),
            reading: false,
            ended: false,
            threads,
            modules,
            #[cfg(feature = "boot-stats")]
            loads: Loads { started: redoubt_rt::handle::time_now().unwrap_or(0), ..Loads::default() },
        })
    }

    /// Starts the reader thread, the first time input is asked for. A `boot-stats` build says so,
    /// with the time and what the lookups cost: for the shell, its prompt is drawn and waiting.
    fn start_reader(&mut self) {
        #[cfg(feature = "boot-stats")]
        {
            say(&self.console, &format!("beamlet: first console read{}", stamp()));
            let Loads { found, absent, refused, bytes, us, started } = self.loads;
            let since = redoubt_rt::handle::time_now().unwrap_or(0).saturating_sub(started);
            say(
                &self.console,
                &format!(
                    "beamlet: boot-stats: loads {} (found {found}, absent {absent}, refused {refused}), \
                     {bytes} bytes, {us} us in loads, {since} us since the platform started",
                    found + absent + refused
                ),
            );
        }
        self.reading = true;
        let console = Arc::clone(&self.console);
        let to = NonZeroU64::new(READER)
            .ok_or(redoubt_rt::abi::Error::InvalidArgument)
            .and_then(|b| self.inbox.mint(b, None));
        let Ok(to) = to else {
            // With nowhere to send input, there is none.
            self.ended = true;
            return;
        };
        // With no thread to read it, there is no input either.
        if self.threads.spawn(Box::new(move || read_console(&console, &to))).is_err() {
            self.ended = true;
        }
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
                say(&self.console, &format!("beamlet: {name} not loaded: {why}"));
                Lookup::Refused
            }
        }
    }

    /// Takes every message the reader has sent, without waiting.
    fn take_waiting(&mut self) {
        while let Ok(event) = self.inbox.receive(0, 0) {
            self.take(event);
        }
    }

    fn take(&mut self, event: Event) {
        let Event::Send(Delivery { caller, words, handles, .. }) = event else { return };
        // Nothing the reader sends carries a handle; anything that came is closed.
        for handle in handles.as_slice().iter().flatten() {
            let _ = redoubt_rt::handle::close(*handle);
        }
        if caller.badge != READER {
            return;
        }
        match words[0] {
            BYTES => {
                let n = (words[1] as usize).min(CHUNK);
                let mut bytes = [0u8; CHUNK];
                bytes[..4].copy_from_slice(&(words[2] as u32).to_le_bytes());
                bytes[4..].copy_from_slice(&(words[3] as u32).to_le_bytes());
                self.input.extend(&bytes[..n]);
            }
            END => self.ended = true,
            _ => {}
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
/// leaves nobody to tell. `beamlet` says with it why it exits, and `run` how the VM ended.
pub fn say(console: &Console, line: &str) {
    if let Ok(mut lend) = Lend::new(1) {
        write_all(console, &mut lend, format!("{line}\n").as_bytes());
    }
}

/// The reader thread: reads the console until its input ends, and sends what it reads to `to`.
fn read_console(console: &Console, to: &Endpoint) {
    let read = || -> Result<(), Error> {
        let mut lend = Lend::new(1)?;
        let mut buf = [0u8; CHUNK];
        loop {
            let n = console.read(&mut lend, &mut buf)?;
            if n == 0 {
                return Ok(());
            }
            let mut chunk = [0u8; CHUNK];
            chunk[..n].copy_from_slice(&buf[..n]);
            let low = u32::from_le_bytes(chunk[..4].try_into().expect("4 bytes"));
            let high = u32::from_le_bytes(chunk[4..].try_into().expect("4 bytes"));
            // Waits until the VM's thread takes it: a VM that is busy slows the reader down.
            to.send(&[BYTES, n as u64, u64::from(low), u64::from(high)], &[], None, FOREVER)
                .map_err(|(e, _)| Error::from(e))?;
        }
    };
    // Whatever ended the reading, the VM is told there is no more.
    let _ = read();
    let _ = to.send(&[END, 0, 0, 0], &[], None, FOREVER);
}

impl Platform for Redoubt {
    fn monotonic_us(&mut self) -> u64 { redoubt_rt::handle::time_now().unwrap_or(0) }

    /// No wall clock exists until time sync does, in M5 (persist, install, share).
    fn system_time_us(&mut self) -> Option<u64> { None }

    fn idle(&mut self, deadline: Option<u64>) {
        self.take_waiting();
        if !self.input.is_empty() {
            return;
        }
        // After the console's end nothing more arrives, but a timer still wants its deadline: the
        // wait below then sleeps until it, rather than returning at once and spinning.
        let timeout = match deadline {
            Some(deadline) => deadline.saturating_sub(self.monotonic_us()),
            // Nothing will arrive, so nothing would wake the VM: return, and it gives up.
            None if !self.reading || self.ended => return,
            None => FOREVER,
        };
        if let Ok(event) = self.inbox.receive(timeout, 0) {
            self.take(event);
        }
    }

    fn console_write(&mut self, bytes: &[u8]) { write_all(&self.console, &mut self.lend, bytes) }

    /// Asked afresh each time, never cached: the console's size can change.
    fn console_size(&mut self) -> Option<(u16, u16)> { self.console.size(&mut self.lend).ok().flatten() }

    fn console_read(&mut self) -> ConsoleInput {
        if !self.reading {
            self.start_reader();
        }
        self.take_waiting();
        if !self.input.is_empty() {
            return ConsoleInput::Data(self.input.drain(..).collect());
        }
        if self.ended { ConsoleInput::Eof } else { ConsoleInput::Nothing }
    }

    fn random(&mut self, buf: &mut [u8]) -> Result<(), PlatformError> {
        redoubt_rt::handle::random(buf).map_err(|_| PlatformError::Unavailable)
    }

    fn load_module(&mut self, module: &str) -> Lookup { self.load(module, &format!("{module}.beam")) }

    fn load_app(&mut self, app: &str) -> Lookup { self.load(app, &format!("{app}.app")) }
}

/// The argument that gives the VM its budget's pages, required on the machine: a program cannot
/// read its own budget (it holds no budget handle), so the manifest that sets the budget says it
/// again here.
pub const BUDGET_PAGES: &str = "budget_pages=";

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
    threads: Box<dyn Threads>,
    modules: Box<dyn Modules>,
    module: &str,
    function: &str,
    budget_pages: Option<u64>,
    report_memory: Option<HeapPages>,
) -> u32 {
    // Without a console there is nowhere to say why.
    let Ok(platform) = Redoubt::new(startup, threads, modules) else { return 1 };
    let console = Arc::clone(&platform.console);
    let natives: &'static [NativeSpec] =
        Box::leak([beamlet_crypto::NATIVES, beamlet_re::NATIVES].concat().into_boxed_slice());
    let mut vm =
        Vm::with_config(Box::new(platform), Config { natives, limits: limits(budget_pages), report_memory });
    let first = match vm.spawn(module, function, |_| Vec::new()) {
        Ok(pid) => pid,
        Err(e) => {
            say(&console, &format!("beamlet: {module}:{function} did not start: {:?} {}", e.class, e.reason));
            return 1;
        }
    };
    match vm.run(first) {
        Ok(Ok(value)) => {
            say(&console, &format!("{value}"));
            0
        }
        Ok(Err(e)) => {
            let class = match e.class {
                Class::Error => "error",
                Class::Exit => "exit",
                Class::Throw => "throw",
            };
            say(&console, &format!("{{'EXCEPTION',{class},{}}}", e.reason));
            0
        }
        Err(e) => {
            say(&console, &format!("beamlet: {e:?}"));
            1
        }
    }
}
