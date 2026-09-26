//! Programs that run inside Redoubt under the test bench (`tools/testbench`).
//!
//! They are `no_std`, because `std` is not ported to rv64 yet, and they print through the
//! bundle's first program, which owns the UART and serves the log endpoint (`logsrv`; by default
//! `log-server`). Everything a client prints travels there in lent memory, so plain logging
//! already exercises IPC and the page-table operations behind it.
//!
//! Convention: a test program ends by logging `<NAME> TEST PASSED` or `<NAME> TEST FAILED`.
//! An attack program ends with `attempts done` instead: its own verdict would count for nothing
//! (docs/testbench.md, "Writing an attack case").

#![no_std]

use core::fmt::Write;

use redoubt_abi::{CID, MemoryFlags, MemoryRange, MemorySize};

pub mod console;
pub mod logsrv;
pub mod rd;
pub mod sched;
pub mod spawn;

/// There is no name server yet, so the log server uses a well-known address.
pub const SERVER_ADDRESS: &[u8; 16] = b"redoubt-ipc-tst!";

/// A legacy connection to `log-server`, for the programs that still send it legacy messages.
pub fn connect_legacy() -> CID {
    let sid = redoubt_abi::SID::from_bytes(SERVER_ADDRESS).unwrap();
    redoubt_abi::connect(sid).expect("couldn't connect to log-server")
}

/// Operations on the log endpoint (`logsrv`), word 0 of a message; word 1 is a byte count.
pub mod op {
    /// Scalar: print the four arguments.
    pub const PRINT_SCALARS: usize = 1;
    /// Call with words: reply with the sum of words 1-3.
    pub const SUM: usize = 2;
    /// Call with a lend: print word 1 bytes of it as UTF-8.
    pub const PRINT: usize = 3;
    /// Call with a writable lend: upper-case word 1 bytes of it in place.
    pub const UPPERCASE: usize = 4;
    /// Send with a transfer: print word 1 bytes of it. The server keeps the pages.
    pub const PRINT_AND_KEEP: usize = 5;
    /// Call, `log-server` only: the first caller gets `root`, `system` and `users` in the reply
    /// (`rd::take_gifts`), and never a device; a later one gets `Refused` in word 0.
    pub const TAKE_GIFTS: usize = 6;
    /// Call, `log-server` only: the attack checker. It prints `[server] done: reported by pid
    /// N; still serving`, N the caller's badge, replies, and powers the machine off.
    pub const DONE: usize = 7;
}

/// A page of memory that can be lent or moved to a server, and written to as text.
pub struct Page {
    pub range: MemoryRange,
    len: usize,
}

impl Page {
    pub fn new() -> Self {
        let range = redoubt_abi::map_memory(None, None, 4096, MemoryFlags::R | MemoryFlags::W)
            .expect("couldn't allocate a page");
        Page { range, len: 0 }
    }

    pub fn clear(&mut self) { self.len = 0; }

    pub fn bytes(&self) -> &[u8] { unsafe { core::slice::from_raw_parts(self.range.as_ptr(), self.len) } }

    pub fn valid(&self) -> Option<MemorySize> { MemorySize::new(self.len) }
}

impl Default for Page {
    fn default() -> Self { Self::new() }
}

impl Write for Page {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let page = unsafe { core::slice::from_raw_parts_mut(self.range.as_mut_ptr(), self.range.len()) };
        let dest = page.get_mut(self.len..self.len + s.len()).ok_or(core::fmt::Error)?;
        dest.copy_from_slice(s.as_bytes());
        self.len += s.len();
        Ok(())
    }
}

/// A program's log: a send on the log endpoint (`rd::LOG`), or, in the first program once it
/// has called `logsrv::start`, the console itself, as `[pid 2]`.
pub struct Logger {
    endpoint: Option<u32>,
    page: Page,
}

impl Logger {
    /// A logger for this thread: every bundle program but the first logs through its slot-2
    /// handle.
    pub fn connect() -> Self {
        let endpoint = if logsrv::started() { None } else { Some(rd::LOG) };
        Logger { endpoint, page: Page::new() }
    }

    pub fn log(&mut self, args: core::fmt::Arguments) {
        self.page.clear();
        self.page.write_fmt(args).ok();
        let Some(endpoint) = self.endpoint else {
            let text = core::str::from_utf8(self.page.bytes()).unwrap_or("<invalid utf-8>");
            return console::relay(logsrv::FIRST_PID, text);
        };
        let body = rd::body([op::PRINT, self.page.len, 0, 0]);
        rd::call_waiting(endpoint, &body, rd::pages(self.page.range.as_ptr() as usize, 1), rd::FOREVER)
            .expect("couldn't lend to the log server");
    }
}

#[macro_export]
macro_rules! log {
    ($logger:expr, $($arg:tt)*) => { $logger.log(format_args!($($arg)*)) };
}

/// Shared panic behaviour: there is nowhere to report to, so just stop making progress.
/// The test bench notices the missing verdict and times out.
pub fn park() -> ! {
    loop {
        let _ = rd::receive(None, rd::FOREVER, 0);
    }
}

/// The attack checker, `log-server`'s `DONE`: the party that ends an attack case. An attacker's
/// own output can never pass a case, because the attacker could print anything; so a victim
/// whose verdict is in (or, with no victim, the attacker when done) reports to the checker,
/// which names the reporter by its kernel-written badge in a line no relayed text can start
/// with, says the system is still serving, and powers off. The case's `reporter` pins who.
/// See docs/testbench.md, "Writing an attack case".
pub mod checker {
    use crate::{op, rd};

    /// Report to the checker, which names this process and powers off. Blocks until it has
    /// answered, which it does just before powering off.
    pub fn done() {
        rd::call_waiting(rd::LOG, &rd::body([op::DONE, 0, 0, 0]), None, rd::FOREVER)
            .expect("couldn't reach the checker");
    }
}

/// Protocol for the Redoubt IPC cases (WP-K2): `redoubt-server` answers `redoubt-client` over
/// the boot endpoint the kernel gives the bundle's programs. The opcode is word 0 of a call.
pub mod redoubt_ipc {
    pub mod op {
        /// Reply with the badge, account and label count the kernel attached.
        pub const ECHO: usize = 1;
        /// A lend: read its first word, write it back plus one, report the page count.
        pub const LEND: usize = 2;
        /// Take this call and do not reply: an open call (R4a).
        pub const KEEP: usize = 3;
        /// Reply to every parked call, each after `serve`.
        pub const DRAIN: usize = 4;
        /// Set the `max_transfer` the server's next `receive` names (R4).
        pub const MAX_TRANSFER: usize = 5;
        /// `mint` from this call's message id with the badge in word 1, and return the handle.
        pub const MINT_BACK: usize = 6;
        /// Reply carrying word 1 minted handles.
        pub const REPLY_HANDLES: usize = 7;
        /// `serve` a message id this thread does not hold.
        pub const SERVE_BAD: usize = 8;
        /// Start filling the server's own open calls, one caller thread at a time, until its
        /// process holds `MAX_OPEN_CALLS` (R4a).
        pub const SELF_FILL: usize = 9;
        /// Report the abandoned notices, parked calls and sends the server has seen.
        pub const COUNTS: usize = 10;
        /// Word 1 = 0: fill the server's own handle table to `MAX_HANDLES`, so that a
        /// message's handles have nowhere to go and R4 refuses it (answer 116). Word 1 = 1:
        /// empty it again, so the steps that follow can mint.
        pub const FILL_TABLE: usize = 11;
        /// The last call of the script.
        pub const DONE: usize = 12;
    }
}

/// Protocol for the memory attack test (`mem-attack`, `mem-victim`). The victim leaves a secret
/// in pages it frees; the attacker lends it every page it gets, and the victim, not the
/// attacker, says whether any of them held data. See `tests/mem-attack.toml`.
pub mod mem {
    /// Well-known address of the victim's server.
    pub const VICTIM_ADDRESS: &[u8; 16] = b"redoubt-mem-vict";
    /// Borrow: a page the attacker got; the victim checks that it holds nothing.
    pub const CHECK: usize = 1;
    /// BlockingScalar: the attacker has lent everything it got.
    pub const DONE: usize = 2;
    /// What the victim writes into the pages it frees.
    pub const SECRET: &[u8; 8] = b"SECRET!!";
    /// How many pages the victim fills with the secret and frees.
    pub const SECRET_PAGES: usize = 64;
}

/// Protocol for the use-after-free attack test (`uaf-*` binaries). A "holder" server
/// keeps a page lent to it by a "victim" that then terminates; a "grabber" tries to
/// reclaim the freed frame. See `tests/uaf-lent-page.toml`.
pub mod uaf {
    /// Well-known address of the holder server.
    pub const HOLDER_ADDRESS: &[u8; 16] = b"redoubt-uaf-hold";
    /// MutableBorrow: hold this page forever and remember where it is mapped.
    pub const HOLD: usize = 1;
    /// BlockingScalar: reply once a page has been held (a barrier for the victim's terminator thread).
    pub const WAIT_HELD: usize = 2;
    /// BlockingScalar: reply immediately (liveness / ordering for the grabber).
    pub const SYNC: usize = 3;
    /// BlockingScalar: re-read the held page; reply 1 if it still reads back as the victim's data.
    pub const CHECK: usize = 4;
    /// The victim writes this into the page before lending it.
    pub const VICTIM_SENTINEL: &[u8; 8] = b"VICTIM!!";
    /// The grabber writes this into every page it allocates.
    pub const GRABBER_SENTINEL: &[u8; 8] = b"GRABBER!";
}

/// Protocol for the move-a-borrowed-page attack test (`move-borrowed*` binaries). See
/// `tests/move-borrowed-page.toml`.
pub mod move_borrowed {
    /// Well-known address of the attacking server.
    pub const ADDRESS: &[u8; 16] = b"redoubt-mv-borrw";
    /// What the victim writes into the page it lends.
    pub const VICTIM_TEXT: &str = "victim data";
}

/// Protocol for the return-a-clobbered-lent-page attack test (`return-lent*` binaries). See
/// `tests/return-lent-unmapped.toml`.
pub mod return_lent {
    /// Well-known address of the borrower server.
    pub const ADDRESS: &[u8; 16] = b"redoubt-ret-lent";
    /// Fixed user address the lender lends, then attacks from a second thread. Between the
    /// message region (`0x4000_0000`, one superpage) and the default region (`0x6000_0000`).
    pub const LENT_ADDR: usize = 0x5000_0000;
}

/// Read the 64-bit `time` CSR. On rv64 that is one `rdtime`; on rv32 `time` is 32 bits, so
/// combine `rdtimeh`/`rdtime`, retrying if the low word wrapped between the two reads.
pub fn read_time() -> u64 {
    #[cfg(target_arch = "riscv64")]
    {
        let t: u64;
        // SAFETY: reads a counter CSR; no memory effect.
        unsafe { core::arch::asm!("rdtime {}", out(reg) t) };
        t
    }
    #[cfg(target_arch = "riscv32")]
    {
        let (mut hi, mut lo, mut check): (u32, u32, u32);
        // SAFETY: reads counter CSRs; no memory effect.
        unsafe {
            core::arch::asm!(
                "1:",
                "rdtimeh {hi}",
                "rdtime  {lo}",
                "rdtimeh {check}",
                "bne {hi}, {check}, 1b",
                hi = out(reg) hi,
                lo = out(reg) lo,
                check = out(reg) check,
            );
        }
        let _ = check; // scratch: the loop uses it to detect a low-word wrap, Rust does not
        ((hi as u64) << 32) | lo as u64
    }
}

/// Sleep `ms` milliseconds on the kernel's timer: a `receive` with nothing to receive, which
/// answers `Timeout`. Used only to order events between processes that cannot otherwise
/// synchronise, and to wait out `Busy`.
pub fn wait_ms(ms: u64) { let _ = rd::receive(None, ms * 1000, 0); }
