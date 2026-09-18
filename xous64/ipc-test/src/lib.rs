//! Shared protocol for the xous64 IPC bring-up test.
//!
//! `ipc-server` owns the UART and acts as a miniature log server. `ipc-client` has no
//! devices at all; everything it prints goes to the server in lent memory. Between them
//! they cover every Xous message type, and so every page-table operation the kernel
//! performs for IPC: lend, mutable lend, return, and move.

#![no_std]

/// There is no name server yet, so the server uses a well-known address.
pub const SERVER_ADDRESS: &[u8; 16] = b"xous64-ipc-test!";

/// Message IDs understood by the server.
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
