//! A slot's reader and its end (servers/sshd.md, "The box's platform"): the reader thread calls
//! the slot's driver with `DATA` for each read and `EOF` once the socket ends, once per
//! connection. The driver may take that `EOF` while it still runs the connection (a client that
//! hangs up first) or only after it has closed the socket (a connection the server ended). The
//! slot is free once the reader's `EOF` is taken, and not before: the reader would otherwise
//! call into the slot's next connection. Here, not in the program, so the host tests drive it.

/// Word 0 of the reader's call: its bytes are in the lend.
pub const DATA: u64 = 2;
/// Word 0 of the reader's last call for a connection: the socket ended.
pub const EOF: u64 = 3;

/// What a reader's call says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read {
    /// Bytes, `n` of them in the call's lend.
    Data,
    /// The socket ended: the reader's last call for this connection.
    End,
}

/// One connection's reader, as its driver has heard it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reader {
    ended: bool,
}

impl Reader {
    /// The reader's call with word 0 `word`. Anything but [`DATA`] ends it, as its last call.
    pub fn took(&mut self, word: u64) -> Read {
        if word == DATA {
            Read::Data
        } else {
            self.ended = true;
            Read::End
        }
    }

    /// Whether the driver must still wait for the reader's last call before the slot is free:
    /// only if it has not taken it already.
    pub fn waits(&self) -> bool { !self.ended }
}
