//! `beamlet`: the Elixir VM on Redoubt, a program `init` starts like any other
//! (docs/userland/beamlet.md, "beamlet on Redoubt").
//!
//!     beamlet budget_pages=N MODULE [FUNCTION]
//!
//! Its arguments, from its startup block, give its budget's pages, which size the VM's limits
//! ([`beamlet_redoubt::limits`]), and name the function it runs, `start` by default; it runs
//! it as `fake-redoubt` does on a host (`beamlet_redoubt::run`), and exits with the code that
//! returns. Its console is `/dev/cons` in its namespace; its threads are the runtime's. Its
//! modules are the userland volume's files, each read whole by its name through its handle
//! `littlefsd:system`, a `littlefsd` that reads the volume through its `verityd`
//! ([`beamlet_redoubt::userland`]). A volume that does not attach, or a start module that cannot
//! be read, parks it: it says why and waits, never exiting, so a tampered disk is not a restart
//! loop that reboots the machine.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;

use beamlet_redoubt::userland::{Disk, Files, Unread, unread};
use beamlet_redoubt::{Modules, Threads, Unloaded};
use redoubt_client::console::Console;
use redoubt_client::file::Connection;
use redoubt_client::ns::Namespace;
use redoubt_client::{Error, Lend};
use redoubt_rt::abi::FOREVER;
use redoubt_rt::handle::{Endpoint, sleep};
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(start);

/// The exit code for a startup block without a module to run or without `littlefsd:system`, or for a
/// start module the userland volume does not hold.
const USAGE: u32 = 2;
/// The exit code for a missing or malformed `budget_pages=N`, before the VM starts: without it
/// the VM's limits would be its defaults, far above any budget.
const BAD_ARGS: u32 = 4;
/// The pages of each thread's stack. The reader thread, the only one, reached 2,832 bytes on rv64
/// and 2,240 on rv32 in beamlet-console, and 5,600 and 4,720 when made to panic at the bottom of
/// its read, the system call, so that the panic's report ran on it too (measured by filling its
/// stack with a pattern); 16 KiB is near three times the deepest, and the stack has no guard page
/// below it.
const STACK_PAGES: usize = 4;
/// The pages of each lend a file is read through: what one read asks for.
const LEND_PAGES: usize = 4;

fn start(startup: &Startup) -> u32 {
    // `budget_pages=N`, anywhere, sizes the VM's limits. The rest are MODULE [FUNCTION].
    let Some(budget_pages) = beamlet_redoubt::budget_pages(startup.args()) else {
        say(startup, "beamlet: no budget_pages=N, or a malformed one, in its arguments");
        return BAD_ARGS;
    };
    let mut args = startup.args().filter(|arg| !arg.starts_with(beamlet_redoubt::BUDGET_PAGES));
    let Some(module) = args.next() else {
        say(startup, "beamlet: no module to run in its arguments");
        return USAGE;
    };
    let function = args.next().unwrap_or("start");
    let Some(system) = startup.handle("littlefsd:system") else {
        say(startup, "beamlet: no littlefsd:system handle");
        return USAGE;
    };
    // A volume served as corrupt refuses every attach: it parks, as a start module that does
    // not load does.
    let files = match System::attach(Endpoint::from_handle(system)) {
        Ok(files) => files,
        Err(e) => park(startup, &format!("beamlet: littlefsd:system did not attach: {e:?}; parked")),
    };
    let mut modules = Disk::new(files);
    // The start module, read before the VM runs anything: if it cannot load, the VM parks.
    match modules.load(&format!("{module}.beam")) {
        Ok(_) => {}
        Err(Unloaded::Absent) => {
            say(startup, &format!("beamlet: {module} is not on the userland volume"));
            return USAGE;
        }
        Err(Unloaded::Refused(why)) => park(startup, &format!("beamlet: {module} not loaded: {why}; parked")),
    }
    #[cfg(not(feature = "boot-stats"))]
    say(startup, &format!("beamlet: {module} read from littlefsd:system"));
    // Stamped only for the boot profile, so every other case sees the line as it was.
    #[cfg(feature = "boot-stats")]
    say(startup, &format!("beamlet: {module} read from littlefsd:system{}", beamlet_redoubt::stamp()));
    beamlet_redoubt::run(startup, Box::new(Machine), Box::new(modules), module, function, Some(budget_pages))
}

/// Says `line` and waits for good: a disk that would only fail again is never a restart loop.
fn park(startup: &Startup, line: &str) -> ! {
    say(startup, line);
    loop {
        let _ = sleep(FOREVER);
    }
}

/// Says why it is exiting on its console, before there is a VM to: as best it can, since a
/// console that does not answer leaves nobody to tell.
fn say(startup: &Startup, line: &str) {
    let open = || -> Result<Console, Error> {
        let mut lend = Lend::new(1)?;
        Console::open(&Namespace::from_startup(startup, &mut lend)?, &mut lend)
    };
    if let Ok(console) = open() {
        beamlet_redoubt::say(&console, line);
    }
}

/// Threads on the machine: the runtime's.
struct Machine;

impl Threads for Machine {
    fn spawn(&self, body: Box<dyn FnOnce() + Send + 'static>) -> Result<(), Error> {
        redoubt_rt::thread::spawn(body, STACK_PAGES)?;
        Ok(())
    }
}

/// The userland volume's files, at the root of its `littlefsd`'s volume.
struct System {
    littlefsd: Connection,
    lend: Lend,
}

impl System {
    fn attach(endpoint: Endpoint) -> Result<System, Error> {
        let mut lend = Lend::new(LEND_PAGES)?;
        let littlefsd = Connection::attach(endpoint, &mut lend)?;
        Ok(System { littlefsd, lend })
    }
}

impl Files for System {
    /// A file `littlefsd` answers `not_found` to at the open is absent; any other refusal, at the open
    /// or on a read, failed.
    fn read(&mut self, name: &str) -> Result<Vec<u8>, Unread> {
        let lend = &mut self.lend;
        let open = self.littlefsd.open(lend, name, mode::OREAD).map_err(|e| unread(e, true))?;
        let mut bytes = Vec::new();
        let mut chunk = alloc::vec![0u8; lend.iounit()];
        let read = loop {
            match open.read_at(lend, bytes.len() as u64, &mut chunk) {
                Ok(0) => break Ok(()),
                Ok(n) => bytes.extend_from_slice(&chunk[..n]),
                Err(e) => break Err(e),
            }
        };
        // The fid goes back to the connection whether or not the read finished.
        let closed = open.close(lend);
        read.and(closed).map(|()| bytes).map_err(|e| unread(e, false))
    }
}
