//! The approval channel's lines (servers/steward.md, "The powerbox and approvals"; sshd.md,
//! "`approve@box`"): what the person types at `approve@box`, read by the steward's reader thread
//! for that channel and turned into the core's `Approve`, `Deny` and `Pending` events. The
//! channel is the one place the steward reads a person's bytes, so the grammar is small and
//! total: a line is `approve ID HASH`, `deny ID` or `pending`, the id sixteen hexadecimal digits
//! and the hash either the sixty-four the screen printed or the eight of the code it printed
//! beside them, fields parted by blanks; anything else is no command, answered `unknown
//! command` and nothing more. The binding (R38) is that the person names what the last screen
//! showed: the server expands a code only from that screen, and the core compares the whole
//! hash (`hash_matches`).
//!
//! [`Lines`] gathers the bytes a read returns into lines: a line ends at `\n` or `\r` (a terminal
//! in raw mode sends Enter as `\r`, a pipe as `\n`, some both), empty lines are nothing, and a
//! line longer than [`MAX_LINE`] is dropped whole, with everything up to its end, so a flood of
//! bytes with no line end holds nothing.

use alloc::vec::Vec;

/// The longest line kept: a command is at most 7 + 1 + 16 + 1 + 64 bytes, and blanks.
pub const MAX_LINE: usize = 256;

/// The hash an approval names: the whole of it, or the screen's four-byte code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hash {
    Full([u8; 32]),
    Code([u8; 4]),
}

impl Hash {
    /// The bytes the wire carries: 32 or 4.
    pub fn bytes(&self) -> &[u8] {
        match self {
            Hash::Full(h) => h,
            Hash::Code(c) => c,
        }
    }
}

/// One command the person typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    Approve { request: u64, hash: Hash },
    Deny { request: u64 },
    Pending,
}

/// The command a line holds, or `None` for anything else.
pub fn parse(line: &[u8]) -> Option<Line> {
    let mut words = line.split(|b| *b == b' ' || *b == b'\t' || *b == b'\r').filter(|w| !w.is_empty());
    let line = match (words.next()?, words.next(), words.next(), words.next()) {
        (b"pending", None, None, None) => Line::Pending,
        (b"deny", Some(id), None, None) => Line::Deny { request: hex_u64(id)? },
        (b"approve", Some(id), Some(hash), None) => {
            Line::Approve { request: hex_u64(id)?, hash: hex_hash(hash)? }
        }
        _ => return None,
    };
    Some(line)
}

fn hex_u64(w: &[u8]) -> Option<u64> {
    if w.len() != 16 {
        return None;
    }
    w.iter().try_fold(0u64, |acc, b| Some((acc << 4) | u64::from(hex_digit(*b)?)))
}

fn hex_hash(w: &[u8]) -> Option<Hash> {
    match w.len() {
        64 => hex_bytes::<32>(w).map(Hash::Full),
        8 => hex_bytes::<4>(w).map(Hash::Code),
        _ => None,
    }
}

fn hex_bytes<const N: usize>(w: &[u8]) -> Option<[u8; N]> {
    let mut out = [0; N];
    for (i, pair) in w.chunks(2).enumerate() {
        out[i] = (hex_digit(pair[0])? << 4) | hex_digit(pair[1])?;
    }
    Some(out)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Bytes gathered into lines, as the reads return them.
#[derive(Debug, Default)]
pub struct Lines {
    buf: Vec<u8>,
    /// The line in progress passed [`MAX_LINE`]: dropped to its end.
    dropping: bool,
    /// Lines dropped for their length since [`Lines::dropped`] last asked.
    dropped: usize,
}

impl Lines {
    pub fn new() -> Lines { Lines::default() }

    /// How many lines were dropped whole for their length since the last ask: each is refused
    /// on the channel, and nothing of it was kept.
    pub fn dropped(&mut self) -> usize { core::mem::take(&mut self.dropped) }

    /// Takes what a read returned and gives each whole, non-empty line it completes, without
    /// its end.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut lines = Vec::new();
        for b in bytes {
            if *b == b'\n' || *b == b'\r' {
                if !self.dropping && !self.buf.is_empty() {
                    lines.push(core::mem::take(&mut self.buf));
                }
                self.buf.clear();
                self.dropping = false;
            } else if self.dropping {
                continue;
            } else if self.buf.len() >= MAX_LINE {
                self.buf.clear();
                self.dropping = true;
                self.dropped += 1;
            } else {
                self.buf.push(*b);
            }
        }
        lines
    }
}
