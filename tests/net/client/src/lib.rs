//! What the net cases' programs (`tests/net`) agree on: the clients' roles, their arguments, the
//! reports a client makes to the judge, the outcomes and the badges.
//!
//! Each net case's manifest makes every client a `servers` entry of its own, handed two badges
//! with the same number ([`badge`]): one to `ipd`, which `ipd`'s `scope=` for that badge confines,
//! and one to `judge`, where it reports. **Nothing a client reports is a verdict** on the network:
//! the judge uses reports to know when to go on (a listener is ready, a slot is held), and every
//! verdict on what reached the network comes from the bench's peers and capture, or from the
//! victim that was supposed to be reached.

#![no_std]

/// Handle names in a client's startup block: the endpoints its manifest entry is handed.
pub const IPD: &str = "ipd";
pub const JUDGE: &str = "judge";
/// The rig's name for where a program it launches reports.
pub const RIG: &str = "rig";

/// A report is a call on `judge` with these words: `[REPORT, event, value, 0]`. The judge answers
/// every report at once with nothing, except [`event::START`], which it holds until the client's
/// turn.
pub const REPORT: u64 = 0x5245_504f;

/// What a report says.
pub mod event {
    /// The client is ready to begin; the answer is its turn. The judge gives no turn before
    /// `ipd` has a link, and sequences the clients (a victim listens before it is attacked).
    pub const START: u64 = 5;
    /// The role ended; the value is its outcome ([`super::code`]). The client then parks: under
    /// `init` an exit is a restart, and a restarted client would act again.
    pub const DONE: u64 = 6;
    /// A listener is listening.
    pub const READY: u64 = 1;
    /// A listener accepted a connection and echoed it; the value is how many so far.
    pub const ACCEPTED: u64 = 2;
    /// `hold` attached (value 0) or was refused (value 1).
    pub const ATTACH: u64 = 3;
    /// `labelled` tried every door; the value has bit n set for each attempt not refused.
    pub const LABELLED: u64 = 4;
}

/// The badges a net case's manifest hands: a client's badge at `ipd` and at the judge are the same
/// number, so the judge knows a client by the badge its reports arrive on.
pub mod badge {
    /// The judge's own at `ipd`: its scope listens only on [`PROBE_PORT`], to learn the link is up.
    pub const PROBE: u64 = 10;
    /// `net-tcp`'s echo client, and its listener.
    pub const ECHO: u64 = 11;
    pub const LISTEN: u64 = 12;
    /// `bench-net-peer`'s connect to an address no peer answers.
    pub const NOWHERE: u64 = 13;
    /// `net-pinned`'s client.
    pub const PIN: u64 = 14;
    /// The port the judge's probe listens on, and closes before anything can connect.
    pub const PROBE_PORT: u16 = 9;
}

/// Outcomes, reported with [`event::DONE`] (and the rig's exit codes). 0 is success for every
/// role; the rest name the step that failed.
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
    /// A connect was accepted and nobody answered: `ipd`'s `ctl` wait ran out (`timeout`).
    pub const TIMED_OUT: u32 = 34;
    /// `pin`: a short read was refused at once instead of parking: an abandoned call was not freed.
    pub const PIN_REFUSED: u32 = 35;
    /// `pin`: a read with nothing coming did not end with `ipd`'s deadline.
    pub const NO_DEADLINE: u32 = 36;
    /// `pin`: a listener's `ctl` read with no peer coming did not end with `ipd`'s deadline.
    pub const NO_CTL_DEADLINE: u32 = 37;
    /// The port `pin` listens on for its `ctl` deadline.
    pub const PIN_LISTEN_PORT: u16 = 8001;
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
    /// Connects to `addr:port`, then `times` reads that its own short timeout abandons while they
    /// are parked, then one read with nothing coming that `ipd`'s deadline must end, then an echo;
    /// then listens on `code::PIN_LISTEN_PORT` and waits on its `ctl` with no peer coming, which
    /// `ipd`'s `ctl` deadline must end.
    Pin,
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
                        "pin" => Role::Pin,
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
