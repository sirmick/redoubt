//! `beamlet`: the Elixir VM on Redoubt, a program `init` starts like any other
//! (docs/userland/beamlet.md, "beamlet on Redoubt").
//!
//!     beamlet budget_pages=N MODULE [FUNCTION]
//!
//! Its arguments, from its startup block, give its budget's pages, which size the VM's limits
//! ([`beamlet_redoubt::limits`]), and name the function it runs, `start` by default; it runs
//! it as `fake-redoubt` does on a host (`beamlet_redoubt::run`), and exits with the code that
//! returns. Its console is `/dev/cons` in its namespace. Its modules are `/boot`'s files, read
//! whole by name through its handle `bootfsd`; its threads are the runtime's.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;

use beamlet_redoubt::{Modules, Threads};
use redoubt_client::console::Console;
use redoubt_client::file::Connection;
use redoubt_client::ns::Namespace;
use redoubt_client::{Error, Lend};
use redoubt_rt::handle::{Endpoint, sleep};
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(start);

/// The exit code for a startup block without a module to run, or without `bootfsd`.
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
/// The pages of the lend `/boot` is read through: what one read asks for.
const LEND_PAGES: usize = 4;
/// How often, and how far apart in microseconds, the first module is asked for before giving
/// up: `init` starts programs before it pushes `/boot`'s public entries, which show only then.
const TRIES: u32 = 200;
const APART: u64 = 10_000;

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
    let Some(boot) = startup.handle("bootfsd") else {
        say(startup, "beamlet: no bootfsd handle");
        return USAGE;
    };
    let file = format!("{module}.beam");
    let modules = match Boot::attach(Endpoint::from_handle(boot), &file) {
        Ok(modules) => modules,
        Err(e) => {
            say(startup, &format!("beamlet: /boot/{file} never showed: {e:?}"));
            return USAGE;
        }
    };
    beamlet_redoubt::run(startup, Box::new(Machine), Box::new(modules), module, function, Some(budget_pages))
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

/// Modules from `/boot`, by file name, on a connection to `bootfsd`.
struct Boot {
    boot: Connection,
    lend: Lend,
}

impl Boot {
    /// Attaches to `bootfsd` at `endpoint`, and waits, for a bounded time, until `first` shows.
    fn attach(endpoint: Endpoint, first: &str) -> Result<Boot, Error> {
        let mut lend = Lend::new(LEND_PAGES)?;
        let boot = Connection::attach(endpoint, &mut lend)?;
        let mut tries = 0;
        while let Err(e) = boot.stat(&mut lend, first) {
            if tries == TRIES {
                return Err(e);
            }
            tries += 1;
            sleep(APART)?;
        }
        Ok(Boot { boot, lend })
    }

    fn read(&mut self, file: &str) -> Result<Vec<u8>, Error> {
        let open = self.boot.open(&mut self.lend, file, mode::OREAD)?;
        let mut bytes = Vec::new();
        let mut chunk = alloc::vec![0u8; self.lend.iounit()];
        let read = loop {
            match open.read_at(&mut self.lend, bytes.len() as u64, &mut chunk) {
                Ok(0) => break Ok(()),
                Ok(n) => bytes.extend_from_slice(&chunk[..n]),
                Err(e) => break Err(e),
            }
        };
        // The fid goes back to the connection whether or not the read finished.
        let closed = open.close(&mut self.lend);
        read.and(closed).map(|()| bytes)
    }
}

impl Modules for Boot {
    /// `/boot` is flat: a name with a path in it is no module's.
    fn load(&mut self, file: &str) -> Option<Vec<u8>> {
        if file.is_empty() || file.contains(['/', '\0']) || file.starts_with('.') {
            return None;
        }
        self.read(file).ok()
    }
}
