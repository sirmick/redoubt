//! `beamlet`: the Elixir VM on Redoubt, a program `init` starts like any other
//! (docs/userland/beamlet.md, "beamlet on Redoubt").
//!
//!     beamlet budget_pages=N MODULE [FUNCTION]
//!
//! Its arguments, from its startup block, give its budget's pages, which size the VM's limits
//! ([`beamlet_redoubt::limits`]), and name the function it runs, `start` by default; it runs
//! it as `fake-redoubt` does on a host (`beamlet_redoubt::run`), and exits with the code that
//! returns. Its console is `/dev/cons` in its namespace; its threads are the runtime's. Its
//! modules are the userland disk's objects, read whole through its handle `fsd:system`, each
//! checked against `/boot/system.index`, which it reads through its handle `bootfsd` and parses
//! strictly before the VM starts ([`beamlet_redoubt::userland`]). A start module that fails the
//! check parks it: it says why and waits, never exiting, so a tampered disk is not a restart loop
//! that reboots the machine.

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use beamlet_redoubt::userland::{Checked, Index, Objects};
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

/// The exit code for a startup block without a module to run or without `fsd:system`, or for a
/// start module `system.index` does not name.
const USAGE: u32 = 2;
/// The exit code for a `system.index` that could not be read (no `bootfsd`, or it never showed)
/// or is malformed, before the VM starts.
const BAD_INDEX: u32 = 3;
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
/// The bundle's table of the userland disk, in `/boot`.
const INDEX: &str = "system.index";
/// How often, and how far apart in microseconds, `/boot/system.index` is asked for before giving
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
    let Some(system) = startup.handle("fsd:system") else {
        say(startup, "beamlet: no fsd:system handle");
        return USAGE;
    };
    let index = match index_bytes(startup) {
        Ok(bytes) => bytes,
        Err(why) => {
            say(startup, &format!("beamlet: no /boot/{INDEX}: {why}"));
            return BAD_INDEX;
        }
    };
    let index = match Index::parse(&index) {
        Ok(index) => index,
        Err(e) => {
            say(startup, &format!("beamlet: /boot/{INDEX} is malformed, line {}: {}", e.line, e.why));
            return BAD_INDEX;
        }
    };
    let objects = match System::attach(Endpoint::from_handle(system)) {
        Ok(objects) => objects,
        Err(e) => {
            say(startup, &format!("beamlet: fsd:system did not attach: {e:?}"));
            return USAGE;
        }
    };
    let mut modules = Checked::new(index, objects);
    // The start module, checked before the VM runs anything: if it cannot load, the VM parks.
    match modules.load(&format!("{module}.beam")) {
        Ok(_) => {}
        Err(Unloaded::Absent) => {
            say(startup, &format!("beamlet: {module} is not in /boot/{INDEX}"));
            return USAGE;
        }
        Err(Unloaded::Refused(why)) => {
            say(startup, &format!("beamlet: {module} not loaded: {why}; parked"));
            loop {
                let _ = sleep(FOREVER);
            }
        }
    }
    say(startup, &format!("beamlet: {} objects in /boot/{INDEX}, read from fsd:system", modules.named()));
    beamlet_redoubt::run(startup, Box::new(Machine), Box::new(modules), module, function, Some(budget_pages))
}

/// The bytes of `system.index`, the signed bundle's table of the userland disk: `/boot`'s, read
/// through the handle `bootfsd` once `init` has pushed it. Its one source.
fn index_bytes(startup: &Startup) -> Result<Vec<u8>, String> {
    let boot = startup.handle("bootfsd").ok_or_else(|| String::from("no bootfsd handle"))?;
    let read = Boot::attach(Endpoint::from_handle(boot), INDEX).and_then(|mut boot| boot.read(INDEX));
    read.map_err(|e| format!("{e:?}"))
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

/// `/boot`, on a connection to `bootfsd`.
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
        read(&self.boot, &mut self.lend, file, u64::MAX)
    }
}

/// The userland disk's objects, at the root of its `fsd`'s volume.
struct System {
    fsd: Connection,
    lend: Lend,
}

impl System {
    fn attach(endpoint: Endpoint) -> Result<System, Error> {
        let mut lend = Lend::new(LEND_PAGES)?;
        let fsd = Connection::attach(endpoint, &mut lend)?;
        Ok(System { fsd, lend })
    }
}

impl Objects for System {
    fn read(&mut self, name: &str, max: u64) -> Result<Vec<u8>, Error> {
        read(&self.fsd, &mut self.lend, name, max)
    }
}

/// The file `file` on `connection`, read whole from its start, but no more than `max` bytes.
fn read(connection: &Connection, lend: &mut Lend, file: &str, max: u64) -> Result<Vec<u8>, Error> {
    let open = connection.open(lend, file, mode::OREAD)?;
    let mut bytes = Vec::new();
    let mut chunk = alloc::vec![0u8; lend.iounit()];
    let read = loop {
        if bytes.len() as u64 >= max {
            break Ok(());
        }
        match open.read_at(lend, bytes.len() as u64, &mut chunk) {
            Ok(0) => break Ok(()),
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
            Err(e) => break Err(e),
        }
    };
    bytes.truncate(usize::try_from(max).unwrap_or(usize::MAX));
    // The fid goes back to the connection whether or not the read finished.
    let closed = open.close(lend);
    read.and(closed).map(|()| bytes)
}
