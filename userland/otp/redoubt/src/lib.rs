//! beamlet's `Platform` on Redoubt (docs/userland/beamlet.md, "beamlet on Redoubt"): the VM's
//! one boundary, answered by the client library (`redoubt-client`) and the runtime's kernel calls.
//!
//! This first part serves the console, the clock and randomness; the module source is the
//! embedder's ([`Modules`]): `/boot` on the machine, directories on a host; there are no files and
//! no programs yet. What it proves is the shape the rest will take: a call that waits, here a
//! console read, is made by a thread of its own, never by the thread the VM runs on, and its
//! result reaches the VM as a message, which [`Platform::idle`] waits for (beamlet.md,
//! "Asynchronous underneath, synchronous on top"). The VM's thread never waits for input. It does
//! write to the console and ask its size itself, calls a live console answers at once; a console
//! that stops answering them stops the VM until those calls move to the I/O threads.
//!
//! - **The console** is `/dev/cons` in the process's namespace, opened once. The VM's thread writes to it and
//!   asks its size; a reader thread, with its own lend, reads it, and sends what it read to the VM's thread,
//!   sixteen bytes to a message, on an endpoint of the VM's own, then its end. The two threads share the open
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

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use beamlet_vm::bif::NativeSpec;
use beamlet_vm::platform::{ConsoleInput, Platform, PlatformError};
use beamlet_vm::vm::Config;
use beamlet_vm::{Class, Vm};
use redoubt_client::console::Console;
use redoubt_client::ns::Namespace;
use redoubt_client::{Error, Lend};
use redoubt_rt::abi::FOREVER;
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
    fn load(&mut self, file: &str) -> Option<Vec<u8>>;
}

/// The badge the reader thread's messages come with: the VM thread's own mint off its endpoint.
const READER: u64 = 1;
/// Word 0 of a message from the reader: bytes follow (word 1 their count, words 2 and 3 them).
const BYTES: u64 = 1;
/// Word 0 of a message from the reader: the console has no more input.
const END: u64 = 2;
/// The most bytes one message holds.
const CHUNK: usize = 16;

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
}

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
        })
    }

    /// Starts the reader thread, the first time input is asked for.
    fn start_reader(&mut self) {
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
                bytes[..8].copy_from_slice(&words[2].to_le_bytes());
                bytes[8..].copy_from_slice(&words[3].to_le_bytes());
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
            let low = u64::from_le_bytes(chunk[..8].try_into().expect("8 bytes"));
            let high = u64::from_le_bytes(chunk[8..].try_into().expect("8 bytes"));
            // Waits until the VM's thread takes it: a VM that is busy slows the reader down.
            to.send(&[BYTES, n as u64, low, high], &[], None, FOREVER).map_err(|(e, _)| Error::from(e))?;
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

    fn load_module(&mut self, module: &str) -> Option<Vec<u8>> {
        self.modules.load(&format!("{module}.beam"))
    }

    fn load_app(&mut self, app: &str) -> Option<Vec<u8>> { self.modules.load(&format!("{app}.app")) }
}

/// Runs `module:function()` in a new VM on the platform of the process started with `startup`,
/// with the natives the shell's modules need, and writes how it ended to the console: the value
/// it returned, or the exception that ended it. The result is the process's exit code: 0 once the
/// function ran, however it ended; 1 if the VM could not start it or failed.
pub fn run(
    startup: &Startup,
    threads: Box<dyn Threads>,
    modules: Box<dyn Modules>,
    module: &str,
    function: &str,
) -> u32 {
    // Without a console there is nowhere to say why.
    let Ok(platform) = Redoubt::new(startup, threads, modules) else { return 1 };
    let console = Arc::clone(&platform.console);
    let natives: &'static [NativeSpec] =
        Box::leak([beamlet_crypto::NATIVES, beamlet_re::NATIVES].concat().into_boxed_slice());
    let mut vm = Vm::with_config(Box::new(platform), Config { natives, ..Default::default() });
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
