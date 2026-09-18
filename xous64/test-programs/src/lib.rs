//! Programs that run inside Xous under the test bench (`xous64/testbench`).
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
pub const SERVER_ADDRESS: &[u8; 16] = b"xous64-ipc-test!";

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
