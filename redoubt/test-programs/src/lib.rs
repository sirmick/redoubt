//! Programs that run inside Redoubt under the test bench (`redoubt/testbench`).
//!
//! They are `no_std`, because `std` is not ported to rv64 yet, and they print through
//! `log-server`, which owns the UART. Everything a client prints travels to the server
//! in lent memory, so plain logging already exercises IPC and the page-table operations
//! behind it.
//!
//! Convention: a test program ends by logging `<NAME> TEST PASSED` or `<NAME> TEST FAILED`.

#![no_std]

use core::fmt::Write;

use xous::{MemoryFlags, MemoryRange, MemorySize, Message, CID};

/// There is no name server yet, so the log server uses a well-known address.
pub const SERVER_ADDRESS: &[u8; 16] = b"redoubt-ipc-tst!";

/// Message IDs understood by `log-server`.
pub mod op {
    /// Scalar: print the four arguments.
    pub const PRINT_SCALARS: usize = 1;
    /// BlockingScalar: reply with the sum of the four arguments.
    pub const SUM: usize = 2;
    /// Borrow: print `valid` bytes of the buffer as UTF-8.
    pub const PRINT: usize = 3;
    /// MutableBorrow: upper-case `valid` bytes of the buffer in place.
    pub const UPPERCASE: usize = 4;
    /// Move: print `valid` bytes of the buffer. The server keeps the page.
    pub const PRINT_AND_KEEP: usize = 5;
}

/// A page of memory that can be lent or moved to a server, and written to as text.
pub struct Page {
    pub range: MemoryRange,
    len: usize,
}

impl Page {
    pub fn new() -> Self {
        let range = xous::map_memory(None, None, 4096, MemoryFlags::R | MemoryFlags::W)
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

/// A connection to `log-server`.
pub struct Logger {
    pub cid: CID,
    page: Page,
}

impl Logger {
    /// Blocks until `log-server` is up.
    pub fn connect() -> Self {
        let sid = xous::SID::from_bytes(SERVER_ADDRESS).unwrap();
        Logger { cid: xous::connect(sid).expect("couldn't connect to log-server"), page: Page::new() }
    }

    pub fn log(&mut self, args: core::fmt::Arguments) {
        self.page.clear();
        self.page.write_fmt(args).ok();
        xous::send_message(self.cid, Message::new_lend(op::PRINT, self.page.range, None, self.page.valid()))
            .expect("couldn't lend to log-server");
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
        xous::yield_slice();
    }
}

/// The attack checker (`attack-checker`): the party that ends an attack case. An attacker's
/// own output can never pass a case, because the attacker could print anything; so when an
/// attacker has made its attempts it tells the checker, and the checker (whose lines log-server
/// marks with the checker's PID) reports that the system is still serving and powers off.
/// See redoubt/README.md, "Writing an attack case".
pub mod checker {
    use xous::Message;

    /// Well-known address of the checker's server.
    pub const ADDRESS: &[u8; 16] = b"redoubt-checker!";
    /// BlockingScalar: the sender has finished its attempts.
    pub const DONE: usize = 1;

    /// Tell the checker this process has finished its attempts. Blocks until it has answered,
    /// which it does just before powering off.
    pub fn done() {
        let sid = xous::SID::from_bytes(ADDRESS).unwrap();
        let cid = xous::connect(sid).expect("couldn't connect to the attack checker");
        xous::send_message(cid, Message::new_blocking_scalar(DONE, 0, 0, 0, 0)).expect("couldn't reach the checker");
    }
}

/// Protocol for the memory attack test (`mem-attack`, `mem-victim`). The victim leaves a secret
/// in pages it frees; the attacker lends it every page it gets, and the victim, not the
/// attacker, says whether any of them held data. See `redoubt/tests/mem-attack.toml`.
pub mod mem {
    /// Well-known address of the victim's server.
    pub const VICTIM_ADDRESS: &[u8; 16] = b"redoubt-mem-vict";
    /// Borrow: a page the attacker got; the victim checks that it holds nothing.
    pub const CHECK: usize = 1;
    /// BlockingScalar: the attacker has lent everything it got.
    pub const DONE: usize = 2;
    /// What the victim writes into the pages it frees.
    pub const SECRET: &[u8; 8] = b"SECRET!!";
}

/// Protocol for the use-after-free attack test (`uaf-*` binaries). A "holder" server
/// keeps a page lent to it by a "victim" that then terminates; a "grabber" tries to
/// reclaim the freed frame. See `redoubt/tests/uaf-lent-page.toml`.
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

/// Wait about `ms` milliseconds, reading the `time` CSR (which the kernel lets U-mode
/// read) but yielding between checks. These processes have no timer preemption, so a pure
/// busy-loop would never let another runnable thread proceed. Used only to order events
/// between processes that cannot otherwise synchronise.
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

pub fn wait_ms(ms: u64) {
    // QEMU virt runs the timer at 10 MHz.
    let deadline = read_time() + ms * 10_000;
    while read_time() < deadline {
        xous::yield_slice();
    }
}
