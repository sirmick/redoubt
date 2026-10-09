//! `piped`: the server of one session's pipes (docs/userland/native.md, "Standard input and
//! output, and pipes"; docs/servers/piped.md).
//!
//! A session starts it when its first pipeline needs it, in a budget carved from its own, and is
//! given the one connection nobody minted ([`server::ROOT_BADGE`]). Through it the session makes a
//! directory per pipe, holding the pipe's two ends, `r` and `w`, and mints for each native stage a
//! connection rooted at exactly one end: its standard input, output or error. A connection rooted
//! at a file reaches nothing else, so a pipe carries no authority: a stage can read the end it was
//! given, or write it, and that is all.
//!
//! **A pipe is a buffer of one page.** A write takes what fits and waits while nothing does; a read
//! takes what is there and waits while nothing is. A waiting call is parked, with no deadline (it
//! waits on another stage, for as long as that takes), and served again whenever a pipe moves;
//! its caller giving up, or dying, is what ends it. An end is held while a connection minted at it
//! lives, or while a fid of the session's is open on it. Once the write end is let go the reader
//! reads what is left and then the end of the stream; once the read end is, a write is refused
//! `state`.
//!
//! **No `unsafe`.** The crate forbids it outright.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod server;

pub use server::{BUDGET, COST, MAX_PIPES, PIPE_BYTES, Pipes, ROOT_BADGE, limits};
