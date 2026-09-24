//! What the D3 rig (`tests/net`) and the programs it launches agree on: the roles, their
//! arguments, the reports a program makes to the rig, and the exit codes.
//!
//! A launched program holds two handles: `net`, its connection to `ipd` (granted by the rig), and
//! `rig`, where it reports. It has no console. **Nothing a program reports is a verdict** on the
//! network: the rig uses reports only to know when to go on (a listener is ready, a slot is held),
//! and every verdict on what reached the network comes from the bench's peers and capture, or from
//! the victim that was supposed to be reached.

#![no_std]

/// Handle names in a program's startup block.
pub const NET: &str = "net";
pub const RIG: &str = "rig";

/// A report is a call on `rig` with these words: `[REPORT, event, value, 0]`. The rig answers at
/// once with nothing.
pub const REPORT: u64 = 0x5245_504f;

/// What a report says.
pub mod event {
    /// A listener is listening.
    pub const READY: u64 = 1;
    /// A listener accepted a connection and echoed it; the value is how many so far.
    pub const ACCEPTED: u64 = 2;
    /// `hold` attached (value 0) or was refused (value 1).
    pub const ATTACH: u64 = 3;
    /// `labelled` tried every door; the value has bit n set for each attempt not refused.
    pub const LABELLED: u64 = 4;
}

/// Exit codes. 0 is success for every role; the rest name the step that failed.
pub mod code {
    pub const OK: u32 = 0;
    pub const BAD_ARGS: u32 = 20;
    pub const NO_NET: u32 = 21;
    pub const NO_MEMORY: u32 = 22;
    pub const ATTACH: u32 = 23;
    pub const CLONE: u32 = 24;
    pub const OPEN: u32 = 25;
    /// The `connect` write was refused (for an attacker: what should happen).
    pub const REFUSED: u32 = 26;
    /// A `ctl` read failed or said something else than expected.
    pub const STATUS: u32 = 27;
    pub const WRITE: u32 = 28;
    pub const READ: u32 = 29;
    /// The bytes that came back are not the bytes sent.
    pub const ECHO: u32 = 30;
    pub const LISTEN: u32 = 31;
    pub const CLOSE: u32 = 32;
    pub const REPORT: u32 = 33;
    /// An attacker's connect was accepted and ended in this state + `CONNECTED` (1..=5).
    pub const CONNECTED: u32 = 40;
}

/// What a launched program does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Connect to `addr:port` `times` times, one after another, each round-tripping bytes.
    Echo,
    /// Listen on `port` with `backlog`; echo each connection accepted and report it.
    Listen,
    /// One connect to `addr:port`: an attack, or its control. Exits `REFUSED` or `CONNECTED + state`.
    Connect,
    /// Tries attach, clone, connect, `new_connection` and `grant`, reports the bits of those not
    /// refused, then stays, holding whatever it got, until it is killed. Run in a labelled budget.
    Labelled,
    /// Attaches, reports, and holds its bucket until it is killed.
    Hold,
}

/// A program's arguments: `role=R`, and `addr=A.B.C.D`, `port=P`, `backlog=B`, `times=N` as its
/// role needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Args {
    pub role: Role,
    pub addr: [u8; 4],
    pub port: u16,
    pub backlog: u8,
    pub times: u32,
}

impl Args {
    pub fn parse<'a>(args: impl Iterator<Item = &'a str>) -> Option<Args> {
        let mut parsed = Args { role: Role::Hold, addr: [0; 4], port: 0, backlog: 1, times: 1 };
        let mut role = None;
        for arg in args {
            let (key, value) = arg.split_once('=')?;
            match key {
                "role" => {
                    role = Some(match value {
                        "echo" => Role::Echo,
                        "listen" => Role::Listen,
                        "connect" => Role::Connect,
                        "labelled" => Role::Labelled,
                        "hold" => Role::Hold,
                        _ => return None,
                    })
                }
                "addr" => parsed.addr = dotted(value)?,
                "port" => parsed.port = value.parse().ok()?,
                "backlog" => parsed.backlog = value.parse().ok()?,
                "times" => parsed.times = value.parse().ok()?,
                _ => return None,
            }
        }
        parsed.role = role?;
        Some(parsed)
    }
}

/// `A.B.C.D` as four bytes, network order.
pub fn dotted(text: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut parts = text.split('.');
    for byte in &mut out {
        *byte = parts.next()?.parse().ok()?;
    }
    parts.next().is_none().then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_parse_strictly() {
        let a = Args::parse(["role=echo", "addr=10.0.9.100", "port=7", "times=2"].into_iter()).unwrap();
        assert_eq!(a, Args { role: Role::Echo, addr: [10, 0, 9, 100], port: 7, backlog: 1, times: 2 });
        assert!(Args::parse(["addr=10.0.9.100"].into_iter()).is_none(), "no role");
        assert!(Args::parse(["role=echo", "addr=10.0.9"].into_iter()).is_none());
        assert!(Args::parse(["role=echo", "addr=10.0.9.1.2"].into_iter()).is_none());
        assert!(Args::parse(["role=spy"].into_iter()).is_none());
        assert!(Args::parse(["role=hold", "extra=1"].into_iter()).is_none());
    }
}
