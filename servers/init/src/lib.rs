//! `init`'s reading of the boot manifest (servers/init.md): the manifest decoded from strict JSON
//! ([`manifest`]), checked whole against the machine ([`check`], [`confine`]), and the bound on
//! what the boot will cost `init` in `root` ([`bound`]). All of it is pure, so the host tests and
//! the fuzz target run the code the boot runs.
//!
//! `init` works in a fixed arena (kernel/budgets.md, "The tree from the boot manifest"), so a
//! manifest the arena cannot parse is refused before it is parsed: see [`read`].

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod bound;
pub mod check;
pub mod confine;
pub mod manifest;
pub mod refusal;
pub mod sshkey;

#[cfg(any(test, feature = "fuzz"))]
pub mod fuzz;

pub use check::{Machine, Plan, check};
pub use manifest::Manifest;
use redoubt_rt::abi::PAGE_SIZE;
pub use refusal::Refusal;

/// The arena `init` takes once at its start, in pages, and parses and checks in.
pub const ARENA_PAGES: usize = 256;
/// The parser's heap per input byte at most (servers/wire.md, "Strict JSON": cost is bounded).
pub const JSON_HEAP_PER_BYTE: usize = 32;
/// The share of the arena the parse may take: the rest holds the decoded manifest, the checks
/// and the boot's startup blocks.
pub const PARSE_SHARE: usize = 2;

/// The longest manifest the arena parses.
pub const fn max_manifest(arena_pages: usize) -> usize {
    arena_pages * PAGE_SIZE / PARSE_SHARE / JSON_HEAP_PER_BYTE
}

/// Decodes the manifest's bytes, refusing first one longer than an arena of `arena_pages` can
/// parse.
pub fn read(bytes: &[u8], arena_pages: usize) -> Result<Manifest, Refusal> {
    if bytes.len() > max_manifest(arena_pages) {
        return Err(Refusal::Arena { len: bytes.len() });
    }
    manifest::decode(bytes).map_err(|e| match e {
        manifest::DecodeError::Json(e) => Refusal::Json(e),
        manifest::DecodeError::Schema(e) => Refusal::Schema(e),
    })
}
