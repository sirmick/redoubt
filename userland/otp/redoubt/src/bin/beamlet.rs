//! `beamlet`: the Elixir VM on Redoubt, a program `init` starts like any other
//! (docs/userland/beamlet.md, "beamlet on Redoubt").
//!
//!     beamlet endpoint=NAME [schedulers=N] [report_memory] [report_io]
//!             [bind=PREFIX=HANDLE]... [principal=NAME [label=NAME:ID]... [context=NAME] [home=/PATH]]
//!             MODULE [FUNCTION]
//!
//! Its startup block gives its budget's pages, which size the VM's limits
//! ([`beamlet_redoubt::limits`]), filled by its launcher from the budget it made for it
//! (`Startup::budget_pages`). Its arguments give the handle its userland volume is reached by, its schedulers
//! ([`beamlet_redoubt::schedulers`], one unless it is told more), ask for its memory
//! breakdown at its first prompt ([`beamlet_redoubt::REPORT_MEMORY`]), say what its I/O cost when
//! it ends ([`beamlet_redoubt::REPORT_IO`]), bind handles it was handed at prefixes of its
//! namespace ([`beamlet_redoubt::BIND`]: its home volume, `bind=/home/alice=littlefsd:data`), tell a
//! session what it is ([`beamlet_redoubt::identity`], which `redoubt:identity/0` gives), and
//! name the function it runs, `start` by default; it runs it as `fake-redoubt` does on a host
//! (`beamlet_redoubt::run`), and exits with the code that returns. Its console is `/dev/cons` in
//! its namespace; its threads are the runtime's. Its modules are the userland volume's files, each
//! read whole by its name through the handle `endpoint=` names, the image's `erofsd:system`, an
//! `erofsd` that reads the volume through its `verityd` ([`beamlet_redoubt::userland`]), and before
//! them the volume's boot pack, read whole once at start ([`beamlet_redoubt::pack`]). A volume that
//! does not attach, a pack that cannot be read or is malformed, or a start module that cannot be
//! read, ends it by its console: with one it says why and waits, never exiting, so a tampered
//! disk is not a restart loop that reboots the machine; without one (a lease's VM) it exits, its
//! code the diagnostic, rather than hold its budget in silence ([`end`]).

#![cfg_attr(target_os = "none", no_std, no_main)]
// On the host the program is only built, never run (`redoubt_rt::entry!`).
#![cfg_attr(not(target_os = "none"), allow(dead_code))]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;

use beamlet_redoubt::pack::{self, Pack};
use beamlet_redoubt::userland::{Disk, Files, Unread, unread};
use beamlet_redoubt::{Modules, Unloaded};
use redoubt_client::console::Console;
use redoubt_client::file::Connection;
use redoubt_client::ns::Namespace;
use redoubt_client::{Error, Lend};
use redoubt_rt::abi::FOREVER;
use redoubt_rt::handle::{Endpoint, sleep};
use redoubt_rt::server::ninep::mode;
use redoubt_rt::startup::Startup;

redoubt_rt::entry!(start);

/// The exit code for a startup block without a module to run, or without the handle `endpoint=`
/// names, or for a start module the userland volume does not hold.
const USAGE: u32 = 2;
/// The exit code for a startup block that tells it no budget, before the VM starts: without it
/// the VM's limits would be its defaults, far above any budget; or for no `endpoint=NAME`.
const BAD_ARGS: u32 = 4;
/// The argument naming the handle the userland volume is reached by.
const ENDPOINT: &str = "endpoint=";
/// The pages of each lend a file is read through: what one read asks for.
const LEND_PAGES: usize = 4;

fn start(startup: &Startup) -> u32 {
    // The block's budget pages size the VM's limits: the launcher's word for the budget it made,
    // which no argument could keep equal to by hand. `report_memory`, anywhere, asks for the
    // breakdown. The rest are MODULE [FUNCTION].
    let Some(budget_pages) = startup.budget_pages() else {
        say(startup, "beamlet: its startup block tells it no budget");
        return BAD_ARGS;
    };
    // `endpoint=NAME`, anywhere: the handle the userland volume is reached by.
    let Some(volume) = startup.args().find_map(|arg| arg.strip_prefix(ENDPOINT)) else {
        say(startup, "beamlet: no endpoint=NAME in its arguments");
        return BAD_ARGS;
    };
    let report_memory = startup.args().any(|arg| arg == beamlet_redoubt::REPORT_MEMORY);
    // `bind=PREFIX=HANDLE` binds a handle it was handed ([`beamlet_redoubt::BIND`]); one naming no
    // handle, or a prefix that is not a clean absolute path, is refused before the VM starts.
    if let Err(arg) = beamlet_redoubt::binds(startup) {
        say(startup, &format!("beamlet: {arg} binds no handle it was given at a clean path"));
        return BAD_ARGS;
    }
    let mut args = startup.args().filter(|arg| {
        !arg.starts_with(ENDPOINT)
            && !arg.starts_with(beamlet_redoubt::BIND)
            && !arg.starts_with(beamlet_redoubt::SCHEDULERS)
            && *arg != beamlet_redoubt::REPORT_MEMORY
            && *arg != beamlet_redoubt::REPORT_IO
            && !arg.starts_with(beamlet_redoubt::PRINCIPAL)
            && !arg.starts_with(beamlet_redoubt::LABEL)
            && !arg.starts_with(beamlet_redoubt::CONTEXT)
            && !arg.starts_with(beamlet_redoubt::HOME)
    });
    let Some(module) = args.next() else {
        say(startup, "beamlet: no module to run in its arguments");
        return USAGE;
    };
    let function = args.next().unwrap_or("start");
    let Some(system) = startup.handle(volume) else {
        say(startup, &format!("beamlet: no {volume} handle"));
        return USAGE;
    };
    // A volume served as corrupt refuses every attach: the VM ends as one whose start module
    // does not load does (`end`).
    let mut files = match System::attach(Endpoint::from_handle(system)) {
        Ok(files) => files,
        Err(e) => return end(startup, &format!("beamlet: {volume} did not attach: {e:?}")),
    };
    // The boot pack, read whole before anything else: a pack that cannot be read, or does not
    // check, ends the VM as a start module that cannot load does.
    let pack = match files.read_pack() {
        Ok(bytes) => match Pack::parse(bytes) {
            Ok(pack) => Some(pack),
            Err(why) => return end(startup, &format!("beamlet: {} refused: {why}", pack::FILE)),
        },
        Err(Unread::Absent) => {
            say(startup, &format!("beamlet: no {} on the userland volume", pack::FILE));
            None
        }
        Err(Unread::Failed(why)) => {
            return end(startup, &format!("beamlet: {} not loaded: {why}", pack::FILE));
        }
    };
    #[cfg(feature = "boot-stats")]
    if let Some(pack) = &pack {
        say(
            startup,
            &format!(
                "beamlet: boot pack read {} bytes, {} entries{}",
                pack.size(),
                pack.len(),
                beamlet_redoubt::stamp()
            ),
        );
    }
    let mut modules = Disk::with_pack(files, pack);
    // The start module, read before the VM runs anything, unless the pack holds it: if it cannot
    // load, the VM ends.
    let start_file = format!("{module}.beam");
    let packed = modules.packs(&start_file);
    let start = if packed { Ok(Vec::new()) } else { modules.load(&start_file) };
    match start {
        Ok(_) => {}
        Err(Unloaded::Absent) => {
            say(startup, &format!("beamlet: {module} is not on the userland volume"));
            return USAGE;
        }
        Err(Unloaded::Refused(why)) => return end(startup, &format!("beamlet: {module} not loaded: {why}")),
    }
    // Where the start module is: the boot pack, or its own file on the volume.
    let from = if packed { "the boot pack" } else { volume };
    #[cfg(not(feature = "boot-stats"))]
    say(startup, &format!("beamlet: {module} read from {from}"));
    // Stamped only for the boot profile, so every other case sees the line as it was.
    #[cfg(feature = "boot-stats")]
    say(startup, &format!("beamlet: {module} read from {from}{}", beamlet_redoubt::stamp()));
    let report_memory = report_memory.then_some(heap_pages as beamlet_vm::memory::HeapPages);
    beamlet_redoubt::run(startup, Box::new(modules), module, function, Some(budget_pages), report_memory)
}

/// The runtime heap's pages, held now and at its peak.
#[cfg(target_os = "none")]
fn heap_pages() -> Option<(u64, u64)> {
    let (now, peak) = redoubt_rt::heap_pages();
    Some((now as u64, peak))
}

/// On the host, where the program is only built, there is no runtime heap.
#[cfg(not(target_os = "none"))]
fn heap_pages() -> Option<(u64, u64)> { None }

/// Ends a VM that cannot start, its volume not attached or its boot pack or start module not
/// read, by what it has to say so on (docs/userland/beamlet.md, "beamlet on Redoubt"). With a
/// console, it says `line` there and waits for good: the line is read where it was said, and a
/// disk that would only fail again is never a restart loop that reboots the machine. Without
/// one, a lease's VM (docs/userland/agents.md, "Leases"), waiting would be silent, a budget held
/// to its deadline with nothing said: it exits, and its exit code, the one thing the steward's
/// `lease ... exited` line carries, is the diagnostic, 1 in a default build and
/// [`beamlet_redoubt::NOT_READ`] under `exit-kinds`.
fn end(startup: &Startup, line: &str) -> u32 {
    match console(startup) {
        Some(console) => {
            beamlet_redoubt::say(&console, &format!("{line}; parked"));
            loop {
                let _ = sleep(FOREVER);
            }
        }
        None => beamlet_redoubt::exit_kind(beamlet_redoubt::NOT_READ),
    }
}

/// The VM's console, `/dev/cons` in its namespace, if it has one that answers.
fn console(startup: &Startup) -> Option<Console> {
    let open = || -> Result<Console, Error> {
        let mut lend = Lend::new(1)?;
        Console::open(&Namespace::from_startup(startup, &mut lend)?, &mut lend)
    };
    open().ok()
}

/// Says why it is exiting on its console, before there is a VM to: as best it can, since a
/// console that does not answer leaves nobody to tell.
fn say(startup: &Startup, line: &str) {
    if let Some(console) = console(startup) {
        beamlet_redoubt::say(&console, line);
    }
}

/// The userland volume's files, at the root of the volume its server serves.
struct System {
    server: Connection,
    lend: Lend,
}

impl System {
    fn attach(endpoint: Endpoint) -> Result<System, Error> {
        let mut lend = Lend::new(LEND_PAGES)?;
        let server = Connection::attach(endpoint, &mut lend)?;
        Ok(System { server, lend })
    }
}

impl System {
    /// The boot pack, read whole into one allocation of its size, in order, in reads of the most
    /// the lend a module file is read through carries: a larger lend would take a larger buffer
    /// in the file server than its heap is sized for.
    fn read_pack(&mut self) -> Result<Vec<u8>, Unread> {
        let lend = &mut self.lend;
        let open = self.server.open(lend, pack::FILE, mode::OREAD).map_err(|e| unread(e, true))?;
        let mut read = || -> Result<Vec<u8>, Unread> {
            let length = open.stat(lend).map_err(|e| unread(e, false))?.length;
            let length = usize::try_from(length).map_err(|_| Unread::Failed(TOO_LARGE))?;
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(length).map_err(|_| Unread::Failed(TOO_LARGE))?;
            bytes.resize(length, 0);
            let mut at = 0;
            while at < length {
                match open.read_at(lend, at as u64, &mut bytes[at..]) {
                    Ok(0) => return Err(Unread::Failed("its file ends before its length")),
                    Ok(n) => at += n,
                    Err(e) => return Err(unread(e, false)),
                }
            }
            Ok(bytes)
        };
        let bytes = read();
        // The fid goes back to the connection whether or not the read finished.
        let closed = open.close(lend).map_err(|e| unread(e, false));
        bytes.and_then(|bytes| closed.map(|()| bytes))
    }
}

/// Why a boot pack too large to hold was not read.
const TOO_LARGE: &str = "it is too large to hold";

impl Files for System {
    /// A file the server answers `not_found` to at the open is absent; any other refusal, at the open
    /// or on a read, failed.
    fn read(&mut self, name: &str) -> Result<Vec<u8>, Unread> {
        let lend = &mut self.lend;
        let open = self.server.open(lend, name, mode::OREAD).map_err(|e| unread(e, true))?;
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
