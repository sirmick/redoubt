//! A program's first and last moments: the entry point, the startup block, exit codes, and the
//! panic handler.
//!
//! The startup page's address arrives in the first argument register (`a0`): `process_start`'s
//! argument (servers/init.md, "The startup block"), passed on by the loader stub.
//!
//! A panic exits through `process_exit` with [`exit::PANIC`]. If the process holds open calls
//! then, the kernel counts it as a fault and blames the sender of the panicking thread's current
//! call (kernel/processes.md R21: the call it took last, or the parked call it named with
//! `serve`), so a crash on hostile input is blamed however the process died.

use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use redoubt_sys::Handle;

use crate::client::{Client, ClientError};
use crate::handle::Endpoint;
use crate::server::ninep::mode;
use crate::startup::Startup;

/// Exit codes the runtime itself uses, distinct from each other and from success.
pub mod exit {
    pub const OK: u32 = 0;
    /// The program panicked (the code Rust's `std` uses).
    pub const PANIC: u32 = 101;
    /// The startup block did not parse: the parent is broken or hostile.
    pub const BAD_STARTUP: u32 = 102;
}

/// Where the panic handler prints: the `/dev/cons` handle from the startup block, 0 for none.
static CONSOLE: AtomicU32 = AtomicU32::new(0);
/// Set by the first panic, so a panic while reporting one exits at once.
static PANICKING: AtomicBool = AtomicBool::new(false);
/// The program's panic hook ([`set_panic_hook`]), as a function pointer's address; 0 for none.
/// Only ever 0 or a `fn()` stored by `set_panic_hook`.
static PANIC_HOOK: AtomicUsize = AtomicUsize::new(0);
/// Set when the hook is first run, so a panic inside the hook does not run it again.
static HOOK_RAN: AtomicBool = AtomicBool::new(false);
/// Set when the first run of the hook has returned, so a later panicker (another thread) waits for
/// it rather than exiting while the device is still being stopped.
static HOOK_DONE: AtomicBool = AtomicBool::new(false);
/// How many times a later panicker checks [`HOOK_DONE`] before it gives up and goes on to exit:
/// the hook is itself bounded (netd's is one register write and a bounded poll), so this is only
/// a backstop against a hook that never returns, such as one that panicked inside itself.
const HOOK_WAIT_SPINS: u32 = 1 << 24;
/// The fid the panic report uses on the console connection: high, so it does not collide with
/// the program's own (a collision only loses the report).
const PANIC_FID: u32 = 0xffff_fff0;
/// How long each 9P request of a panic report may take (µs): a console that does not answer
/// must not keep a dead program from exiting.
const PANIC_TIMEOUT: u64 = 1_000_000;

/// Declares `main` (any name but `main`) as the program's entry: `fn(&Startup) -> u32`, whose
/// result is the exit code. On the host it declares an empty `main`: host tests call the
/// program's function directly, against a fake kernel.
#[macro_export]
macro_rules! entry {
    ($main:path) => {
        /// The entry point: the loader stub jumps here with the startup page's address in `a0`.
        #[cfg(target_os = "none")]
        #[no_mangle]
        pub extern "C" fn _start(startup: usize) -> ! { $crate::start($main, startup) }

        #[cfg(not(target_os = "none"))]
        #[allow(dead_code)]
        fn main() {}
    };
}

/// Parses the startup block at `block` (0 = none), runs `main`, and exits with its code.
#[cfg(target_os = "none")]
pub fn start(main: fn(&Startup<'static>) -> u32, block: usize) -> ! {
    let startup = match startup_block(block) {
        Ok(startup) => startup,
        Err(_) => crate::handle::process_exit(exit::BAD_STARTUP),
    };
    note_console(&startup);
    crate::handle::process_exit(main(&startup))
}

#[cfg(target_os = "none")]
fn startup_block(addr: usize) -> Result<Startup<'static>, crate::startup::StartupError> {
    use crate::startup::{MAX_BLOCK, StartupError};
    if addr == 0 {
        return Ok(Startup::EMPTY);
    }
    if !addr.is_multiple_of(redoubt_sys::PAGE_SIZE) {
        return Err(StartupError::BadLength);
    }
    // SAFETY: the loader stub passes the address of the startup page the parent mapped into
    // this process (servers/init.md); it is one whole page (checked aligned above), stays
    // mapped for the life of the process, and nothing in this process writes it. A parent that
    // passes a bad address can only fault its own child, which it controls anyway.
    let bytes = unsafe { core::slice::from_raw_parts(addr as *const u8, MAX_BLOCK) };
    Startup::parse(bytes)
}

/// Records what the runtime needs from the startup block: the console, for panic reports.
/// [`start`] calls it; host tests call it themselves.
pub fn note_console(startup: &Startup) {
    if let Some((_, console)) = startup.namespace().find(|(path, _)| *path == "/dev/cons") {
        CONSOLE.store(console.index(), Ordering::Relaxed);
    }
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // The hook first: reporting can wait up to `PANIC_TIMEOUT` on the console, and the hook is
    // what makes the process safe to leave (a driver stopping its device).
    run_panic_hook();
    report_panic(format_args!("{info}"));
    crate::handle::process_exit(exit::PANIC)
}

/// Sets the one function the panic handler runs before anything else, for a program that must
/// put something right before it dies: `netd` resets its device so it stops writing into pages
/// that are about to be freed (servers/netd.md R57). It may be set once; `false` if one was
/// already set. The hook must be short and bounded, must not allocate (the panic may be the
/// heap's) and must not rely on anything but its own atomics: it runs on a thread that has just
/// panicked. A panic inside it skips it and exits.
pub fn set_panic_hook(hook: fn()) -> bool {
    PANIC_HOOK.compare_exchange(0, hook as usize, Ordering::AcqRel, Ordering::Acquire).is_ok()
}

/// Runs the panic hook, once per process. A later panic on another thread finds it already
/// running and waits, bounded, for it to finish, so no thread reaches `process_exit` while the
/// first is still stopping the device. A panic inside the hook itself
/// finds it running too; its wait is the same bounded one, and then it exits.
///
/// Crate-private on the machine: only the panic handler runs it, and any other call would spend
/// the one run. Host tests run it as the handler would (there is no handler on the host).
#[cfg(target_os = "none")]
pub(crate) fn run_panic_hook() { run_hook_once() }

/// Host only: runs the panic hook as the machine's panic handler would (see the target version).
#[cfg(not(target_os = "none"))]
pub fn run_panic_hook() { run_hook_once() }

fn run_hook_once() {
    if HOOK_RAN.swap(true, Ordering::AcqRel) {
        for _ in 0..HOOK_WAIT_SPINS {
            if HOOK_DONE.load(Ordering::Acquire) {
                return;
            }
            core::hint::spin_loop();
        }
        return;
    }
    let hook = PANIC_HOOK.load(Ordering::Acquire);
    if hook == 0 {
        HOOK_DONE.store(true, Ordering::Release);
        return;
    }
    // SAFETY: `PANIC_HOOK` only ever holds 0 or the address of a `fn()` stored by
    // `set_panic_hook` (`hook as usize`), and 0 was ruled out just above, so this is that same
    // function pointer back, with its own type.
    let hook: fn() = unsafe { core::mem::transmute::<usize, fn()>(hook) };
    hook();
    HOOK_DONE.store(true, Ordering::Release);
}

/// Prints a panic report on the console, if the startup block named one. It never allocates
/// from the heap (the panic may be the heap's) and gives up quietly on any failure; a panic
/// inside it prints nothing more.
pub fn report_panic(message: fmt::Arguments) {
    if PANICKING.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Some(console) = Handle::new(CONSOLE.load(Ordering::Relaxed)) {
        let _ = write_console(console, message);
    }
}

fn write_console(console: Handle, message: fmt::Arguments) -> Result<(), ClientError> {
    let mut text = Text { bytes: [0; TEXT], len: 0 };
    // Too long a message is cut, never an error.
    let _ = writeln!(text, "panicked: {message}");
    // One page of lend, from map_anon rather than the heap.
    let mut client = Client::new(Endpoint::from_handle(console), 1)?;
    client.timeout = PANIC_TIMEOUT;
    client.attach(PANIC_FID, "")?;
    let written =
        client.open(PANIC_FID, mode::OWRITE).and_then(|_| client.write(PANIC_FID, 0, text.as_bytes()));
    let _ = client.clunk(PANIC_FID);
    written.map(|_| ())
}

/// Bytes of panic report.
const TEXT: usize = 512;

/// A fixed buffer that keeps what fits.
struct Text {
    bytes: [u8; TEXT],
    len: usize,
}

impl Text {
    fn as_bytes(&self) -> &[u8] { self.bytes.get(..self.len).unwrap_or(&[]) }
}

impl Write for Text {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let room = TEXT - self.len;
        let mut n = s.len().min(room);
        while !s.is_char_boundary(n) {
            n -= 1;
        }
        self.bytes[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        if n < s.len() { Err(fmt::Error) } else { Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static CALLS: AtomicU32 = AtomicU32::new(0);
    static STARTED: AtomicBool = AtomicBool::new(false);
    static RELEASE: AtomicBool = AtomicBool::new(false);

    /// A hook that takes its time: it runs until the test lets it finish.
    fn slow() {
        CALLS.fetch_add(1, Ordering::Relaxed);
        STARTED.store(true, Ordering::Release);
        while !RELEASE.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
    }

    fn other() {}

    /// The hook is set once and runs once, however often the handler asks; and a second
    /// panicking thread waits for the first run to finish rather than going on to exit.
    #[test]
    fn the_panic_hook_is_set_once_runs_once_and_is_waited_for() {
        extern crate std;
        assert!(set_panic_hook(slow));
        assert!(!set_panic_hook(other), "a second hook is refused");
        let first = std::thread::spawn(run_panic_hook);
        while !STARTED.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
        let second = std::thread::spawn(run_panic_hook);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(!second.is_finished(), "a second panicker went on while the hook was running");
        RELEASE.store(true, Ordering::Release);
        first.join().unwrap();
        second.join().unwrap();
        run_panic_hook();
        assert_eq!(CALLS.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn text_keeps_what_fits_at_a_character_boundary() {
        let mut text = Text { bytes: [0; TEXT], len: 0 };
        let long = "é".repeat(TEXT);
        assert!(write!(text, "{long}").is_err());
        assert_eq!(text.len, TEXT);
        assert!(core::str::from_utf8(text.as_bytes()).is_ok());
    }
}
