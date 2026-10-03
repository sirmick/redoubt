//! The trusted tester in `init`'s place (docs/testbench.md, "Starting a case's programs" and
//! rule F): the bundle's second entry, the one program the loader starts. It owns the console,
//! echoes UART input, serves the log endpoint (`test_programs::logsrv`) and answers `DONE` (the
//! attack checker: it names the reporter and powers off).
//!
//! It starts the case's other programs itself. The builder lists them in the bundle's
//! `programs` entry, one line each in the case's order: the program's entry name, then the
//! budgets it gets (`root`, `system`, `users`). The tester reads every line before it starts
//! anything, and refuses the boot on any line it cannot parse or that names a program or a
//! budget it does not have. Then each program starts from the bundle's pages through the loader
//! stub, in a budget of its own carved from `system`: an equal share of what `system` has free,
//! weight 1,000. Each gets the boot endpoint in slot 1 (the receive right for the first one
//! started, a send badged with its place for each later one), the log endpoint in slot 2, badged
//! with its place, its own budget in slot 3, and the budgets its line names from slot 4 on, in
//! that order. Each program's exit notice comes to an endpoint the tester gave it alone, and the
//! tester prints it under the program's name and place. The tester is
//! place 2, and the case's later programs 3 on, because the kernel draws every PID but the
//! tester's.
//!
//! It takes the UART through `map_device` on the console MMIO handle and waits for input in
//! `receive` on the console IRQ handle (kernel/devices.md, R5: the kernel masks the source when
//! it fires and the next receive unmasks it; there is no acknowledge call).
//!
//! Attack cases take their verdict from lines an attacker cannot write (docs/testbench.md,
//! "Rule F (trusted verdicts)"): a client's bytes print only through `logsrv`'s relay, prefixed
//! with its badge, and this server's own lines are `logsrv::Line`'s closed set of templates.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use redoubt_rt::startup::StartupBuilder;
use stub::{IMAGE_AT, STACK_TOP, STARTUP_AT, STUB_ENTRY};
use test_programs::bundle::{Bundle, Entry};
use test_programs::logsrv::{self, Line, Refusal};
use test_programs::rd::{self, Error, MemFlags, MessageKind, ResetKind};
use test_programs::{console, op};

redoubt_rt::panic_handler!();

/// The stub's own flat binary (objcopied by `build.rs`).
static STUB_BIN: &[u8] = include_bytes!(env!("STUB_BIN"));

/// A started program's stack: the 32 pages the loader reserves for a boot program's first
/// thread (kernel/memory-layout.md, "Regions"), here all backed.
const STACK_PAGES: usize = 32;
/// The most programs a case starts: `system` has 15 processes.
const MAX_PROGRAMS: usize = 15;
/// The most budgets a line names: `root`, `system` and `users`.
const MAX_BUDGETS: usize = 3;
/// Each started program's weight: a driver's (kernel/budgets.md).
const WEIGHT: u32 = 1000;
/// The place of the first program the tester starts (logsrv::FIRST_PID is the tester's own).
const FIRST_PLACE: u64 = logsrv::FIRST_PID + 1;
/// More than any program's index: a watcher thread's argument packs an endpoint and an index.
const PLACES: usize = 64;

/// Each started program's entry name, by its index in the `programs` entry: where in the bundle
/// it is, and its length. Written before the program's watcher starts, read by that watcher only.
static NAMES: [(AtomicUsize, AtomicUsize); MAX_PROGRAMS] =
    [const { (AtomicUsize::new(0), AtomicUsize::new(0)) }; MAX_PROGRAMS];

/// Set by the echo thread just before its first `receive`: the source is masked until then (R5).
static ECHOING: AtomicBool = AtomicBool::new(false);

/// A thread of its own, blocked in `receive` on the console's IRQ handle (R5). It echoes
/// every byte the bench types, as the server's own line, which is the evidence the attack
/// cases take their verdict from.
fn uart_irq(_: usize) {
    ECHOING.store(true, Ordering::Release);
    loop {
        match rd::receive(Some(rd::CONSOLE_IRQ), rd::FOREVER, 0) {
            // One byte per interrupt, deliberately: the FIFO is left with data in it, so
            // every byte needs its own interrupt and the next `receive` must unmask the
            // source again. (It does not catch a kernel that forgets to *mask* a fired
            // source: QEMU's 16550 raises the controller once per byte pushed rather than
            // from a continuing level, so nothing storms. `uart-irq` says so.)
            Ok(rd::Received::Interrupt) => {
                if let Some(byte) = console::receive() {
                    logsrv::say(Line::Received(byte as char));
                }
            }
            // Nothing else can arrive on an IRQ handle; a refusal means the handle is gone.
            _ => test_programs::park(),
        }
    }
}

/// One program to start: its image in the bundle, and the budget handles its line names.
#[derive(Clone, Copy)]
struct Program {
    image: Entry,
    budgets: [u32; MAX_BUDGETS],
    nbudgets: usize,
}

/// Every line of the `programs` entry, or why the boot is refused. Nothing is started until all
/// of them have been read.
fn read_programs(bundle: &Bundle) -> Result<([Option<Program>; MAX_PROGRAMS], usize), Refusal> {
    let entry = bundle.find(b"programs").ok_or(Refusal::NoProgramsEntry)?;
    let mut programs = [None; MAX_PROGRAMS];
    let mut n = 0;
    for (index, line) in entry.data.split(|b| *b == b'\n').enumerate() {
        let number = index + 1;
        if line.is_empty() {
            continue;
        }
        if !line.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
            return Err(Refusal::Unreadable(number));
        }
        let mut words = line.split(|b| *b == b' ');
        let name = words.next().filter(|w| !w.is_empty()).ok_or(Refusal::Unreadable(number))?;
        let image = bundle.find(name).ok_or(Refusal::UnknownProgram(number))?;
        let mut budgets = [0; MAX_BUDGETS];
        let mut nbudgets = 0;
        for word in words {
            let budget = match word {
                b"root" => rd::ROOT,
                b"system" => rd::SYSTEM,
                b"users" => rd::USERS,
                _ => return Err(Refusal::UnknownBudget(number)),
            };
            if nbudgets == MAX_BUDGETS || budgets[..nbudgets].contains(&budget) {
                return Err(Refusal::Unreadable(number));
            }
            budgets[nbudgets] = budget;
            nbudgets += 1;
        }
        if n == MAX_PROGRAMS {
            return Err(Refusal::TooMany);
        }
        programs[n] = Some(Program { image, budgets, nbudgets });
        n += 1;
    }
    Ok((programs, n))
}

/// Copy `bytes` into fresh pages of this process and move them to `process` at `at`.
/// `spawn`'s `give_*` do not serve here: they give a child a copy of the caller itself, at
/// `spawn`'s own stack and startup addresses, and a launch through the stub gives the stub, an
/// image from the bundle and a startup block at the stub's.
fn give(process: u32, bytes: &[u8], at: usize, flags: MemFlags) -> Result<(), Error> {
    let len = bytes.len().max(1).next_multiple_of(rd::PAGE_SIZE);
    let scratch = rd::map_anon(len, rd::rw())?;
    // SAFETY: `scratch` is `len` bytes this process has just mapped read-write, and
    // `bytes.len() <= len`; `bytes` is the bundle's or this program's, elsewhere.
    unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), scratch as *mut u8, bytes.len()) };
    rd::process_map(process, scratch, at, len, flags)
}

/// Wait for the exit notice of the program at a place, on the endpoint it alone was given, and
/// print it (kernel/processes.md: a launcher tells children apart by their exit endpoints).
/// `arg` is the endpoint's index times `PLACES`, plus the program's index in `programs`.
fn watch(arg: usize) {
    let (exit, index) = ((arg / PLACES) as u32, arg % PLACES);
    let (at, len) = (NAMES[index].0.load(Ordering::Acquire), NAMES[index].1.load(Ordering::Acquire));
    // SAFETY: `at..at + len` is the program's entry name, inside the bundle the loader mapped
    // read-only for as long as this program runs; `start_programs` stored it before this thread.
    let name = unsafe { core::slice::from_raw_parts(at as *const u8, len) };
    let place = FIRST_PLACE + index as u64;
    while let Ok(received) = rd::receive(Some(exit), rd::FOREVER, 0) {
        if let rd::Received::Exit(notice) = received {
            logsrv::say(Line::Ended(name, place, notice));
            return;
        }
    }
}

/// Start `image` through the loader stub (servers/init.md, "Launching through the loader stub")
/// in `budget`, its exit notice to `exit`, with `handles` in its slots 1..n: the stub at
/// `STUB_ENTRY`, a copy of the image at `IMAGE_AT`, a stack, and a startup page naming the copy.
/// The stub parses the image inside the child's budget; this parses no ELF.
fn launch(budget: u32, exit: u32, image: &[u8], handles: &[u32]) -> Result<(), Error> {
    let process = rd::process_create(budget, exit)?;
    give(process, STUB_BIN, STUB_ENTRY, MemFlags::READ | MemFlags::EXECUTE)?;
    give(process, image, IMAGE_AT, rd::rw())?;
    let stack_len = STACK_PAGES * rd::PAGE_SIZE;
    let stack = rd::map_anon(stack_len, rd::rw())?;
    rd::process_map(process, stack, STACK_TOP - stack_len, stack_len, rd::rw())?;
    let block = StartupBuilder::new(handles.len() as u32)
        .image(IMAGE_AT, image.len())
        .finish()
        .map_err(|_| Error::InvalidArgument)?;
    give(process, &block, STARTUP_AT, MemFlags::READ)?;
    rd::process_start(process, STUB_ENTRY, STACK_TOP - 16, STARTUP_AT, handles)
}

/// Start every program, each in an equal share of what `system` has free.
fn start_programs(programs: &[Option<Program>], log: u32) -> Result<(), Error> {
    let n = programs.len() as u64;
    if n == 0 {
        return Ok(());
    }
    let free = rd::usage(rd::SYSTEM)?;
    // A carved budget costs `system` its own page beside the pages it is given.
    let pages = (free.pages_limit - free.pages_usage) / n - 1;
    let processes = (free.processes_limit - free.processes_usage) / n as u32;
    let boot = rd::endpoint_create()?;
    for (index, program) in programs.iter().flatten().enumerate() {
        let place = FIRST_PLACE + index as u64;
        let budget = rd::create(rd::SYSTEM, &rd::spec(pages, processes, WEIGHT))?;
        // The receive right for the first program started; a send badged with its place for
        // each later one.
        let to_boot = if index == 0 { boot } else { rd::mint_from_handle(boot, place, None)? };
        let to_log = rd::mint_from_handle(log, place, None)?;
        let mut handles = [to_boot, to_log, budget, 0, 0, 0];
        handles[3..3 + program.nbudgets].copy_from_slice(&program.budgets[..program.nbudgets]);
        let exit = rd::endpoint_create()?;
        launch(budget, exit, program.image.data, &handles[..3 + program.nbudgets])?;
        logsrv::say(Line::Started(place, program.image.name));
        let name = program.image.name;
        NAMES[index].0.store(name.as_ptr() as usize, Ordering::Release);
        NAMES[index].1.store(name.len(), Ordering::Release);
        rd::thread(watch, exit as usize * PLACES + index)?;
        // The child has its copies (kernel/processes.md, "Creating and starting").
        if index != 0 {
            rd::close(to_boot)?;
        }
        rd::close(to_log)?;
        rd::close(budget)?;
    }
    rd::close(boot)
}

#[no_mangle]
pub extern "C" fn _start(bundle: usize, len: usize) -> ! {
    logsrv::start();
    let log = logsrv::receive_right();
    logsrv::say(Line::Up);
    // Every line first, so a bad one refuses the boot before anything starts.
    // SAFETY: the loader mapped the whole verified bundle here, read-only, `len` bytes from
    // `bundle`, for as long as this program runs (kernel/boot.md).
    let bundle = unsafe { Bundle::at(bundle, len) };
    let programs = bundle.ok_or(Refusal::NoProgramsEntry).and_then(|b| read_programs(&b));
    let (programs, n) = match programs {
        Ok(programs) => programs,
        Err(why) => {
            logsrv::say(Line::Refused(why));
            rd::system_reset(rd::RESET, ResetKind::PowerOff).ok();
            test_programs::park()
        }
    };
    // Echo what arrives on the UART as this server's own lines: irq-attack and uart-irq take
    // their verdict from those. The thread starts before any client can run its first request.
    rd::thread(uart_irq, 0).expect("couldn't spawn the console's irq thread");
    // The bench types only after `Listening`, so the echo thread must be in `receive` by then.
    while !ECHOING.load(Ordering::Acquire) {
        test_programs::wait_ms(1);
    }
    logsrv::say(Line::ConsoleIrq);
    if let Err(e) = start_programs(&programs[..n], log) {
        logsrv::say(Line::StartFailed(e));
        rd::system_reset(rd::RESET, ResetKind::PowerOff).ok();
        test_programs::park()
    }
    logsrv::say(Line::Listening);
    logsrv::serve(|m| {
        let MessageKind::Call { .. } = m.kind else { return false };
        match m.body.words[0] {
            op::DONE => {
                // The badge is the tester's, given with the log endpoint: nobody can report as
                // another.
                logsrv::say(Line::Done(logsrv::Sender(m.badge)));
                rd::reply(m.msg_id.get(), &rd::body([0; rd::WORDS])).ok();
                rd::system_reset(rd::RESET, ResetKind::PowerOff).ok();
                test_programs::park()
            }
            _ => false,
        }
    })
}
