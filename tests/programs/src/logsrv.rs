//! The log server's code, as a library (docs/testbench.md, rule F). The bundle's first program
//! owns the console and serves the log endpoint, whose receive right the kernel installed last in
//! its table (`rd::log_rx`); every later program holds a send on it in slot 2, badged with its
//! PID (`rd::LOG`).
//!
//! By default that first program is `log-server`. A case whose trusted tester needs the first
//! program's own gifts runs the tester first instead, and it calls [`start`]: its own lines go
//! straight to the console as `[pid 2]`, since the loader numbers bundle programs from 2. With
//! siblings to serve it also calls [`start_serving`].
//!
//! `log-server` also hands the first program's budgets to the first caller of `TAKE_GIFTS`, and
//! to no one after it: first caller wins, so a case that uses it runs its attacker as the only
//! program that asks.
//!
//! The badge only says whose line it is; it never authorizes anything here.

use core::fmt;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::rd::{self, FOREVER, Message, MessageKind, Received};
use crate::{Logger, console, op};

/// The first program's PID: the loader numbers bundle programs from 2.
pub const FIRST_PID: u64 = 2;

/// The lowest badge [`mint_child`] makes (R1). The loader's PIDs are at most 64, so a minted
/// badge can never read as a PID: `console::relay` prints it as `[badge N]`.
pub const CHILD_BADGES: u64 = 0x100;

/// A message's sender as its badge names it: `pid N` below [`CHILD_BADGES`], which only the
/// kernel writes (the loader's PIDs), and `badge N` for any other, which only [`mint_child`] makes.
pub struct Sender(pub u64);

impl fmt::Display for Sender {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.0 < CHILD_BADGES { write!(f, "pid {}", self.0) } else { write!(f, "badge {}", self.0) }
    }
}

/// The log endpoint's receive right, read once by [`start`] or `log-server`.
static LOG_RX: AtomicU32 = AtomicU32::new(0);

/// Every line the server prints in its own name. No variant carries another program's text.
pub enum Line {
    Up,
    ConsoleIrq,
    Listening,
    MovedPage(usize),
    Unexpected(usize),
    /// A byte that arrived on the UART (input from the bench), printed escaped.
    Received(char),
    GiftsGiven(Sender),
    /// Anchored by the bench (`reporter`): no relayed line can start this way.
    Done(Sender),
}

pub fn say(line: Line) {
    match line {
        Line::Up => console::line(format_args!("[server] PID {FIRST_PID} up")),
        Line::ConsoleIrq => console::line(format_args!("[server] holding the console irq")),
        Line::Listening => console::line(format_args!("[server] PID {FIRST_PID} listening")),
        Line::MovedPage(at) => {
            console::line(format_args!("[server] moved page at {at:#x}; its text follows"))
        }
        Line::Unexpected(id) => console::line(format_args!("[server] unexpected message, id {id}")),
        Line::Received(byte) => console::line(format_args!("[server] irq: received {byte:?}")),
        Line::GiftsGiven(to) => console::line(format_args!("[server] gifts given to {to}")),
        Line::Done(by) => console::line(format_args!("[server] done: reported by {by}; still serving")),
    }
}

/// Map the console and take the log endpoint's receive right: the first program's start. Call it
/// before creating any handle (`rd::log_rx`). Returns this program's own logger, which prints
/// straight to the console as `[pid 2]`.
pub fn start() -> Logger {
    let (uart, _) = rd::map_device(rd::CONSOLE_MMIO).expect("the console's mmio handle");
    console::init(uart);
    LOG_RX.store(rd::log_rx(), Ordering::Relaxed);
    Logger::connect()
}

/// Whether this program called [`start`]: it owns the console, and its own loggers print there.
pub(crate) fn started() -> bool { LOG_RX.load(Ordering::Relaxed) != 0 }

/// Serve the loader's siblings on a thread of its own, after [`start`].
pub fn start_serving() {
    fn server(_: usize) { serve(|_| false) }
    rd::thread(server, 0).expect("the log server's thread");
}

/// A send on the log endpoint for a child this program spawns, badged `badge`: its lines print
/// as `[badge N]`, never as a PID's (R1).
pub fn mint_child(badge: u64) -> Result<u32, rd::Error> {
    assert!(badge >= CHILD_BADGES, "a child's log badge must not read as a PID");
    rd::mint_from_handle(LOG_RX.load(Ordering::Relaxed), badge, None)
}

/// The text a caller lent or transferred: `len` bytes of `pages`, at most all of them.
fn text(pages: rd::Pages, len: usize) -> &'static str {
    let len = len.min(pages.npages.get() * rd::PAGE_SIZE);
    // SAFETY: the kernel mapped `pages` here, readable, for as long as the message is held, and
    // the caller replies or unmaps only after the text is printed.
    let bytes = unsafe { core::slice::from_raw_parts(pages.addr as *const u8, len) };
    core::str::from_utf8(bytes).unwrap_or("<invalid utf-8>")
}

/// Serve the log endpoint for ever: `PRINT`, `PRINT_AND_KEEP`, `SUM` and `UPPERCASE`, and any
/// call `extra` answers (it returns whether it replied).
pub fn serve(mut extra: impl FnMut(&Message) -> bool) -> ! {
    let rx = LOG_RX.load(Ordering::Relaxed);
    loop {
        let m = match rd::receive(Some(rx), FOREVER, 1) {
            Ok(Received::Message(m)) => m,
            // The caller is gone; the reply only frees the call.
            Ok(Received::Abandoned(id)) => {
                rd::reply(id.get(), &rd::body([0; rd::WORDS])).ok();
                continue;
            }
            _ => continue,
        };
        // Nothing here takes a handle: dropped at once, so a sender cannot fill this table.
        for handle in m.body.handles.as_slice().iter().flatten() {
            rd::close(handle.index()).ok();
        }
        let id = m.msg_id.get();
        let words = m.body.words;
        match (words[0], m.kind) {
            (op::PRINT, MessageKind::Call { lend: Some(pages) }) => {
                console::relay(m.badge, text(pages, words[1]));
                rd::reply(id, &rd::body([0; rd::WORDS])).ok();
            }
            (op::PRINT_AND_KEEP, MessageKind::Send { transfer: Some(pages) }) => {
                say(Line::MovedPage(pages.addr));
                console::relay(m.badge, text(pages, words[1]));
                rd::unmap(pages.addr, pages.npages.get() * rd::PAGE_SIZE).ok();
            }
            (op::SUM, MessageKind::Call { .. }) => {
                rd::reply(id, &rd::body([words[1] + words[2] + words[3], 0, 0, 0])).ok();
            }
            (op::UPPERCASE, MessageKind::Call { lend: Some(pages) }) => {
                let len = words[1].min(pages.npages.get() * rd::PAGE_SIZE);
                // SAFETY: the kernel lent `pages` here writable until the reply below.
                unsafe { core::slice::from_raw_parts_mut(pages.addr as *mut u8, len) }.make_ascii_uppercase();
                rd::reply(id, &rd::body([0; rd::WORDS])).ok();
            }
            _ if extra(&m) => {}
            (other, kind) => {
                say(Line::Unexpected(other));
                if let MessageKind::Call { .. } = kind {
                    rd::reply(id, &rd::body([rd::Error::InvalidArgument as usize, 0, 0, 0])).ok();
                }
            }
        }
    }
}
